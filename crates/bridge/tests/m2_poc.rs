//! M2 acceptance: one real end-to-end integration proof, run by CI.
//!
//! What this proves, with real pixels and real code paths (nothing is stubbed):
//!
//! 1. A timeline is built in the FilmCraft editing engine — a synthetic
//!    Bars-and-Tone generator item (no downloads, no files), one video clip,
//!    one audio clip, one sequence.
//! 2. A real EffectCraft effect (Gaussian Blur, `ec.blur.gaussian`) is attached
//!    to the media item through a composition.
//! 3. The effect's `blurriness` parameter is animated with the editing
//!    engine's keyframe system (exact ticks, media time).
//! 4. Frames render through the editing engine's compositor, reading the
//!    composited source; evaluation is a pure function of time, so rendering
//!    backward gives the identical frame as rendering forward.
//! 5. The project saves to `.ffproj` and reopens; the frames are unchanged.
//! 6. The same project exports through the editing engine's real export
//!    pipeline (H.264 + AAC in an MP4), audio included.
//! 7. The exported file is opened back and verified: resolution, frame rate,
//!    duration, an audio track that is not silent, and a frame that really
//!    carries the animated blur.

// Tests fail loudly on purpose (the upstream convention): expects here name
// the invariant that broke. The library keeps the strict never-crash lints.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::sync::Arc;

use effectcraft_effects as fx;
use filmcraft_export::{ExportSettings, Format, Progress, export};
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{FrameRequest, Generator, MediaSource, open_bytes};
use filmcraft_project::keyframe::{Param, ParamValue};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, Project, SequenceSettings, TrackKind};
use filmcraft_render::{Image, RenderOptions, SourceProvider, render_sequence};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
use frameforge_bridge::{EffectLink, FrameForgeProject, ProjectSources};

const R: FrameRate = FrameRate::FPS_24;
const W: u32 = 640;
const H: u32 = 360;
const FRAMES: i64 = 72; // 3 s at 24 fps
const DURATION: Tick = Tick(3 * TICKS_PER_SECOND);
/// Blurriness at the end of the clip (the keyframe target).
const END_BLUR: f64 = 48.0;

/// The document under test: a project whose single media item is a synthetic
/// Bars-and-Tone generator, edited into one video and one audio clip, with a
/// Gaussian Blur composition whose `blurriness` ramps 0 → `END_BLUR` over the
/// clip (media time, exact ticks).
fn build() -> (FrameForgeProject, ItemId, ItemId) {
    let mut project = Project::new("M2 integration proof");
    let generator = GeneratorSource::new(Generator::BarsAndTone, W, H, R, DURATION);
    let info = generator.info().clone();
    let name = info.name.clone();
    let media = project.add_item(
        name.as_str(),
        Label::Iris,
        ItemKind::Media(MediaClip {
            media: MediaRef::Generator(Generator::BarsAndTone),
            info,
            interpret: Default::default(),
            mark_in: None,
            mark_out: None,
            markers: Vec::new(),
            offline: false,
            proxy: None,
            identity: None,
        }),
        None,
    );
    let seq = project.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 1, 1, None);
    let source = TimeRange::new(Tick::ZERO, R.tick_of(FRAMES));
    let video_clip = project.make_track_item(media, TrackKind::Video, Tick::ZERO, source, R).expect("video track item");
    let audio_clip = project.make_track_item(media, TrackKind::Audio, Tick::ZERO, source, R).expect("audio track item");
    {
        let s = project.sequence_mut(seq).expect("sequence");
        s.video_tracks[0].items.push(video_clip);
        s.video_tracks[0].sort();
        s.audio_tracks[0].items.push(audio_clip);
        s.audio_tracks[0].sort();
    }

    let mut doc = FrameForgeProject::new(project);
    let mut blurriness = Param::new(ParamValue::Float(0.0));
    blurriness.put_keyframe(Tick::ZERO, ParamValue::Float(0.0));
    blurriness.put_keyframe(DURATION, ParamValue::Float(END_BLUR));
    doc.composition_mut(media).effects.push(EffectLink {
        effect: "ec.blur.gaussian".to_string(),
        enabled: true,
        params: HashMap::from([("blurriness".to_string(), blurriness)]),
    });
    (doc, seq, media)
}

