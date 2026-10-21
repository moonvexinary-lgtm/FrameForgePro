//! M4 acceptance: compositions with real layers — the deep engine integration.
//!
//! A compositing-engine composition (text, shapes, solids, nested comps) now
//! renders *inside* the timeline: its source footage layer is backed by the
//! editing engine's media through the effect chain, served over the
//! compositing engine's own `FootageSource` seam. Also covered: audio-only
//! generators (Tone), relinking file media (the healing path), and importing
//! `.ecproj` compositions onto the timeline.
//!
//! Every case is built through the one [`build_case`] construction path: the
//! moving-text investigation (see `m4_text_visibility_matrix`) showed that two
//! near-identical builders diverged mysteriously, so the duplication was
//! removed rather than the difference trusted.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::Path;

use effectcraft_color::Label as EcLabel;
use effectcraft_keyframe::{Keyframe, TextDoc, Value as EcValue};
use effectcraft_project as ecp;
use effectcraft_time::{FrameRate as EcFrameRate, Tick as EcTick};
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource, open_bytes};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, Project, SequenceSettings, TrackKind};
use filmcraft_render::{Image, RenderOptions, SourceProvider, render_sequence};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};
use frameforge_bridge::{FrameForgeProject, ProjectSources};

const R: FrameRate = FrameRate::FPS_24;
const W: u32 = 320;
const H: u32 = 180;
const FRAMES: i64 = 48; // 2 s at 24 fps

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

fn add_video_clip(p: &mut Project, seq: ItemId, media: ItemId) {
    let video = p.make_track_item(media, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, R.tick_of(FRAMES)), R).expect("video item");
    let s = p.sequence_mut(seq).expect("sequence");
    s.video_tracks[0].items.push(video);
    s.video_tracks[0].sort();
}

/// The one construction path of the moving-text cases: a compositing project
/// whose comp holds a text layer — `keys` animate its position from key to key
/// by frame 40, `None` leaves it static — over an optional footage layer
/// backed by the clip's media, on a timeline whose media is a Bars-and-Tone
/// generator (`with_footage`) or a transparent carrier.
fn build_case(text: &str, size: f64, keys: Option<([f64; 3], [f64; 3])>, with_footage: bool) -> (FrameForgeProject, ItemId) {
    let mut ec = ecp::Project::default();
    ec.settings.bit_depth = ecp::BitDepth::Bpc32;
    let comp = ecp::Comp::new(W, H, EcFrameRate { num: 24, den: 1 }, EcTick(FRAMES * TICKS_PER_SECOND / 24));
    let cid = ec.add_item("Comp", EcLabel::Sandstone, None, ecp::ItemKind::Comp(comp.clone().into()));
    let fid = with_footage.then(|| {
        ec.add_item(
            "Source",
            EcLabel::Red,
            None,
            ecp::ItemKind::Footage(ecp::Footage {
                path: "frameforge://source".to_string(),
                width: W,
                height: H,
                frame_rate: EcFrameRate { num: 24, den: 1 },
                duration: EcTick(FRAMES * TICKS_PER_SECOND / 24),
                has_video: true,
                has_audio: false,
                ..Default::default()
            }),
        )
    });
    // Top layer first (index 0): the text. A text layer's position is the left
    // origin of its glyph run (point-text semantics: the first glyph's
    // baseline start), so the keys are laid out for a ~130-px run to stay
    // inside the frame and the measured band at both ends.
    let mut l = ecp::build::layer(&mut ec, &comp, "Title", ecp::LayerSource::Text, (W, H), None);
    if let Some(pr) = l.props.prop_mut("text/sourceText") {
        pr.value = EcValue::Text(Box::new(TextDoc { text: text.to_string(), size, ..Default::default() }));
    }
    if let Some(pos) = l.props.prop_mut("transform/position") {
        match keys {
            Some((a, b)) => {
                pos.keys = vec![Keyframe::new(EcTick::ZERO, EcValue::Vec3(a)), Keyframe::new(EcTick(40 * TICKS_PER_SECOND / 24), EcValue::Vec3(b))];
            }
            None => pos.value = EcValue::Vec3([75.0, 60.0, 0.0]),
        }
    }
    let source_layer = fid.map(|fid| ecp::build::layer(&mut ec, &comp, "Source", ecp::LayerSource::Footage { item: fid }, (W, H), None));
    let c = ec.comp_mut(cid).expect("comp");
    c.layers.push(l);
    if let Some(sl) = source_layer {
        c.layers.push(sl);
    }

    let generator = if with_footage { Generator::BarsAndTone } else { Generator::TransparentVideo };
    let mut p = Project::new("M4");
    let g = GeneratorSource::new(generator.clone(), W, H, R, Tick(FRAMES * TICKS_PER_SECOND / 24));
    let info = g.info().clone();
    let media = add_media(&mut p, MediaRef::Generator(generator), info);
    let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 1, 0, None);
    add_video_clip(&mut p, seq, media);

    let mut doc = FrameForgeProject::new(p);
    let composition = doc.composition_mut(media);
    composition.ec_project = Some(ec);
    composition.comp = Some(cid);
    composition.source_item = fid;
    (doc, seq)
}

