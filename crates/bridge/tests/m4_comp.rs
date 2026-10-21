//! M4 acceptance: compositions with real layers — the deep engine integration.
//!
//! A compositing-engine composition (text, shapes, solids, nested comps) now
//! renders *inside* the timeline: its source footage layer is backed by the
//! editing engine's media through the effect chain, served through the same
//! `FootageSource` seam the compositing engine's own media uses. Also covered:
//! audio-only generators (Tone), relinking file media (the healing path), and
//! importing `.ecproj` compositions onto the timeline.

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

fn bars_project() -> (FrameForgeProject, ItemId, ItemId) {
    let mut p = Project::new("M4 comp proof");
    let g = GeneratorSource::new(Generator::BarsAndTone, W, H, R, Tick(FRAMES * TICKS_PER_SECOND / 24));
    let info = g.info().clone();
    let media = add_media(&mut p, MediaRef::Generator(Generator::BarsAndTone), info);
    let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 1, 0, None);
    let source = TimeRange::new(Tick::ZERO, R.tick_of(FRAMES));
    let video = p.make_track_item(media, TrackKind::Video, Tick::ZERO, source, R).expect("video item");
    let s = p.sequence_mut(seq).expect("sequence");
    s.video_tracks[0].items.push(video);
    s.video_tracks[0].sort();
    (FrameForgeProject::new(p), seq, media)
}

/// A compositing-engine project: one comp holding a text layer (animated
/// position) over a footage layer backed by the clip's media.
fn comp_project() -> (ecp::Project, ecp::ItemId, ecp::ItemId) {
    let mut p = ecp::Project::default();
    p.settings.bit_depth = ecp::BitDepth::Bpc32;
    let comp = ecp::Comp::new(W, H, EcFrameRate { num: 24, den: 1 }, EcTick(FRAMES * TICKS_PER_SECOND / 24));
    let comp_id = p.add_item("Comp", EcLabel::Sandstone, None, ecp::ItemKind::Comp(comp.clone().into()));
    let footage = ecp::Footage {
        path: "frameforge://source".to_string(),
        width: W,
        height: H,
        frame_rate: EcFrameRate { num: 24, den: 1 },
        duration: EcTick(FRAMES * TICKS_PER_SECOND / 24),
        has_video: true,
        has_audio: false,
        ..Default::default()
    };
    let fid = p.add_item("Source", EcLabel::Red, None, ecp::ItemKind::Footage(footage));
    // Top layer first (index 0): white text moving left → right across the clip.
    // Sized so the whole glyph run stays inside one half at each end.
    let mut text = ecp::build::layer(&mut p, &comp, "Title", ecp::LayerSource::Text, (W, H), None);
    if let Some(pr) = text.props.prop_mut("text/sourceText") {
        pr.value = EcValue::Text(Box::new(TextDoc {
            text: "FrameForge".to_string(),
            font: "Inter".to_string(),
            size: 18.0,
            fill: [1.0, 1.0, 1.0, 1.0],
            ..Default::default()
        }));
    }
    if let Some(pos) = text.props.prop_mut("transform/position") {
        // The move completes by frame 40, so the sampled late frame (42) sits
        // past the last key: the whole run is in the right half.
        pos.keys = vec![
            Keyframe::new(EcTick::ZERO, EcValue::Vec3([75.0, 150.0, 0.0])),
            Keyframe::new(EcTick(40 * TICKS_PER_SECOND / 24), EcValue::Vec3([215.0, 150.0, 0.0])),
        ];
    }
    let source_layer = ecp::build::layer(&mut p, &comp, "Source", ecp::LayerSource::Footage { item: fid }, (W, H), None);
    let c = p.comp_mut(comp_id).expect("comp");
    c.layers.push(text);
    c.layers.push(source_layer);
    (p, comp_id, fid)
}

fn frame_at(doc: &FrameForgeProject, seq: ItemId, sources: &ProjectSources, f: i64) -> Image {
    render_sequence(&doc.project, seq, R.tick_of(f), RenderOptions::default(), sources)
}

/// Bright (text) pixels per horizontal half of a horizontal band below the
/// color bars (the bars carry a big white patch of their own, so the moving
/// text is measured out of its way).
fn bright_halves(img: &Image) -> (usize, usize) {
    let (mut left, mut right) = (0, 0);
    for y in 138..img.h.min(168) {
        for x in 0..img.w {
            let p = img.get(x, y);
            if p[0] > 0.85 && p[1] > 0.85 && p[2] > 0.85 && p[3] > 0.9 {
                if x < img.w / 2 { left += 1 } else { right += 1 }
            }
        }
    }
    (left, right)
}

