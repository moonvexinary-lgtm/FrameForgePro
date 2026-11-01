//! End-to-end acceptance for the rotoscoping engine's *deterministic* half: the
//! path that must work with no model, no runtime and no inference.
//!
//! The test walks the whole pipeline exactly as the workspace does — prompts →
//! mask → quality control → brush correction → matting → cache → export — and
//! asserts that each stage's output is what the next one consumes. Nothing here
//! loads weights or starts a process; the model-backed stages are covered by
//! their planning tests in the crate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::PathBuf;

use frameforge_roto::brush::{BrushMode, BrushSettings, Stroke};
use frameforge_roto::cache::{self, MaskKey};
use frameforge_roto::engine::FrameRange;
use frameforge_roto::export::{self, ExportSpec, FrameNaming};
use frameforge_roto::matting::{MatteMode, MattingParams, Plane};
use frameforge_roto::refinement;
use frameforge_roto::selection::{BoxRect, Prompt, SelectionSession};
use frameforge_roto::temporal::{self, MaskSummary, QcLimits, SignalKind};
use frameforge_roto::tracking::{Direction, ObjectTrack, TrackStatus};

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("frameforge-roto-pipeline-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temp root");
    root
}

/// A moving square mask at the given position, as a plane.
fn subject(width: u32, height: u32, x: u32, y: u32, size: u32) -> Plane {
    let mut plane = Plane::new(width, height, 0).expect("plane");
    for py in y..(y + size).min(height) {
        for px in x..(x + size).min(width) {
            plane.set(px, py, 255);
        }
    }
    plane
}

fn summarise(frame: i64, plane: &Plane) -> MaskSummary {
    MaskSummary::from_alpha(frame, plane.data(), plane.width(), plane.height(), 128).expect("summary")
}