fn frame(doc: &FrameForgeProject, seq: ItemId, sources: &ProjectSources, f: i64) -> Image {
    render_sequence(&doc.project, seq, R.tick_of(f), RenderOptions::default(), sources)
}

/// Horizontal detail energy of the compositor's linear-light image: what a
/// blur removes. Bars have hard vertical edges, so this is a sharp detector.
fn sharpness(img: &Image) -> f64 {
    let luma = |p: [f32; 4]| f64::from(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]);
    let mut s = 0.0;
    for y in (0..img.h).step_by(16) {
        for x in (0..img.w.saturating_sub(1)).step_by(16) {
            s += (luma(img.get(x + 1, y)) - luma(img.get(x, y))).abs();
        }
    }
    s
}

/// Same metric on straight RGBA8 bytes (for decoded frames).
fn sharpness_rgba(rgba: &[u8], w: u32, h: u32) -> f64 {
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        0.2126 * f64::from(rgba[i]) + 0.7152 * f64::from(rgba[i + 1]) + 0.0722 * f64::from(rgba[i + 2])
    };
    let mut s = 0.0;
    for y in (0..h).step_by(16) {
        for x in (0..w.saturating_sub(1)).step_by(16) {
            s += (px(x + 1, y) - px(x, y)).abs();
        }
    }
    s
}

