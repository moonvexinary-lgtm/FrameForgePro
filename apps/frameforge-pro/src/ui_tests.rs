//! Headless UI interaction tests.
//!
//! These drive the application the way a person does: real `RawInput` events
//! (key presses, pointer clicks and drags) into the same [`App::ui_frame`] a
//! window calls, run by an `egui::Context` at a fixed size. Every assertion is
//! made against the engine document or the editor state, never against pixels,
//! so a restyled control does not break a test that is really about what the
//! command did.
//!
//! The harness is deliberately small: the UI is almost entirely custom-painted
//! rectangles, so an accessibility-tree driver would be testing a tree that the
//! timeline and transport barely populate. What matters is that the real frame
//! loop, the real key map and the real engine commands agree.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};

use crate::app::App;
use crate::edit;
use crate::relink::RelinkDialog;

/// A desktop-sized frame, so the panels lay out as they do in use rather than
/// collapsing into a degenerate rect.
const SCREEN: Vec2 = Vec2::new(1600.0, 900.0);

struct Harness {
    context: egui::Context,
    app: App,
    time: f64,
    /// The modifier state egui currently believes, updated with
    /// `Event::ModifiersChanged` the way a real backend reports it.
    held: Modifiers,
}

impl Harness {
    /// A harness over `app`, positioned so the first asserted frame is not the
    /// layout-only first pass.
    fn new(app: App) -> Self {
        let mut harness = Self { context: egui::Context::default(), app, time: 0.0, held: Modifiers::NONE };
        harness.frame(Vec::new());
        harness
    }

    /// Deliver one frame, exactly as a backend would: a screen rect, a focused
    /// window and this frame's events.
    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo::default());
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            events,
            focused: true,
            viewports,
            ..Default::default()
        };
        let app = &mut self.app;
        let mut output = self.context.run_ui(input, |ui| app.ui_frame(ui));
        // There is no painter here. `TexturesDelta` asserts on drop that a
        // backend consumed its deltas, so discard them explicitly instead of
        // letting the headless output trip the assertion.
        output.textures_delta.clear();
    }

    /// A whole key press: modifiers change first (as a real backend reports
    /// them), the down event in one frame, the release in the next, and the
    /// modifiers released afterwards. One frame per event keeps egui's repeat
    /// detection honest.
    fn press(&mut self, key: Key, modifiers: Modifiers) {
        let mut events = Vec::new();
        if modifiers != self.held {
            events.push(Event::ModifiersChanged(modifiers));
            self.held = modifiers;
        }
        events.push(key_event(key, true, modifiers));
        self.frame(events);
        self.frame(vec![key_event(key, false, modifiers)]);
        if self.held != Modifiers::NONE {
            self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
            self.held = Modifiers::NONE;
        }
    }

    /// The rect egui registered for a control on the last frame. Custom-painted
    /// controls still register an id through `Ui::interact`, so their rect is
    /// available without a second geometry helper.
    fn rect_of(&self, id: egui::Id) -> Option<Rect> {
        self.context.read_response(id).map(|response| response.rect)
    }

    /// A whole click at an arbitrary point: hover, press, release. egui
    /// reports the click on the release frame.
    fn click_at(&mut self, pos: Pos2) {
        self.frame(vec![Event::PointerMoved(pos)]);
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: self.held }]);
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: self.held }]);
    }

    /// A whole click at the centre of a registered control: hover, press,
    /// release.
    fn click(&mut self, id: egui::Id) {
        let pos = self.rect_of(id).unwrap_or_else(|| panic!("no widget registered for {id:?}")).center();
        self.click_at(pos);
    }

    /// A drag: hover, press at `from`, move through each waypoint, release at
    /// the last one. The timeline decides the gesture on the frame egui first
    /// calls it a drag, which is the first waypoint past egui's 6 px click
    /// tolerance — so a trim test keeps that waypoint inside the 8 px edge
    /// band, exactly as a careful person would.
    fn drag(&mut self, from: Pos2, path: &[Pos2]) {
        let end = *path.last().expect("a drag needs at least one waypoint");
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: self.held }]);
        for point in path {
            self.frame(vec![Event::PointerMoved(*point)]);
        }
        self.frame(vec![Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: self.held }]);
    }
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers }
}