#[test]
fn the_whole_deterministic_pipeline_runs_without_a_model() {
    let work = root("full");
    const W: u32 = 96;
    const H: u32 = 54;

    // 1. Selection: the user clicks the subject and draws a box around it.
    let mut session = SelectionSession::new();
    session.add_point(20.0, 20.0, true, 0);
    session.add_point(70.0, 40.0, false, 0);
    session.set_box(BoxRect::from_corners(10.0, 10.0, 34.0, 34.0), 0);
    assert_eq!(session.positives(), 1);
    assert_eq!(session.negatives(), 1);
    assert!(session.for_frame(0).iter().any(|prompt| matches!(prompt, Prompt::Box { .. })));
    assert!(session.for_frame(0).iter().any(|prompt| matches!(prompt, Prompt::Point { positive: false, .. })));

    // 2. The mask a selection engine would return (here: the truth we painted).
    let first = subject(W, H, 10, 10, 24);
    assert_eq!(first.covered(128), 24 * 24, "the selection's mask is the subject, nothing else");
    let first_summary = summarise(0, &first);
    assert_eq!(first_summary.bbox, Some((10, 10, 33, 33)), "and it is where the box was drawn");
    let mut track = ObjectTrack::new(1, "Subject");
    track.plan(FrameRange::new(0, 5), Direction::Forward);
    assert_eq!(track.status, TrackStatus::Untracked);
    assert_eq!(track.total_frames(), 6);

    // 3. Propagation results, with a deliberate break at frame 3.
    let propagated = [
        subject(W, H, 10, 10, 24),
        subject(W, H, 12, 11, 24),
        subject(W, H, 14, 12, 24),
        // A tracking failure: the mask jumps to the far corner and shrinks.
        subject(W, H, 70, 40, 8),
        subject(W, H, 70, 40, 9),
        subject(W, H, 70, 40, 9),
    ];
    let summaries: Vec<MaskSummary> = propagated.iter().enumerate().map(|(index, plane)| summarise(index as i64, plane)).collect();
    let report = temporal::analyze(&summaries, &QcLimits::default());
    assert!(!report.is_clean(), "the jump must be caught: {}", report.summary());
    let kinds: Vec<SignalKind> = report.signals.iter().map(|signal| signal.kind).collect();
    assert!(kinds.iter().any(|kind| kind.suggested_action() == frameforge_roto::RecoveryAction::BrushCorrect), "{kinds:?}");
    let flagged = report.review_frames();
    assert!(!flagged.is_empty());
    assert!(report.reinit_from.is_some());

    // 4. The user corrects the bad frame by hand: erase the wrong mask, paint the
    //    subject where it really is.
    let mut corrected = propagated[3].clone();
    let mut erase = Stroke::new(3, BrushSettings { radius: 10.0, hardness: 1.0, mode: BrushMode::Subtract });
    erase.push(74.0, 44.0);
    erase.apply(&mut corrected);
    let mut paint = Stroke::new(3, BrushSettings { radius: 10.0, hardness: 1.0, mode: BrushMode::Add });
    paint.push(20.0, 16.0);
    paint.apply(&mut corrected);
    assert_eq!(corrected.get(70, 40), Some(0), "the wrong mask is gone");
    assert_eq!(corrected.get(20, 16), Some(255), "the subject is painted back");
    track.correct(3, FrameRange::new(0, 5));
    assert!(track.corrections.contains(&3));
    assert_eq!(track.status, TrackStatus::NeedsReview, "a correction puts the run back into review");
    assert!(track.segments.iter().any(|segment| segment.from_correction == Some(3)), "re-propagation starts at the correction: {:?}", track.segments);

    // 5. Refinement: clean the corrected mask and fill a polygon the user drew.
    let cleaned = refinement::despeckle(&refinement::close(&corrected, 1), 3);
    assert!(cleaned.covered(128) > 0);
    let mut filled = cleaned.clone();
    refinement::fill_polygon(&mut filled, &[(16.0, 12.0), (28.0, 12.0), (28.0, 24.0), (16.0, 24.0)]);
    assert_eq!(filled.get(22, 18), Some(255));

    // 6. Matting: a soft, slightly expanded matte from the corrected mask.
    let params = MattingParams { feather: 2.0, expand: 1.0, smooth: 0.0, invert: false };
    let alpha = frameforge_roto::matting::matte(&cleaned, &params, MatteMode::Alpha);
    assert!(alpha.data().iter().any(|value| *value > 0 && *value < 255), "the matte has a soft edge");
    let tolerance = cleaned.covered(128) / 10 + 4;
    assert!(
        alpha.covered(128) + tolerance >= cleaned.covered(128),
        "the matte keeps the mask's coverage within the feather's own margin: {} vs {}",
        alpha.covered(128),
        cleaned.covered(128)
    );

    // 7. Cache: publish the corrected run for the object and read it back.
    let key = MaskKey {
        model_id: "cutie-base".to_string(),
        weights_sha256: "ab".repeat(32),
        source_identity: "item:1:file:clip.mp4:0f1e2d3c4b5a6978:1048576".to_string(),
        width: W,
        height: H,
        frame_rate_num: 24,
        frame_rate_den: 1,
        object_ids: vec![1],
        correction_frames: vec![3],
        pipeline: 1,
    };
    let cache_root = work.join("cache");
    let published = cache::publish(&cache_root, &key, 1, 0, std::slice::from_ref(&cleaned), MatteMode::Alpha).expect("publish the corrected mask");
    assert_eq!(published, 1);
    let loaded = cache::load(&cache_root, &key, 1, 0).expect("load").expect("frame 0");
    assert_eq!(loaded.covered(128), cleaned.covered(128), "the cache returns the exact bytes it was given");
    assert_eq!(loaded.get(20, 16), Some(255));

    // A different model must not read that cache.
    let mut other_key = key.clone();
    other_key.model_id = "xmem".to_string();
    assert!(cache::load(&cache_root, &other_key, 1, 0).is_err(), "another backend addresses another cache");

    // 8. Export: alpha sequence plus a transparent-background RGBA sequence.
    let frames = vec![(0i64, alpha.clone()), (3i64, alpha.clone())];
    let alpha_dir = work.join("alpha");
    export::write_mask_sequence(&alpha_dir, &frames, &ExportSpec::default(), Some(&key.digest()), Some(1)).expect("alpha sequence");
    assert_eq!(export::written_frames(&alpha_dir, FrameNaming::Alpha).len(), 2);
    let manifest = export::read_manifest(&alpha_dir).expect("manifest");
    assert_eq!(manifest.cache_key.as_deref(), Some(key.digest().as_str()));
    assert_eq!(manifest.object_id, Some(1));

    let rgba_dir = work.join("rgba");
    let rgb: Vec<u8> = (0..(W as usize * H as usize)).flat_map(|_| [200u8, 100, 50]).collect();
    export::write_rgba_sequence(&rgba_dir, &frames, &rgb, &ExportSpec::rgba(true), Some(&key.digest()), Some(1)).expect("rgba sequence");
    let first_rgba = export::written_frames(&rgba_dir, FrameNaming::Frame).into_iter().next().expect("a frame");
    let saved = image::open(&first_rgba).expect("png").to_rgba8();
    assert_eq!(saved.get_pixel(0, 0).0[3], 0, "the background is transparent");
    assert_eq!(saved.get_pixel(22, 18).0[3], 255, "the subject is opaque");
    assert!(export::read_manifest(&rgba_dir).expect("manifest").premultiplied);

    // 9. The queue would have carried all of that, and still runs nothing by itself.
    let mut queue = frameforge_roto::scheduler::Queue::default();
    let tracking = queue.enqueue(frameforge_roto::scheduler::JobKind::Tracking, "cutie-base", Some(1), FrameRange::new(0, 5), 5);
    let export_job = queue.enqueue(frameforge_roto::scheduler::JobKind::Export, "", Some(1), FrameRange::new(0, 5), 0);
    assert!(queue.running() == 0, "enqueuing starts nothing");
    assert_eq!(queue.start_next(), Some(tracking));
    queue.pause(tracking);
    queue.resume(tracking);
    queue.cancel_all();
    assert!(queue.job(export_job).expect("job").status.is_terminal());
}

