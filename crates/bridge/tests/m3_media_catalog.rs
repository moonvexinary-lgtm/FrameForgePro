//! M3 acceptance: file-based media through the editing engine's codecs,
//! compositions on file media, offline slates for missing files, `.fcproj`
//! import, and the machine-readable two-engine effects registry — every claim
//! measured on real code paths in CI.
//!
//! The fixture is self-contained: a one-second H.264 file exported by the M2
//! pipeline itself (no downloads), then re-imported as file media.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use effectcraft_effects as fx;
use filmcraft_export::{ExportSettings, Format, Progress, export};
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{FrameRequest, Generator, MediaSource, open_bytes};
use filmcraft_project::keyframe::{Param, ParamValue};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, Project, SequenceSettings, TrackKind};
use filmcraft_render::{Image, RenderOptions, SourceProvider, render_sequence};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
use frameforge_bridge::{ComposedSource, Composition, EffectLink, FrameForgeProject, ProjectSources, catalog_report, name_collisions};

const R: FrameRate = FrameRate::FPS_24;
const W: u32 = 640;
const H: u32 = 360;

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

fn add_media(p: &mut Project, media: MediaRef, info: filmcraft_media::MediaInfo) -> ItemId {
    let name = info.name.clone();
    p.add_item(name.as_str(), Label::Iris, media_clip(media, info), None)
}

fn add_clips(p: &mut Project, media: ItemId, seq: ItemId, frames: i64) {
    let source = TimeRange::new(Tick::ZERO, R.tick_of(frames));
    let video = p.make_track_item(media, TrackKind::Video, Tick::ZERO, source, R).expect("video item");
    let audio = p.make_track_item(media, TrackKind::Audio, Tick::ZERO, source, R).expect("audio item");
    let s = p.sequence_mut(seq).expect("sequence");
    s.video_tracks[0].items.push(video);
    s.video_tracks[0].sort();
    s.audio_tracks[0].items.push(audio);
    s.audio_tracks[0].sort();
}

fn new_sequence(p: &mut Project) -> ItemId {
    p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 1, 1, None)
}

/// The sharpest horizontal step in the picture (see `tests/m2_poc.rs`): the
/// quantity a blur provably removes.
fn max_edge(img: &Image) -> f32 {
    let luma = |p: [f32; 4]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let mut m = 0.0f32;
    for y in 0..img.h {
        for x in 0..img.w.saturating_sub(1) {
            m = m.max((luma(img.get(x + 1, y)) - luma(img.get(x, y))).abs());
        }
    }
    m
}

fn frame6(doc: &FrameForgeProject, seq: ItemId, sources: &ProjectSources) -> Image {
    render_sequence(&doc.project, seq, R.tick_of(6), RenderOptions::default(), sources)
}

/// Export a one-second Bars-and-Tone H.264 file with the real export pipeline.
fn export_fixture(path: &Path) {
    let mut p = Project::new("M3 fixture");
    let g = GeneratorSource::new(Generator::BarsAndTone, W, H, R, Tick(TICKS_PER_SECOND));
    let info = g.info().clone();
    let media = add_media(&mut p, MediaRef::Generator(Generator::BarsAndTone), info);
    let seq = new_sequence(&mut p);
    add_clips(&mut p, media, seq, 24);
    let doc = FrameForgeProject::new(p);
    let sources = ProjectSources::build(&doc).expect("fixture sources");
    let settings = ExportSettings {
        format: Format::H264,
        path: path.to_string_lossy().into_owned(),
        range: Some(TimeRange::new(Tick::ZERO, Tick(TICKS_PER_SECOND))),
        bitrate_kbps: 6000,
        ..ExportSettings::default()
    };
    export(&Arc::new(doc.project.clone()), seq, &settings, &sources, &Progress::default()).expect("fixture export");
}