#[test]
fn m2_integration_proof() {
    // --- 0. the compositing engine's catalog is real and connected -----------
    assert!(fx::registry().len() >= 300, "expected a large effect registry, got {}", fx::registry().len());
    assert!(fx::find("ec.blur.gaussian").is_some(), "Gaussian Blur must be in the registry");

    let (doc, seq, media) = build();
    let sources = ProjectSources::build(&doc).expect("sources");
    assert!(sources.source(media).is_some(), "the media item needs a source");

    // --- 1. the animation really runs: blur grows with the keyframes ---------
    let f12 = frame(&doc, seq, &sources, 12);
    let f36 = frame(&doc, seq, &sources, 36);
    let f60 = frame(&doc, seq, &sources, 60);
    let (s12, s36, s60) = (sharpness(&f12), sharpness(&f36), sharpness(&f60));
    assert!(s12 > s36 && s36 > s60, "the keyframed blur must soften the picture over time: {s12} -> {s36} -> {s60}");

    // --- 2. time is direction-independent: rendering backward is identical ---
    let b60 = frame(&doc, seq, &sources, 60);
    let b36 = frame(&doc, seq, &sources, 36);
    let b12 = frame(&doc, seq, &sources, 12);
    assert_eq!(b60, f60, "scrubbing backward must reproduce the same frame");
    assert_eq!(b36, f36, "scrubbing backward must reproduce the same frame");
    assert_eq!(b12, f12, "scrubbing backward must reproduce the same frame");

    // --- 3. at blurriness 0 the composition is the identity -----------------
    let mut raw_doc = doc.clone();
    raw_doc.compositions.clear();
    let raw_sources = ProjectSources::build(&raw_doc).expect("raw sources");
    let raw12 = frame(&raw_doc, seq, &raw_sources, 12);
    let mut max_diff = 0.0f32;
    for y in (0..raw12.h).step_by(4) {
        for x in (0..raw12.w).step_by(4) {
            let a = raw12.get(x, y);
            let b = f12.get(x, y);
            for k in 0..4 {
                max_diff = max_diff.max((a[k] - b[k]).abs());
            }
        }
    }
    assert!(max_diff > 0.05, "blurriness 8 at frame 12 must differ from the raw picture");
    let raw0 = frame(&raw_doc, seq, &raw_sources, 0);
    let fx0 = frame(&doc, seq, &sources, 0);
    let mut max_diff0 = 0.0f32;
    for y in (0..raw0.h).step_by(4) {
        for x in (0..raw0.w).step_by(4) {
            let a = raw0.get(x, y);
            let b = fx0.get(x, y);
            for k in 0..4 {
                max_diff0 = max_diff0.max((a[k] - b[k]).abs());
            }
        }
    }
    assert!(max_diff0 < 0.01, "blurriness 0 must be the identity (RGBA round-trip only), max diff {max_diff0}");

    // --- 4. save, drop everything, reopen: the edit survives ------------------
    let dir = std::env::temp_dir();
    let path = dir.join(format!("frameforge-m2-{}.ffproj", std::process::id()));
    doc.save(&path).expect("save");
    let reopened = FrameForgeProject::load(&path).expect("reopen");
    assert_eq!(reopened.compositions.len(), 1, "the composition must survive the round-trip");
    let reopened_sources = ProjectSources::build(&reopened).expect("reopened sources");
    for (f, expected) in [(12, &f12), (36, &f36), (60, &f60)] {
        let got = frame(&reopened, seq, &reopened_sources, f);
        assert_eq!(&got, expected, "frame {f} must be unchanged after save + reopen");
    }

    // --- 5. the real export pipeline: H.264 + AAC in an MP4 -------------------
    let mp4 = dir.join(format!("frameforge-m2-{}.mp4", std::process::id()));
    let settings = ExportSettings {
        format: Format::H264,
        path: mp4.to_string_lossy().into_owned(),
        range: Some(TimeRange::new(Tick::ZERO, DURATION)),
        bitrate_kbps: 6000,
        ..ExportSettings::default()
    };
    let report = export(&Arc::new(doc.project.clone()), seq, &settings, &sources, &Progress::default()).expect("export");
    assert_eq!(report.frames, FRAMES as u64, "3 s at 24 fps must export 72 frames");
    assert!(report.bytes > 10_000, "the file must not be empty");

    // --- 6. verify the exported file ------------------------------------------
    let bytes = std::fs::read(&mp4).expect("read exported file");
    let src = open_bytes("m2-proof.mp4", bytes.into(), &filmcraft_codecs::openers()).expect("open exported file");
    let info = src.info();
    let video = info.video.as_ref().expect("the export has video");
    assert_eq!((video.width, video.height), (W, H), "resolution must match the sequence");
    assert_eq!(video.frame_rate, R, "frame rate must be exact");
    assert!(info.has_audio(), "the tone must be in the file");
    let drift = (info.duration.0 as f64 - DURATION.0 as f64).abs();
    assert!(drift < 2.0 * R.frame_duration().0 as f64, "duration must be 3 s within two frames");

    // The frame at 1.5 s carries the animated blur (blurriness 24 there),
    // not the sharp bars: compare against the raw generator frame.
    let out_frame = src.video_frame(FrameRequest::full(Tick(3 * TICKS_PER_SECOND / 2))).expect("decode a frame");
    let raw_frame =
        GeneratorSource::new(Generator::BarsAndTone, W, H, R, DURATION).video_frame(FrameRequest::full(Tick(3 * TICKS_PER_SECOND / 2))).expect("raw frame");
    let (out_s, raw_s) = (sharpness_rgba(&out_frame.to_rgba8(), W, H), sharpness_rgba(&raw_frame.to_rgba8(), W, H));
    assert!(out_s < raw_s * 0.5, "the exported frame must carry the blur: {out_s} vs raw {raw_s}");

    // The audio decodes and is the tone, not silence.
    let audio = src.audio(0, 48_000, 48_000).expect("decode audio");
    assert_eq!(audio.sample_rate, 48_000);
    let peak = audio.channels.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.05, "the tone must be audible in the export: peak {peak}");

    // The temporary files stay in the system temp dir (the CI runner is
    // ephemeral; locally they can be deleted at any time).
}