#[test]
fn a_projects_masks_survive_a_cache_round_trip_for_every_object() {
    let work = root("objects");
    const W: u32 = 32;
    const H: u32 = 18;
    let key = MaskKey {
        model_id: "sam2.1-tiny".to_string(),
        weights_sha256: "cd".repeat(32),
        source_identity: "item:9:generator:ColorMatte".to_string(),
        width: W,
        height: H,
        frame_rate_num: 24,
        frame_rate_den: 1,
        object_ids: vec![1, 2],
        correction_frames: vec![],
        pipeline: 1,
    };
    let cache_root = work.join("cache");
    let one = subject(W, H, 2, 2, 8);
    let two = subject(W, H, 20, 8, 6);

    cache::publish(&cache_root, &key, 1, 0, &[one.clone(), one.clone()], MatteMode::Alpha).expect("object 1");
    cache::publish(&cache_root, &key, 2, 0, std::slice::from_ref(&two), MatteMode::Alpha).expect("object 2");

    assert_eq!(cache::cached_objects(&cache_root, &key), vec![1, 2]);
    let loaded_one = cache::load(&cache_root, &key, 1, 1).expect("load").expect("frame 1");
    let loaded_two = cache::load(&cache_root, &key, 2, 0).expect("load").expect("frame 0");
    assert_eq!(loaded_one.covered(128), one.covered(128));
    assert_eq!(loaded_two.covered(128), two.covered(128));
    assert!(cache::load(&cache_root, &key, 2, 5).expect("outside the range").is_none(), "object 2 has one frame");

    // Combining the two objects gives the union the export would write.
    let union = refinement::combine(&loaded_one, &loaded_two, refinement::BoolOp::Union).expect("same size");
    assert_eq!(union.covered(128), one.covered(128) + two.covered(128));
}