/// The desktop command modifier (Ctrl on Windows/Linux, Command on macOS).
fn command(shift: bool) -> Modifiers {
    Modifiers { command: true, ctrl: true, shift, ..Modifiers::NONE }
}

fn video_clips(app: &App) -> usize {
    app.sequence().map(|s| s.video_tracks.iter().map(|t| t.items.len()).sum()).unwrap_or(0)
}

fn audio_clips(app: &App) -> usize {
    app.sequence().map(|s| s.audio_tracks.iter().map(|t| t.items.len()).sum()).unwrap_or(0)
}

/// A fresh app with one sequence and one Bars & Tone clip (picture and sound)
/// placed on the patched tracks. The placement is a real engine command, so the
/// history has a step for the undo test to reach; the item id comes back so a
/// test can check that undo reached the placement and not the edit that created
/// the media.
fn seeded() -> (Harness, ItemId) {
    let mut app = App::new();
    app.execute("file.newSequence", serde_json::json!({"name": "Main", "width": 1280, "height": 720, "fps": 24, "video": 2, "audio": 2}))
        .expect("create the sequence");

    // Both streams: `Tone` is audio-only, and the undo/split tests need a clip
    // on a video track as well as its linked sound.
    let generator = Generator::BarsAndTone;
    let source = GeneratorSource::new(generator.clone(), 1280, 720, FrameRate::FPS_24, Tick(10 * TICKS_PER_SECOND));
    let info = source.info().clone();
    let name = info.name.clone();
    let item: ItemId = app
        .edit_project("Bars & Tone", |project, _| {
            Ok(project.add_item(
                name.as_str(),
                Label::Iris,
                ItemKind::Media(MediaClip {
                    media: MediaRef::Generator(generator),
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
            ))
        })
        .expect("add the tone item");
    edit::place_at_playhead(&mut app, item, false);

    assert_eq!(video_clips(&app), 1, "the fixture placed one video clip");
    assert_eq!(audio_clips(&app), 1, "the fixture placed the matching audio clip");
    (Harness::new(app), item)
}

#[test]
fn ctrl_z_and_ctrl_shift_z_undo_and_redo_an_engine_edit() {
    let (mut h, item) = seeded();

    h.press(Key::Z, command(false));
    assert_eq!(video_clips(&h.app), 0, "Ctrl+Z undoes the placement through the engine history");
    assert!(h.app.session.project.items.contains_key(&item), "the undo reached the placement, not the media edit");
    assert!(h.app.status.starts_with("Undo "), "the frame ran the undo path, not a stray edit: {}", h.app.status);

    h.press(Key::Z, command(true));
    assert_eq!(video_clips(&h.app), 1, "Ctrl+Shift+Z redoes it");
    assert_eq!(audio_clips(&h.app), 1, "the audio half comes back with the video half");
    assert!(h.app.status.starts_with("Redo "), "the frame ran the redo path: {}", h.app.status);
}

#[test]
fn ctrl_k_splits_the_clips_under_the_playhead_through_the_real_key_map() {
    let (mut h, _) = seeded();
    h.app.playhead = Tick(2 * TICKS_PER_SECOND);

    h.press(Key::K, command(false));

    assert_eq!(video_clips(&h.app), 2, "Add Edit cut V1 at the playhead");
    assert_eq!(audio_clips(&h.app), 2, "Add Edit cut A1 at the playhead");
    assert_eq!(h.app.status, "Add Edit (all tracks)");
}

#[test]
fn m_adds_a_marker_and_shift_m_jumps_to_the_next_one() {
    let (mut h, _) = seeded();
    h.app.playhead = Tick(3 * TICKS_PER_SECOND);

    h.press(Key::M, Modifiers::NONE);
    let marker = h.app.sequence().and_then(|s| s.markers.first().map(|m| m.start)).expect("M added a marker");
    assert_eq!(h.app.sequence().map_or(0, |s| s.markers.len()), 1, "one marker, not a marker per frame");
    assert_eq!(marker, h.app.playhead, "the marker sits on the snapped playhead");

    h.app.playhead = Tick::ZERO;
    h.press(Key::M, Modifiers::SHIFT);
    assert_eq!(h.app.sequence().map_or(0, |s| s.markers.len()), 1, "Shift+M jumps, it does not add");
    assert_eq!(h.app.playhead, marker, "Shift+M moved the playhead to the marker");
}

#[test]
fn clicking_the_a1_mute_button_is_one_undoable_track_edit() {
    let (mut h, _) = seeded();
    let a1 = h.app.sequence().expect("the fixture has a sequence").audio_tracks[0].id;
    assert!(!h.app.sequence().expect("sequence").audio_tracks[0].muted, "A1 starts unmuted");

    // The track header paints its own squares but registers each one with
    // `Ui::interact`, so egui knows the rect and the click goes through the
    // same `ff-track-mute` id the editor uses.
    h.click(egui::Id::new(("ff-track-mute", a1.0)));

    assert!(h.app.sequence().expect("sequence").audio_tracks[0].muted, "the click ran timeline.setTrack");
    assert_eq!(h.app.status, "Mute updated");

    h.press(Key::Z, command(false));
    assert!(!h.app.sequence().expect("sequence").audio_tracks[0].muted, "the header click is one undo step");
}

/// The fixture's one video clip: its row on the timeline (video tracks are
/// drawn first) and the start and duration the engine document has for it.
fn v1_clip(app: &App) -> (usize, Tick, Tick) {
    let sequence = app.sequence().expect("the fixture has a sequence");
    let (row, item) = sequence
        .video_tracks
        .iter()
        .enumerate()
        .find_map(|(row, track)| track.items.first().map(|item| (row, item)))
        .expect("the fixture placed one video clip");
    (row, item.start, item.duration)
}

/// The fixture's V1 clip as the engine document has it: start and duration.
fn v1_span(app: &App) -> (Tick, Tick) {
    let (_, start, duration) = v1_clip(app);
    (start, duration)
}

/// The engine's id for the fixture's video clip, which is what a selection holds.
fn v1_id(app: &App) -> filmcraft_project::ClipId {
    let (row, _, _) = v1_clip(app);
    app.sequence().expect("the fixture has a sequence").video_tracks[row].items[0].id
}

/// Lay the timeline out at a known zoom and return the body rect and the V1
/// clip's painted rect, both in screen points. `zoom` is set directly and one
/// frame is rendered, so the rects egui stores belong to that layout; the clip
/// rect comes from the same function the draw loop uses, so a scripted drag
/// grabs exactly what a person sees.
fn timeline_at(h: &mut Harness, zoom: f32) -> (Rect, Rect) {
    h.app.zoom = zoom;
    h.frame(Vec::new());
    let body = h.rect_of(egui::Id::new("ff-timeline-body")).expect("the timeline body registers `ff-timeline-body`");
    let (row, start, duration) = v1_clip(&h.app);
    (body, edit::timeline_clip_rect(body, row, start, duration, zoom))
}

#[test]
fn clicking_the_ruler_puts_the_playhead_where_the_pointer_is() {
    let (mut h, _) = seeded();
    let (body, _) = timeline_at(&mut h, 40.0);

    // Two seconds in at 40 px/s, inside the ruler strip above every clip row.
    h.click_at(Pos2::new(body.left() + 2.0 * 40.0, body.top() + edit::RULER_H / 2.0));

    assert!((h.app.playhead.seconds() - 2.0).abs() < 0.01, "the ruler click scrubbed to {}s", h.app.playhead.seconds());
    assert!(h.app.selection.is_none(), "scrubbing empty space clears the clip selection");
}

#[test]
fn dragging_a_clip_moves_it_and_ctrl_z_undoes_the_move() {
    let (mut h, _) = seeded();
    let (_, clip) = timeline_at(&mut h, 40.0);
    let (start_before, duration_before) = v1_span(&h.app);
    assert_eq!(start_before, Tick::ZERO);

    // Press in the middle of the clip, then move 70 px right. egui calls it a
    // drag on the first waypoint past its 6 px click tolerance and the timeline
    // measures from there, so the second waypoint is the 60 px (1.5 s at
    // 40 px/s) that count.
    let grab = clip.center();
    h.drag(grab, &[grab + Vec2::new(10.0, 0.0), grab + Vec2::new(70.0, 0.0)]);

    let (start, duration) = v1_span(&h.app);
    assert!((start.seconds() - 1.5).abs() < 0.02, "V1 moved by the dragged distance: {}s", start.seconds());
    assert_eq!(duration, duration_before, "a move keeps the clip's length");
    assert_eq!(h.app.status, "Moved clip");

    h.press(Key::Z, command(false));
    assert_eq!(v1_span(&h.app).0, Tick::ZERO, "the drag is one undo step");
}

#[test]
fn dragging_the_out_edge_trims_the_tail_and_ctrl_z_restores_the_length() {
    let (mut h, _) = seeded();
    let (_, clip) = timeline_at(&mut h, 40.0);
    let (_, duration_before) = v1_span(&h.app);
    let y = clip.center().y;

    // Press inside the 8 px edge band, and keep the drag-start waypoint inside
    // it too: the timeline reads the gesture from that first waypoint, so a
    // waypoint that escaped the band would become a move instead of a trim.
    let press = Pos2::new(clip.right() - 1.0, y);
    let grab = Pos2::new(clip.right() - 7.9, y);
    let end = Pos2::new(clip.right() - 47.9, y);
    h.drag(press, &[grab, end]);

    let (start, duration) = v1_span(&h.app);
    assert_eq!(start, Tick::ZERO, "trimming the out edge leaves the start alone");
    assert!((duration.seconds() - 9.0).abs() < 0.02, "the tail lost 1 s: {}s", duration.seconds());
    assert_eq!(h.app.status, "Trimmed out edge");

    h.press(Key::Z, command(false));
    assert_eq!(v1_span(&h.app).1, duration_before, "the trim is one undo step");
}

#[test]
fn dragging_the_in_edge_trims_the_head_and_leaves_the_tail_in_place() {
    let (mut h, _) = seeded();
    let (_, clip) = timeline_at(&mut h, 40.0);
    let (start_before, duration_before) = v1_span(&h.app);
    let y = clip.center().y;

    let press = Pos2::new(clip.left() + 1.0, y);
    let grab = Pos2::new(clip.left() + 7.9, y);
    let end = Pos2::new(clip.left() + 27.9, y);
    h.drag(press, &[grab, end]);

    let (start, duration) = v1_span(&h.app);
    assert!((start.seconds() - 0.5).abs() < 0.02, "the head moved in by 0.5 s: {}s", start.seconds());
    assert!((duration.seconds() - 9.5).abs() < 0.02, "the clip is 0.5 s shorter: {}s", duration.seconds());
    assert_eq!(start + duration, start_before + duration_before, "a regular trim moves the head, not the tail");
    assert_eq!(h.app.status, "Trimmed in edge");
}

#[test]
fn clicking_a_clip_selects_it_and_clicking_empty_space_deselects() {
    let (mut h, _) = seeded();
    let (body, clip) = timeline_at(&mut h, 40.0);
    assert!(h.app.selection.is_none(), "nothing is selected before the click");

    h.click_at(clip.center());
    assert_eq!(h.app.selection, Some(v1_id(&h.app)), "the click selected the clip under the pointer");

    // Same row, past the clip's right edge: a hit-rect miss, so the timeline
    // scrubs and clears the selection instead of selecting anything.
    let empty = Pos2::new(clip.right() + 40.0, clip.center().y);
    h.click_at(empty);
    assert!(h.app.selection.is_none(), "empty space on a clip row deselects");
    let scrubbed = (empty.x - body.left()) as f64 / 40.0;
    assert!((h.app.playhead.seconds() - scrubbed).abs() < 0.01, "and scrubs to {}s, not {}", h.app.playhead.seconds(), scrubbed);
}

#[test]
fn delete_removes_the_selected_clip_and_ctrl_z_restores_it() {
    let (mut h, _) = seeded();
    let (_, clip) = timeline_at(&mut h, 40.0);

    h.click_at(clip.center());
    assert_eq!(h.app.selection, Some(v1_id(&h.app)), "the pointer selected the clip to delete");

    h.press(Key::Delete, Modifiers::NONE);

    assert_eq!(video_clips(&h.app), 0, "Delete removed the selected video clip");
    assert_eq!(audio_clips(&h.app), 0, "the linked sound went with it");
    assert_eq!(h.app.status, "Clip removed");
    assert!(h.app.selection.is_none(), "a deleted clip is no longer selected");

    h.press(Key::Z, command(false));
    assert_eq!(video_clips(&h.app), 1, "Ctrl+Z brought the clip back");
    assert_eq!(audio_clips(&h.app), 1, "and its linked sound");
}

// ---- relinking missing media ----------------------------------------------

/// A scratch folder under the OS temp directory, unique per call.
fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ff-relink-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the scratch folder");
    dir
}

/// Write a real 16-bit PCM WAV the media crate can open. The engine's own
/// movie helpers are not visible from this crate, and a WAV is one
/// deterministic write with no encoder.
fn wav_fixture(dir: &Path, name: &str) -> PathBuf {
    let samples: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.05).sin() * 0.2).collect();
    let path = dir.join(name);
    std::fs::write(&path, filmcraft_media::wav::write_wav16(&samples, 1, 48_000)).expect("write the WAV");
    path
}