fn frame_at(doc: &FrameForgeProject, seq: ItemId, sources: &ProjectSources, f: i64) -> Image {
    render_sequence(&doc.project, seq, R.tick_of(f), RenderOptions::default(), sources)
}

/// The clean top band of the bars footage (rows 40..90) holds none of the
/// footage's own bright pixels, so every bright pixel there is the white text.
/// Returns the count and the x-centroid of their mass.
fn band_stats(img: &Image) -> (usize, Option<f64>) {
    let (mut n, mut sum) = (0usize, 0.0f64);
    for y in 40..img.h.min(90) {
        for x in 0..img.w {
            let p = img.get(x, y);
            if p[0] > 0.85 && p[1] > 0.85 && p[2] > 0.85 && p[3] > 0.9 {
                n += 1;
                sum += x as f64 + 0.5;
            }
        }
    }
    (n, if n > 0 { Some(sum / n as f64) } else { None })
}

/// The record of how the moving-text case was understood. Two wrong
/// assumptions, both mine, both measured away:
///
/// 1. a text layer's `position` is the **left origin of its glyph run**
///    (point-text semantics), not its centre — the run renders half a run
///    right of where a "centre" reading puts it (static [75, 60] measured a
///    centroid of 141.8 for a 130-px run: 75 + 65);
/// 2. the bars footage carries its own bright white block in the bottom half,
///    and white text on it is invisible to any brightness measurement — the
///    y=150 cases measured an empty band for that reason too.
///
/// The engines and the bridge were rendering correctly the whole time.
#[test]
fn m4_text_visibility_matrix() {
    let show = |text: &str, size: f64, keys: Option<([f64; 3], [f64; 3])>, with_footage: bool, name: &str| {
        let (doc, seq) = build_case(text, size, keys, with_footage);
        let sources = ProjectSources::build(&doc).expect("sources");
        let img = frame_at(&doc, seq, &sources, 6);
        let (n, centroid) = band_stats(&img);
        println!("diagnostic matrix {name}: band {n} px, centroid {centroid:?}");
    };

    show("HI", 60.0, None, true, "a: HI 60 static over footage");
    show("FrameForge", 24.0, None, true, "b: FrameForge 24 static over footage");
    show("FrameForge", 24.0, Some(([75.0, 150.0, 0.0], [215.0, 150.0, 0.0])), true, "c: FrameForge 24 keys at y=150 (over the footage's white block)");
    show("FrameForge", 24.0, Some(([75.0, 150.0, 0.0], [215.0, 150.0, 0.0])), false, "d: FrameForge 24 keys at y=150, no footage");
    show("FrameForge", 24.0, Some(([75.0, 60.0, 0.0], [215.0, 60.0, 0.0])), true, "e: FrameForge 24 keys at y=60 over footage");
    show("HI", 60.0, Some(([75.0, 60.0, 0.0], [215.0, 60.0, 0.0])), true, "f: HI 60 keys at y=60 over footage");
}