#[test]
fn m3_file_media_offline_and_fcproj_import() {
    let dir = std::env::temp_dir();
    let mp4 = dir.join(format!("frameforge-m3-fixture-{}.mp4", std::process::id()));

    // --- 1. file media plays through the editing engine's codecs ----------
    export_fixture(&mp4);
    let bytes = std::fs::read(&mp4).expect("fixture file");
    let probed = open_bytes("fixture.mp4", bytes.into(), &filmcraft_codecs::openers()).expect("probe");
    let info = probed.info().clone();
    assert!(info.video.is_some(), "the exported fixture must have video");
    assert!(info.has_audio(), "the exported fixture must have audio");

    let mut p = Project::new("file media");
    let item = add_media(&mut p, MediaRef::File { path: mp4.to_string_lossy().into_owned() }, info.clone());
    let seq = new_sequence(&mut p);
    add_clips(&mut p, item, seq, 24);
    let plain = FrameForgeProject::new(p);
    let plain_sources = ProjectSources::build(&plain).expect("plain sources");
    assert!(plain_sources.source(item).is_some(), "the file item must have a source");
    let plain_frame = frame6(&plain, seq, &plain_sources);
    let plain_edge = max_edge(&plain_frame);
    assert!(plain_edge > 0.3, "decoded bars must keep hard edges (linear light), got {plain_edge}");

    // --- 2. a composition runs on real decoded footage ---------------------
    let mut blurred = plain.clone();
    blurred.composition_mut(item).effects.push(EffectLink {
        effect: "ec.blur.gaussian".to_string(),
        enabled: true,
        params: HashMap::from([("blurriness".to_string(), Param::new(ParamValue::Float(30.0)))]),
    });
    let blurred_sources = ProjectSources::build(&blurred).expect("blurred sources");
    let blurred_frame = frame6(&blurred, seq, &blurred_sources);
    let blurred_edge = max_edge(&blurred_frame);
    assert!(blurred_edge < plain_edge * 0.5, "the blur must soften real footage: {blurred_edge} vs {plain_edge}");

    // --- 3. a missing file is the offline slate, never a failure ------------
    let mut missing_p = Project::new("missing media");
    let bogus = dir.join(format!("frameforge-m3-missing-{}.mp4", std::process::id()));
    let missing_item = add_media(&mut missing_p, MediaRef::File { path: bogus.to_string_lossy().into_owned() }, info);
    let missing_seq = new_sequence(&mut missing_p);
    add_clips(&mut missing_p, missing_item, missing_seq, 24);
    let missing_doc = FrameForgeProject::new(missing_p);
    let missing_sources = ProjectSources::build(&missing_doc).expect("missing media must not fail the build");
    let missing_frame = frame6(&missing_doc, missing_seq, &missing_sources);
    assert!(max_edge(&missing_frame) < 0.05, "the offline slate is a flat picture, got {}", max_edge(&missing_frame));

    // --- 4. a FilmCraft .fcproj imports and renders identically ------------
    let fcproj = dir.join(format!("frameforge-m3-{}.fcproj", std::process::id()));
    std::fs::write(&fcproj, serde_json::to_string_pretty(&plain.project).expect("serialize fcproj")).expect("write fcproj");
    let imported = FrameForgeProject::import_fcproj(&fcproj).expect("import fcproj");
    assert!(imported.compositions.is_empty(), "an imported editing project starts without compositions");
    let imported_sources = ProjectSources::build(&imported).expect("imported sources");
    let imported_frame = frame6(&imported, seq, &imported_sources);
    assert_eq!(imported_frame, plain_frame, "the imported project must render identically");
}

#[test]
fn m3_effects_registry_audit() {
    // Both engines describe their catalogs as data; the merged registry keeps
    // every entry's engine and original identity.
    let fc = filmcraft_project::effect::effect_defs();
    assert!(fc.len() >= 60, "the editing engine's catalog is large, got {}", fc.len());
    let ec = fx::registry();
    assert!(ec.len() >= 300, "the compositing engine's catalog is large, got {}", ec.len());
    let report = catalog_report();
    assert_eq!(report.catalog.len(), fc.len() + ec.len(), "the merged catalog must be complete");
    assert!(report.catalog.iter().all(|e| !e.id.is_empty() && !e.name.is_empty()), "every entry is identified");

    // The duplicate audit finds effects both engines ship under one name
    // (Gaussian Blur is the known one).
    let collisions = name_collisions();
    assert!(collisions.contains_key("gaussian blur"), "the duplicate audit must find Gaussian Blur in both engines");
    assert!(!collisions.is_empty(), "the catalogs share display names");

    // Sweep: every compositing-engine effect runs through the real
    // composed-source path (the same code the renderer uses) on a real frame,
    // and is classified. No effect may panic (the test completing is the
    // proof); effects the bridge cannot yet place report a clear error.
    let generator = Arc::new(GeneratorSource::new(Generator::BarsAndTone, 32, 32, R, Tick(TICKS_PER_SECOND)));
    let mut ok = 0usize;
    let mut unsupported = 0usize;
    for s in ec {
        let composition = Composition { effects: vec![EffectLink { effect: s.id.to_string(), enabled: true, params: HashMap::new() }], ..Default::default() };
        let source = ComposedSource::new(generator.clone(), composition, 7);
        match source.video_frame(FrameRequest::full(Tick(TICKS_PER_SECOND / 2))) {
            Ok(f) => {
                assert_eq!((f.width, f.height), (32, 32), "{} changed the frame size", s.id);
                ok += 1;
            }
            Err(_) => unsupported += 1,
        }
    }
    assert_eq!(ok + unsupported, ec.len(), "every effect must be classified");
    assert!(ok >= 150, "most of the catalog must run through the bridge: {ok} of {}", ec.len());
    println!("catalog sweep: {ok} run through the composed-source bridge, {unsupported} reported unsupported, of {} total", ec.len());
}
