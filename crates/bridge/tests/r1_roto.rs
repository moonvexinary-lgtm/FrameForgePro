//! R1 acceptance: cached roto masks become the clip's alpha on the one
//! composed-source path that preview and export both use.
//!
//! The masks here are synthetic PNGs written into the documented cache layout.
//! No XMem, PyTorch or weights are involved: R1 proves the Rust seam — cache
//! lookup, key invalidation, alpha multiplication, preview and export — before
//! any model is allowed to feed it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use filmcraft_export::{ExportSettings, Format, Progress, export};
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{FrameRequest, Generator, MediaSource, open_bytes};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, Project, SequenceSettings, TrackKind};
use filmcraft_render::{RenderOptions, render_sequence};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
use frameforge_bridge::{ComposedSource, Composition, FrameForgeProject, MaskCache, ProjectSources, RotoCacheKey, RotoRef, XMEM_COMMIT};

const R: FrameRate = FrameRate::FPS_24;
const W: u32 = 64;
const H: u32 = 36;
const FRAMES: i64 = 24;
const DURATION: Tick = Tick(TICKS_PER_SECOND);

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("frameforge-r1-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("temporary roto root");
    root
}

fn key(corrections: Vec<u64>) -> RotoCacheKey {
    RotoCacheKey {
        source_identity: "synthetic://source:64x36:sha256=test".to_string(),
        model_sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        width: W,
        height: H,
        frame_rate: R,
        correction_ids: corrections,
        xmem_commit: XMEM_COMMIT.to_string(),
    }
}

/// Write the documented cache layout: `manifest.json` plus one 8-bit grayscale
/// PNG per covered frame. This is the same shape the R2 worker will produce.
fn write_cache(root: &Path, key: &str, frames: &[Vec<u8>], declared_frames: i64) {
    let dir = root.join(key);
    std::fs::create_dir_all(&dir).expect("cache directory");
    let manifest = serde_json::json!({
        "version": 1,
        "key": key,
        "width": W,
        "height": H,
        "frame_rate": R,
        "frame_start": 0,
        "frame_count": declared_frames,
    });
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).expect("manifest JSON")).expect("manifest");
    for (i, alpha) in frames.iter().enumerate() {
        let image = image::GrayImage::from_raw(W, H, alpha.clone()).expect("mask dimensions");
        image.save(dir.join(format!("frame_{i:08}.png"))).expect("mask PNG");
    }
}

/// Zero alpha on the left half, opaque on the right half.
fn left_half_transparent() -> Vec<u8> {
    let mut alpha = vec![0u8; (W * H) as usize];
    for y in 0..H as usize {
        for x in (W as usize / 2)..W as usize {
            alpha[y * W as usize + x] = 255;
        }
    }
    alpha
}

fn media_clip(media: MediaRef, info: filmcraft_media::MediaInfo) -> ItemKind {
    ItemKind::Media(MediaClip {
        media,
        info,
        interpret: Default::default(),
        mark_in: None,
        mark_out: None,
        markers: Vec::new(),
        offline: false,
        proxy: None,
        identity: None,
    })
}

fn color_item(p: &mut Project, color: [f32; 4], name: &str) -> ItemId {
    let generator = Generator::ColorMatte { color };
    let info = GeneratorSource::new(generator.clone(), W, H, R, DURATION).with_name(name).info().clone();
    p.add_item(name, Label::Iris, media_clip(MediaRef::Generator(generator), info), None)
}

fn add_clip(p: &mut Project, seq: ItemId, media: ItemId, track: usize) {
    let item = p.make_track_item(media, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, R.tick_of(FRAMES)), R).expect("track item");
    let sequence = p.sequence_mut(seq).expect("sequence");
    sequence.video_tracks[track].items.push(item);
    sequence.video_tracks[track].sort();
}

fn channel(p: [f32; 4], i: usize) -> f32 {
    p[i]
}

fn strong(px: [f32; 4], channel_i: usize, other_i: usize) -> bool {
    channel(px, channel_i) > channel(px, other_i) + 0.2
}

#[test]
fn mask_cache_applies_exact_alpha_inside_coverage_and_leaves_other_frames_unmasked() {
    let root = temp_root("alpha");
    let cache_key = key(vec![]).cache_key();
    write_cache(&root, &cache_key, &[left_half_transparent()], 1);

    let cache = MaskCache::open(&root, &cache_key).expect("cached mask");
    let generator = Arc::new(GeneratorSource::new(Generator::ColorMatte { color: [1.0, 0.0, 0.0, 1.0] }, W, H, R, DURATION));
    let source = ComposedSource::new(generator, Composition::default(), 7).with_roto_cache(cache);

    let frame = source.video_frame(FrameRequest::full(R.tick_of(0))).expect("masked frame");
    let px = frame.to_rgba8();
    let at = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    assert_eq!(at(0, 0), [255, 0, 0, 0], "left half must be fully transparent");
    assert_eq!(at(W - 1, H - 1), [255, 0, 0, 255], "right half must stay opaque");
    assert_eq!(at(W / 2, 0), [255, 0, 0, 255], "the boundary pixel belongs to the opaque half");

    let outside = source.video_frame(FrameRequest::full(R.tick_of(10))).expect("frame outside coverage");
    assert_eq!(outside.to_rgba8()[3], 255, "frames outside the cached range must not be blanked");
}