/// The minimal text case — their renderer test's exact comp ("HI", size 60,
/// static position), rendered through the bridge's comp path over a
/// transparent carrier. Isolates text-through-the-bridge from the bigger
/// timeline test.
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

/// One controlled experiment over the suspected factors: does the text render
/// (a) "HI" at size 60, static position, over the footage layer;
/// (b) "FrameForge" at size 18, static, over the footage;
/// (c) "FrameForge" at size 18, keyframed, over the footage;
/// (d) "FrameForge" at size 18, keyframed, with NO footage layer.
/// Prints each frame's bright grid so one CI run shows which factor hides
/// the text.
#[test]
fn m4_text_visibility_matrix() {
    let make = |text: &str, size: f64, keys: Option<([f64; 3], [f64; 3])>, with_footage: bool, name: &str| {
        let mut ec = ecp::Project::default();
        ec.settings.bit_depth = ecp::BitDepth::Bpc32;
        let comp = ecp::Comp::new(W, H, EcFrameRate { num: 24, den: 1 }, EcTick(FRAMES * TICKS_PER_SECOND / 24));
        let cid = ec.add_item("Comp", EcLabel::Sandstone, None, ecp::ItemKind::Comp(comp.clone().into()));
        let mut fid = None;
        if with_footage {
            let footage = ecp::Footage {
                path: "frameforge://source".to_string(),
                width: W,
                height: H,
                frame_rate: EcFrameRate { num: 24, den: 1 },
                duration: EcTick(FRAMES * TICKS_PER_SECOND / 24),
                has_video: true,
                has_audio: false,
                ..Default::default()
            };
            fid = Some(ec.add_item("Source", EcLabel::Red, None, ecp::ItemKind::Footage(footage)));
        }
        let mut l = ecp::build::layer(&mut ec, &comp, "Title", ecp::LayerSource::Text, (W, H), None);
        if let Some(pr) = l.props.prop_mut("text/sourceText") {
            pr.value = EcValue::Text(Box::new(TextDoc { text: text.to_string(), size, ..Default::default() }));
        }
        if let Some(pos) = l.props.prop_mut("transform/position") {
            match keys {
                Some((a, b)) => {
                    pos.keys = vec![Keyframe::new(EcTick::ZERO, EcValue::Vec3(a)), Keyframe::new(EcTick(40 * TICKS_PER_SECOND / 24), EcValue::Vec3(b))];
                }
                None => pos.value = EcValue::Vec3([75.0, 90.0, 0.0]),
            }
        }
        let c = ec.comp_mut(cid).expect("comp");
        c.layers.push(l);
        if let Some(fid) = fid {
            c.layers.push(ecp::build::layer(&mut ec, &comp, "Source", ecp::LayerSource::Footage { item: fid }, (W, H), None));
        }

        let mut p = Project::new(name);
        let generator = if with_footage { Generator::BarsAndTone } else { Generator::TransparentVideo };
        let g = GeneratorSource::new(generator, W, H, R, Tick(FRAMES * TICKS_PER_SECOND / 24));
        let info = g.info().clone();
        let media = add_media(&mut p, MediaRef::Generator(generator), info);
        let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 1, 0, None);
        let video = p.make_track_item(media, TrackKind::Video, Tick::ZERO, TimeRange::new(Tick::ZERO, R.tick_of(FRAMES)), R).expect("clip");
        let s = p.sequence_mut(seq).expect("sequence");
        s.video_tracks[0].items.push(video);
        s.video_tracks[0].sort();

        let mut doc = FrameForgeProject::new(p);
        let composition = doc.composition_mut(media);
        composition.ec_project = Some(ec);
        composition.comp = Some(cid);
        composition.source_item = fid;
        let sources = ProjectSources::build(&doc).expect("sources");
        let img = render_sequence(&doc.project, seq, R.tick_of(6), RenderOptions::default(), &sources);
        let mut cells = [[0usize; 4]; 4];
        for y in 0..img.h {
            for x in 0..img.w {
                let q = img.get(x, y);
                if q[0] > 0.85 && q[1] > 0.85 && q[2] > 0.85 && q[3] > 0.9 {
                    cells[y * 4 / img.h][x * 4 / img.w] += 1;
                }
            }
        }
        println!("diagnostic matrix {name}: grid {cells:?}");
    };

    make("HI", 60.0, None, true, "a: HI 60 static over footage");
    make("FrameForge", 18.0, None, true, "b: FrameForge 18 static over footage");
    make("FrameForge", 18.0, Some(([75.0, 150.0, 0.0], [215.0, 150.0, 0.0])), true, "c: FrameForge 18 keys over footage");
    make("FrameForge", 18.0, Some(([75.0, 150.0, 0.0], [215.0, 150.0, 0.0])), false, "d: FrameForge 18 keys no footage");
}