/// A real file's properties, read once and dropped. `open_path` reads the file
/// whole, so no handle is left open on it — the recorded path of a missing item
/// is never touched on disk, which matters on Windows where an imported file is
/// held open by the streaming reader.
fn wav_info(path: &Path) -> filmcraft_media::MediaInfo {
    let source = filmcraft_media::open_path(path, &[]).expect("open the WAV to read its properties");
    source.info().clone()
}

/// Add a project item that names `recorded` (a file that is not there) but
/// carries a real file's properties, so the offline scan reports it missing.
fn add_missing_item(app: &mut App, name: &str, recorded: &Path, info: filmcraft_media::MediaInfo) -> ItemId {
    app.edit_project("Missing media", |project, _| {
        Ok(project.add_item(
            name,
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::File { path: recorded.display().to_string() },
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
        ))
    })
    .expect("add the item that names the missing file")
}

/// The file path the project stores for a media item.
fn media_path(app: &App, item: ItemId) -> String {
    match &app.session.project.item(item).and_then(|i| i.as_media()).expect("a media item").media {
        MediaRef::File { path } => path.clone(),
        MediaRef::Generator(_) => panic!("the relink fixture is file media"),
    }
}

#[test]
fn a_missing_file_shows_the_badge_and_the_relink_dialog_lists_it() {
    let (mut h, _) = seeded();
    h.app.refresh_offline();
    assert!(!h.app.has_offline_media(), "a project of generators has nothing missing");
    assert!(RelinkDialog::open(&h.app).rows.is_empty(), "and the dialog has nothing to show");

    let present = wav_fixture(&scratch("present"), "take.wav");
    let recorded = scratch("missing").join("take.wav");
    let item = add_missing_item(&mut h.app, "take", &recorded, wav_info(&present));
    h.app.refresh_offline();

    assert!(h.app.has_offline_media(), "the badge appears once a named file is not there");
    let dialog = RelinkDialog::open(&h.app);
    assert_eq!(dialog.rows.len(), 1, "the one missing file is listed");
    assert_eq!(dialog.rows[0].item, item, "by the item the project holds");
    assert_eq!(dialog.rows[0].file_name, "take.wav");
    assert_eq!(dialog.rows[0].status, "missing");
    assert_eq!(dialog.selected, Some(0), "the first row is ready to link");
    assert_eq!(dialog.path, recorded.display().to_string(), "the path field starts at the recorded location");
}