#[test]
fn mask_cache_reports_a_missing_frame_inside_coverage() {
    let root = temp_root("missing-frame");
    let cache_key = key(vec![]).cache_key();
    write_cache(&root, &cache_key, &[vec![255; (W * H) as usize], vec![255; (W * H) as usize]], 2);
    std::fs::remove_file(root.join(&cache_key).join("frame_00000001.png")).expect("remove one mask");

    let cache = MaskCache::open(&root, &cache_key).expect("cached mask");
    let generator = Arc::new(GeneratorSource::new(Generator::ColorMatte { color: [1.0, 0.0, 0.0, 1.0] }, W, H, R, DURATION));
    let source = ComposedSource::new(generator, Composition::default(), 7).with_roto_cache(cache);

    let error = source.video_frame(FrameRequest::full(R.tick_of(1))).expect_err("a missing mask frame is not silently unmasked");
    let message = error.to_string();
    assert!(message.contains("roto") && message.contains("frame 1"), "the error must name the missing mask frame: {message}");
}

#[test]
fn cache_key_changes_with_every_invalidation_input() {
    let base = key(vec![7, 9]);
    let base_key = base.cache_key();
    assert_eq!(base_key.len(), 16, "the key is a stable 64-bit hex digest");
    assert!(base_key.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(base.cache_key(), base_key, "the same inputs must address the same cache");

    let mut source = base.clone();
    source.source_identity.push_str(":edited");
    let mut model = base.clone();
    model.model_sha256 = "1111111111111111111111111111111111111111111111111111111111111111".to_string();
    let mut width = base.clone();
    width.width += 1;
    let mut height = base.clone();
    height.height += 1;
    let mut rate = base.clone();
    rate.frame_rate = FrameRate::FPS_25;
    let mut corrections = base.clone();
    corrections.correction_ids = vec![7, 10];
    let mut correction_order = base.clone();
    correction_order.correction_ids = vec![9, 7];
    let mut commit = base.clone();
    commit.xmem_commit = "0000000000000000000000000000000000000000".to_string();

    for (name, variant) in [
        ("source identity", source),
        ("model hash", model),
        ("working width", width),
        ("working height", height),
        ("frame rate", rate),
        ("correction ids", corrections),
        ("correction order", correction_order),
        ("XMem commit", commit),
    ] {
        assert_ne!(variant.cache_key(), base_key, "{name} must invalidate the cache");
    }
}

#[test]
fn changed_corrections_miss_the_previous_cache() {
    let root = temp_root("invalidation");
    let old = key(vec![1]).cache_key();
    let new = key(vec![2]).cache_key();
    assert_ne!(old, new);
    write_cache(&root, &old, &[left_half_transparent()], 1);

    assert!(MaskCache::open(&root, &old).is_ok(), "the old cache is readable");
    assert!(MaskCache::open(&root, &new).is_err(), "a correction edit must not reuse the old mask bytes");
}

#[test]
fn preview_and_export_share_the_cached_matte() {
    let root = temp_root("preview-export");
    let cache_key = key(vec![3]).cache_key();
    let masks = (0..FRAMES).map(|_| left_half_transparent()).collect::<Vec<_>>();
    write_cache(&root, &cache_key, &masks, FRAMES);

    let mut project = Project::new("R1 preview/export");
    let blue = color_item(&mut project, [0.0, 0.0, 1.0, 1.0], "Blue");
    let red = color_item(&mut project, [1.0, 0.0, 0.0, 1.0], "Red");
    let sequence = project.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 2, 0, None);
    add_clip(&mut project, sequence, blue, 0);
    add_clip(&mut project, sequence, red, 1);

    let mut doc = FrameForgeProject::new(project);
    doc.composition_mut(red).roto = Some(RotoRef::new(cache_key.clone()));

    let document = root.join("r1.ffproj");
    doc.save(&document).expect("save");
    let doc = FrameForgeProject::load(&document).expect("reopen");
    let saved_key = doc.compositions.get(&red).and_then(|c| c.roto.as_ref()).map(|r| r.key.as_str());
    assert_eq!(saved_key, Some(cache_key.as_str()), "the roto reference must survive save and reopen");

    let sources = ProjectSources::build_with_roto_root(&doc, &root).expect("project sources");
    let preview = render_sequence(&doc.project, sequence, R.tick_of(5), RenderOptions::default(), &sources);
    let left = preview.get(8, 18);
    let right = preview.get(56, 18);
    assert!(strong(left, 2, 0), "preview left must be the background blue, got {left:?}");
    assert!(strong(right, 0, 2), "preview right must be the masked clip red, got {right:?}");

    let mp4 = root.join("r1.mp4");
    let settings = ExportSettings {
        format: Format::H264,
        path: mp4.to_string_lossy().into_owned(),
        range: Some(TimeRange::new(Tick::ZERO, DURATION)),
        bitrate_kbps: 6000,
        ..ExportSettings::default()
    };
    let report = export(&Arc::new(doc.project.clone()), sequence, &settings, &sources, &Progress::default()).expect("export");
    assert_eq!(report.frames, FRAMES as u64);

    let bytes = std::fs::read(&mp4).expect("exported bytes");
    let exported = open_bytes("r1.mp4", bytes.into(), &filmcraft_codecs::openers()).expect("decode export");
    let frame = exported.video_frame(FrameRequest::full(R.tick_of(5))).expect("exported frame");
    let px = frame.to_rgba8();
    let at = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [f32::from(px[i]) / 255.0, f32::from(px[i + 1]) / 255.0, f32::from(px[i + 2]) / 255.0, f32::from(px[i + 3]) / 255.0]
    };
    assert!(strong(at(8, 18), 2, 0), "export left must be the background blue, got {:?}", at(8, 18));
    assert!(strong(at(56, 18), 0, 2), "export right must be the masked clip red, got {:?}", at(56, 18));
}