#[test]
fn m4_comp_layers_over_footage_in_the_timeline() {
    let (mut doc, seq, media) = bars_project();
    let (ec_project, comp_id, fid) = comp_project();
    let composition = doc.composition_mut(media);
    composition.ec_project = Some(ec_project);
    composition.comp = Some(comp_id);
    composition.source_item = Some(fid);

    let sources = ProjectSources::build(&doc).expect("sources");

    // The animation really runs: the text starts left and ends right, drawn
    // over the footage by the compositing engine's layer compositor.
    let early = frame_at(&doc, seq, &sources, 6); // 0.25 s
    let late = frame_at(&doc, seq, &sources, 42); // 1.75 s

    // Diagnostics: what the document holds, and where the bright pixels are.
    {
        let composition = doc.compositions.get(&media).expect("composition");
        let project = composition.ec_project.as_ref().expect("ec project");
        let cid = composition.comp.expect("comp id");
        if let Some(c) = project.comp(cid) {
            let l0 = c.layers.first().expect("layer 0");
            let pos = l0.props.prop("transform/position");
            println!(
                "diagnostic doc: layer0 position keys {}, static {:?}, sourceText {}",
                pos.map(|pr| pr.keys.len()).unwrap_or(9999),
                pos.map(|pr| pr.value.clone()),
                l0.props.prop("text/sourceText").is_some()
            );
        }
    }
    for (name, img) in [("early", &early), ("late", &late)] {
        let mut cells = [[0usize; 4]; 4];
        for y in 0..img.h {
            for x in 0..img.w {
                let p = img.get(x, y);
                if p[0] > 0.85 && p[1] > 0.85 && p[2] > 0.85 && p[3] > 0.9 {
                    cells[y * 4 / img.h][x * 4 / img.w] += 1;
                }
            }
        }
        println!("diagnostic {name}: bright grid {cells:?}");
    }

    // The animation really runs: the text starts left and ends right, drawn
    // over the footage by the compositing engine's layer compositor.
    let (el, er) = bright_halves(&early);
    let (ll, lr) = bright_halves(&late);
    assert!(el > 100 && er < el / 4, "the text must start on the left: {el} left vs {er} right");
    assert!(lr > 100 && ll < lr / 4, "the text must end on the right: {ll} left vs {lr} right");

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

    // Time is direction-independent: rendering the late frame first changes nothing.
    let late_first = frame_at(&doc, seq, &sources, 42);
    let early_again = frame_at(&doc, seq, &sources, 6);
    assert_eq!(late_first, late, "backward scrubbing must reproduce the frame");
    assert_eq!(early_again, early, "backward scrubbing must reproduce the frame");

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

#[test]
fn m4_tone_generator_plays() {
    let mut p = Project::new("tone");
    let g = GeneratorSource::new(Generator::Tone { hz: 440.0, db: -6.0 }, W, H, R, Tick(TICKS_PER_SECOND));
    let info = g.info().clone();
    assert!(info.video.is_none() && info.has_audio(), "a tone generator is audio-only");
    let media = add_media(&mut p, MediaRef::Generator(Generator::Tone { hz: 440.0, db: -6.0 }), info);
    let seq = p.new_sequence("Main", SequenceSettings { width: W, height: H, frame_rate: R, ..Default::default() }, 0, 1, None);
    let source = TimeRange::new(Tick::ZERO, R.tick_of(24));
    let audio = p.make_track_item(media, TrackKind::Audio, Tick::ZERO, source, R).expect("audio item");
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
    let source = TimeRange::new(Tick::ZERO, Tick(TICKS_PER_SECOND / 2));
    let audio = p.make_track_item(item, TrackKind::Audio, Tick::ZERO, source, R).expect("audio item");
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
    let source = TimeRange::new(Tick::ZERO, R.tick_of(24));
    let video = doc.project.make_track_item(item, TrackKind::Video, Tick::ZERO, source, R).expect("clip");
    let s = doc.project.sequence_mut(seq).expect("sequence");
    s.video_tracks[0].items.push(video);
    s.video_tracks[0].sort();

    let sources = ProjectSources::build(&doc).expect("sources");
    let img = render_sequence(&doc.project, seq, R.tick_of(12), RenderOptions::default(), &sources);
    let c = img.get(W as usize / 2, H as usize / 2);
    assert!(c[0] > 0.9 && c[1] < 0.1 && c[2] < 0.1 && c[3] > 0.9, "the imported comp must paint its solid red, got {c:?}");
    // The comp is alpha-carrying content over a transparent carrier: where its
    // layers do not reach, the timeline below shows through.
}