#[test]
fn linking_a_missing_row_repairs_the_path_and_ctrl_z_restores_it() {
    let (mut h, _) = seeded();
    let found = wav_fixture(&scratch("found"), "take.wav");
    let recorded = scratch("recorded").join("take.wav");
    let item = add_missing_item(&mut h.app, "take", &recorded, wav_info(&found));
    let old_path = media_path(&h.app, item);
    h.app.refresh_offline();
    assert!(h.app.has_offline_media(), "the project cannot find the recorded file");

    let mut dialog = RelinkDialog::open(&h.app);
    dialog.select(0);
    dialog.path = found.display().to_string();
    dialog.link(&mut h.app, false);

    assert_eq!(media_path(&h.app, item), found.display().to_string(), "the project now points at the real file");
    assert!(!h.app.has_offline_media(), "the badge clears once the file is linked");
    assert!(dialog.rows.is_empty(), "the dialog has nothing left to link");
    assert!(dialog.message.as_deref().unwrap_or_default().starts_with("Linked"), "the dialog reports the link: {:?}", dialog.message);

    h.press(Key::Z, command(false));
    assert_eq!(media_path(&h.app, item), old_path, "the relink is one undo step");
}

#[test]
fn a_mismatched_file_is_refused_until_forced() {
    let (mut h, _) = seeded();
    let dir = scratch("candidates");
    let found = wav_fixture(&dir, "take.wav");
    let other = wav_fixture(&dir, "different.wav");
    let recorded = scratch("recorded-2").join("take.wav");
    let item = add_missing_item(&mut h.app, "take", &recorded, wav_info(&found));
    h.app.refresh_offline();

    let mut dialog = RelinkDialog::open(&h.app);
    dialog.select(0);
    dialog.path = other.display().to_string();
    dialog.link(&mut h.app, false);

    assert!(dialog.message.as_deref().unwrap_or_default().contains("doesn't match"), "the mismatch is reported: {:?}", dialog.message);
    assert_eq!(dialog.rows.len(), 1, "the row is still there");
    assert!(h.app.has_offline_media(), "nothing was linked");

    dialog.link(&mut h.app, true);
    assert!(dialog.rows.is_empty(), "Force linked the mismatched file anyway");
    assert_eq!(media_path(&h.app, item), other.display().to_string());
}