#[test]
fn m4_comp_layers_over_footage_in_the_timeline() {
    let (doc, seq) = build_case("FrameForge", 24.0, Some(([20.0, 60.0, 0.0], [190.0, 60.0, 0.0])), true);
    let sources = ProjectSources::build(&doc).expect("sources");

    let early = frame_at(&doc, seq, &sources, 6); // 0.25 s: the run starts near its left key
    let late = frame_at(&doc, seq, &sources, 42); // past the last key: the run holds at its right key

    // The animation really runs: the text's mass centre travels from the left
    // key (~97) to the right key (225); the static default position would sit
    // at the comp centre (160) at both times.
    let (n_early, c_early) = band_stats(&early);
    let (n_late, c_late) = band_stats(&late);
    let (ce, cl) = (c_early.expect("text pixels in the early band"), c_late.expect("text pixels in the late band"));
    assert!(n_early > 20, "the text must render over the footage: {n_early} band pixels");
    assert!(n_late > 20, "the text must render over the footage late too: {n_late} band pixels");
    assert!(ce < 130.0, "the text must start near the left key: centroid {ce}");
    assert!(cl > 180.0, "the text must end near the right key: centroid {cl}");
    assert!(cl - ce > 60.0, "the keyframed position must animate: centroid {ce} -> {cl}");

    // Time is direction-independent: rendering the late frame first changes nothing.
    let late_first = frame_at(&doc, seq, &sources, 42);
    let early_again = frame_at(&doc, seq, &sources, 6);
    assert_eq!(late_first, late, "backward scrubbing must reproduce the frame");
    assert_eq!(early_again, early, "backward scrubbing must reproduce the frame");

    // The footage shows through where the text is not: bars keep a hard edge
    // far from the text.
    let mut max_edge = 0.0f32;
    for y in 0..early.h {
        for x in 0..early.w.saturating_sub(1) {
            let a = early.get(x, y);
            let b = early.get(x + 1, y);
            let (l1, l2) = (0.2126 * a[0] + 0.7152 * a[1] + 0.0722 * a[2], 0.2126 * b[0] + 0.7152 * b[1] + 0.0722 * b[2]);
            max_edge = max_edge.max((l2 - l1).abs());
        }
    }
    assert!(max_edge > 0.3, "the bars under the text must stay sharp, got {max_edge}");

    // Save, drop everything, reopen: the composition survives the round-trip
    // (the compositing project serializes inside the .ffproj) and renders
    // byte-identically.
    let dir = std::env::temp_dir();
    let path = dir.join(format!("frameforge-m4-{}.ffproj", std::process::id()));
    doc.save(&path).expect("save");
    let reopened = FrameForgeProject::load(&path).expect("reopen");
    assert!(reopened.compositions.len() == 1 && reopened.compositions.values().next().unwrap().has_comp(), "the comp must survive the round-trip");
    let reopened_sources = ProjectSources::build(&reopened).expect("reopened sources");
    assert_eq!(frame_at(&reopened, seq, &reopened_sources, 6), early, "frame 6 must be unchanged after save + reopen");
    assert_eq!(frame_at(&reopened, seq, &reopened_sources, 42), late, "frame 42 must be unchanged after save + reopen");
}

/// The minimal text case — the compositing engine's own renderer test comp
/// ("HI", size 60, static), rendered through the bridge's comp path over a
/// transparent carrier.
#[test]
fn m4_text_minimal_repro() {
    let mut ec = ecp::Project::default();
    ec.settings.bit_depth = ecp::BitDepth::Bpc32;
    let comp = ecp::Comp::new(200, 100, EcFrameRate { num: 24, den: 1 }, EcTick(TICKS_PER_SECOND));
    let cid = ec.add_item("Comp", EcLabel::Sandstone, None, ecp::ItemKind::Comp(comp.clone().into()));
    let mut l = ecp::build::layer(&mut ec, &comp, "Text", ecp::LayerSource::Text, (200, 100), None);
    if let Some(pr) = l.props.prop_mut("text/sourceText") {
        pr.value = EcValue::Text(Box::new(TextDoc { text: "HI".to_string(), size: 60.0, ..Default::default() }));
    }
    if let Some(pos) = l.props.prop_mut("transform/position") {
        pos.value = EcValue::Vec3([60.0, 80.0, 0.0]);
    }
    ec.comp_mut(cid).expect("comp").layers.push(l);

    let mut p = Project::new("text repro");
    let g = GeneratorSource::new(Generator::TransparentVideo, 200, 100, R, Tick(TICKS_PER_SECOND));
    let info = g.info().clone();
    let media = add_media(&mut p, MediaRef::Generator(Generator::TransparentVideo), info);
    let seq = p.new_sequence("Main", SequenceSettings { width: 200, height: 100, frame_rate: R, ..Default::default() }, 1, 0, None);
    let video = p.make_track_item(media, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, R.tick_of(24)), R).expect("clip");
    let s = p.sequence_mut(seq).expect("sequence");
    s.video_tracks[0].items.push(video);
    s.video_tracks[0].sort();

    let mut doc = FrameForgeProject::new(p);
    let composition = doc.composition_mut(media);
    composition.ec_project = Some(ec);
    composition.comp = Some(cid);
    let sources = ProjectSources::build(&doc).expect("sources");
    let img = render_sequence(&doc.project, seq, R.tick_of(6), RenderOptions::default(), &sources);

    let covered = img.px.chunks(4).filter(|px| px[3] > 0.5).count();
    println!("diagnostic text repro: covered {covered}, size {}x{}", img.w, img.h);
    assert!(covered > 300, "the text comp must render through the bridge: {covered} covered pixels");
}

#[test]
fn m4_tone_generator_plays() {
    let mut p = Project::new("tone");
    let g = GeneratorSource::new(Generator::Tone { hz: 440.0, db: -6.0 }, W, H, R, Tick(TICKS_PER_SECOND));
    let info = g.info().clone();
    assert!(info.video.is_none() && info.has_audio(), "a tone generator is audio-only");
    let media = add_media(&mut p, MediaRef::Generator(Generator::Tone { hz: 440.0, db: -6.0 }), info);
    let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 0, 1, None);
    let audio = p.make_track_item(media, TrackKind::Audio, Tick::ZERO, TimeRange::new(Tick::ZERO, R.tick_of(24)), R).expect("audio item");
    let s = p.sequence_mut(seq).expect("sequence");
    s.audio_tracks[0].items.push(audio);
    s.audio_tracks[0].sort();

    let doc = FrameForgeProject::new(p);
    let sources = ProjectSources::build(&doc).expect("sources");
    let src = sources.source(media).expect("the tone item needs a source");
    let buffer = src.audio(0, 48_000, 48_000).expect("decode the tone");
    assert_eq!(buffer.sample_rate, 48_000);
    let peak = buffer.channels.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.2, "the tone must be audible: {peak}");
}

/// A minimal hand-written PCM WAV file (no downloads, no encoders).
fn write_wav(path: &Path) {
    let rate = 48_000u32;
    let samples = 24_000usize; // 0.5 s
    let mut data = Vec::with_capacity(samples * 2);
    for i in 0..samples {
        let v = if (i / 240) % 2 == 0 { 12_000i16 } else { -12_000 };
        data.extend_from_slice(&v.to_le_bytes());
    }
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVE");
    file.extend_from_slice(b"fmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes()); // PCM
    file.extend_from_slice(&1u16.to_le_bytes()); // mono
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    file.extend_from_slice(&2u16.to_le_bytes()); // block align
    file.extend_from_slice(&16u16.to_le_bytes()); // bits
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&data);
    std::fs::write(path, file).expect("write wav");
}