#[test]
fn the_relink_button_opens_the_dialog_and_its_link_button_repairs_the_project() {
    let (mut h, _) = seeded();
    let found = wav_fixture(&scratch("button-found"), "take.wav");
    let recorded = scratch("button-recorded").join("take.wav");
    let item = add_missing_item(&mut h.app, "take", &recorded, wav_info(&found));
    h.app.refresh_offline();
    h.frame(Vec::new());

    // The badge's button appears in the media browser; a click opens the dialog.
    h.click(egui::Id::new("ff-relink-open"));
    assert!(h.app.relink.is_some(), "the media-browser button opened the dialog");

    // One frame lays the floating window out, then its rows are addressable.
    h.frame(Vec::new());
    h.click(egui::Id::new(("ff-relink-row", 0usize)));
    assert_eq!(h.app.relink.as_ref().and_then(|d| d.selected), Some(0), "clicking a row selects it");

    // Typing would need focus and text events; the field is set directly, the
    // way a pasted path arrives, and the Link button is still a real click.
    h.app.relink.as_mut().expect("the dialog is open").path = found.display().to_string();
    h.frame(Vec::new());
    h.click(egui::Id::new("ff-relink-link"));

    let dialog = h.app.relink.as_ref().expect("the dialog stays open");
    assert!(dialog.rows.is_empty(), "the Link button cleared the row");
    assert_eq!(media_path(&h.app, item), found.display().to_string(), "and repointed the project");
    assert!(!h.app.has_offline_media(), "the badge is gone");
}