#[test]
fn m4_relink_heals_missing_media() {
    let dir = std::env::temp_dir();
    let wav = dir.join(format!("frameforge-m4-{}.wav", std::process::id()));
    write_wav(&wav);

    // Probe the real file for its MediaInfo (what import does).
    let bytes = std::fs::read(&wav).expect("wav");
    let probed = open_bytes("tone.wav", bytes.into(), &filmcraft_codecs::openers()).expect("probe wav");
    let info = probed.info().clone();

    let mut p = Project::new("relink");
    let bogus = dir.join("frameforge-m4-missing.wav");
    let item = add_media(&mut p, MediaRef::File { path: bogus.to_string_lossy().into_owned() }, info);
    let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 0, 1, None);
    let audio = p.make_track_item(item, TrackKind::Audio, Tick::ZERO, TimeRange::new(Tick::ZERO, Tick(TICKS_PER_SECOND / 2)), R).expect("audio item");
    let s = p.sequence_mut(seq).expect("sequence");
    s.audio_tracks[0].items.push(audio);
    s.audio_tracks[0].sort();

    // Missing: the offline slate — silence, not a failure.
    let mut doc = FrameForgeProject::new(p);
    let sources = ProjectSources::build(&doc).expect("missing media must not fail the build");
    let offline = sources.source(item).expect("offline source").audio(0, 4800, 48_000).expect("offline audio is silence");
    let peak = offline.channels.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak < 0.01, "the offline slate is silent, got {peak}");

    // Relink to the real file: the media heals — same document, now audible.
    let wav_path = wav.to_string_lossy().into_owned();
    doc.relink(item, &wav_path).expect("relink");
    let healed_sources = ProjectSources::build(&doc).expect("healed build");
    let healed = healed_sources.source(item).expect("healed source").audio(0, 4800, 48_000).expect("decode");
    let peak = healed.channels.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.2, "the relinked media must play: {peak}");
}

#[test]
fn m4_import_ecproj_onto_the_timeline() {
    // A compositing project with one solid red layer, saved as a real .ecproj.
    let mut ec = ecp::Project::default();
    ec.settings.bit_depth = ecp::BitDepth::Bpc32;
    let comp = ecp::Comp::new(W, H, EcFrameRate { num: 24, den: 1 }, EcTick(TICKS_PER_SECOND));
    let comp_id = ec.add_item("Comp", EcLabel::Sandstone, None, ecp::ItemKind::Comp(comp.clone().into()));
    let sid = ec.add_item("Solid", EcLabel::Red, None, ecp::ItemKind::Solid(ecp::Solid { color: [1.0, 0.0, 0.0], width: W, height: H, pixel_aspect: 1.0 }));
    let solid = ecp::build::layer(&mut ec, &comp, "Solid", ecp::LayerSource::Solid { item: sid }, (W, H), None);
    ec.comp_mut(comp_id).expect("comp").layers.push(solid);
    let text = ec.to_file_json().expect("ecproj text");

    let dir = std::env::temp_dir();
    let path = dir.join(format!("frameforge-m4-{}.ecproj", std::process::id()));
    std::fs::write(&path, text).expect("write ecproj");

    // Import: the comp lands on the timeline as an alpha-carrying media item.
    let mut p = Project::new("import");
    let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 1, 0, None);
    let mut doc = FrameForgeProject::new(p);
    let item = doc.import_ecproj(&path).expect("import ecproj");
    let video = doc.project.make_track_item(item, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, R.tick_of(24)), R).expect("clip");
    let s = doc.project.sequence_mut(seq).expect("sequence");
    s.video_tracks[0].items.push(video);
    s.video_tracks[0].sort();

    let sources = ProjectSources::build(&doc).expect("sources");
    let img = render_sequence(&doc.project, seq, R.tick_of(12), RenderOptions::default(), &sources);
    let c = img.get(W as usize / 2, H as usize / 2);
    assert!(c[0] > 0.9 && c[1] < 0.1 && c[2] < 0.1 && c[3] > 0.9, "the imported comp must paint its solid red, got {c:?}");
}