// ---- source range drag and source markers ---------------------------------

/// The marks the project holds for a media item.
fn source_marks(app: &App, item: ItemId) -> (Option<Tick>, Option<Tick>) {
    let media = app.session.project.item(item).and_then(|i| i.as_media()).expect("a media item");
    (media.mark_in, media.mark_out)
}

/// The open source's marker starts, in project order.
fn source_marker_starts(app: &App) -> Vec<Tick> {
    let Some(item) = app.session.state.source_item else { return Vec::new() };
    app.session.project.item(item).and_then(|i| i.as_media()).map(|m| m.markers.iter().map(|k| k.start).collect()).unwrap_or_default()
}

/// Open the seeded clip in the Source monitor and mark In at 2 s and Out at
/// 6 s through the real commands, so the range bar has two interior handles.
fn source_with_marks(h: &mut Harness, item: ItemId) {
    edit::open_source(&mut h.app, item);
    h.app.session.execute("source.setPlayhead", serde_json::json!({"frame": 48})).unwrap();
    edit::source_mark(&mut h.app, true);
    h.app.session.execute("source.setPlayhead", serde_json::json!({"frame": 144})).unwrap();
    edit::source_mark(&mut h.app, false);
    h.frame(Vec::new());
}

#[test]
fn dragging_the_source_in_handle_sets_the_mark_and_one_undo_restores_it() {
    let (mut h, item) = seeded();
    source_with_marks(&mut h, item);
    let before = source_marks(&h.app, item);
    assert_eq!(before, (Some(Tick(2 * TICKS_PER_SECOND)), Some(Tick(6 * TICKS_PER_SECOND))), "the fixture marked 2 s to 6 s");

    let bar = h.rect_of(egui::Id::new("ff-source-range")).expect("the range bar is laid out");
    let handle = h.rect_of(egui::Id::new("ff-source-range-in")).expect("the In handle is laid out");
    // A tenth of the bar is exactly one second of this ten-second clip.
    let step = egui::vec2(bar.width() / 10.0, 0.0);

    h.drag(handle.center(), &[handle.center() + step]);

    assert_eq!(source_marks(&h.app, item).0, Some(Tick(3 * TICKS_PER_SECOND)), "dragging the In handle by one second moved the mark");
    assert_eq!(source_marks(&h.app, item).1, before.1, "the Out mark stayed put");

    h.press(Key::Z, command(false));
    assert_eq!(source_marks(&h.app, item), before, "the whole drag is one undo step");
}

#[test]
fn dragging_the_source_range_body_moves_both_marks() {
    let (mut h, item) = seeded();
    source_with_marks(&mut h, item);

    let bar = h.rect_of(egui::Id::new("ff-source-range")).expect("the range bar is laid out");
    let body = h.rect_of(egui::Id::new("ff-source-range-body")).expect("the range body is laid out");
    let step = egui::vec2(bar.width() / 10.0, 0.0);

    h.drag(body.center(), &[body.center() + step]);

    assert_eq!(
        source_marks(&h.app, item),
        (Some(Tick(3 * TICKS_PER_SECOND)), Some(Tick(7 * TICKS_PER_SECOND))),
        "the body drag slid both marks forward by one second"
    );
}

#[test]
fn the_source_marker_strip_adds_seeks_and_deletes() {
    let (mut h, item) = seeded();
    edit::open_source(&mut h.app, item);
    h.app.session.execute("source.setPlayhead", serde_json::json!({"frame": 48})).unwrap();
    h.frame(Vec::new());

    h.click(egui::Id::new("ff-source-marker-add"));
    assert_eq!(source_marker_starts(&h.app), vec![Tick(2 * TICKS_PER_SECOND)], "the button added a marker on the playhead");

    // Moving away and clicking the row brings the playhead back to the marker.
    h.app.session.execute("source.setPlayhead", serde_json::json!({"frame": 0})).unwrap();
    h.frame(Vec::new());
    h.click(egui::Id::new(("ff-source-marker", 0usize)));
    assert_eq!(h.app.session.state.source_playhead, Tick(2 * TICKS_PER_SECOND), "the row seeks to its marker");

    h.frame(Vec::new());
    h.click(egui::Id::new(("ff-source-marker-del", 0usize)));
    assert!(source_marker_starts(&h.app).is_empty(), "the row's delete removed the marker");

    h.press(Key::Z, command(false));
    assert_eq!(source_marker_starts(&h.app), vec![Tick(2 * TICKS_PER_SECOND)], "deleting a source marker is undoable");
}
