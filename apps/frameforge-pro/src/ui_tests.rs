//! Headless UI interaction tests.
//!
//! These drive the application the way a person does: real `RawInput` events
//! (key down, key up) into the same [`App::ui_frame`] a window calls, run by an
//! `egui::Context` at a fixed size. Every assertion is made against the engine
//! document or the editor state, never against pixels, so a restyled control
//! does not break a test that is really about what the command did.
//!
//! The harness is deliberately small: the UI is almost entirely custom-painted
//! rectangles, so an accessibility-tree driver would be testing a tree that the
//! timeline and transport barely populate. What matters is that the real frame
//! loop, the real key map and the real engine commands agree.

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};

use crate::app::App;
use crate::edit;

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
        let _ = self.context.run_ui(input, |ui| app.ui_frame(ui));
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

/// A fresh app with one sequence and one tone clip placed on the patched tracks.
/// The placement is a real engine command, so the history has a step for the
/// undo test to reach; the item id comes back so a test can check that undo
/// reached the placement and not the edit that created the media.
fn seeded() -> (Harness, ItemId) {
    let mut app = App::new();
    app.execute("file.newSequence", serde_json::json!({"name": "Main", "width": 1280, "height": 720, "fps": 24, "video": 2, "audio": 2}))
        .expect("create the sequence");

    let tone = Generator::Tone { hz: 440.0, db: -6.0 };
    let source = GeneratorSource::new(tone.clone(), 1280, 720, FrameRate::FPS_24, Tick(10 * TICKS_PER_SECOND));
    let info = source.info().clone();
    let name = info.name.clone();
    let item: ItemId = app
        .edit_project("New Tone", |project, _| {
            Ok(project.add_item(
                name.as_str(),
                Label::Iris,
                ItemKind::Media(MediaClip {
                    media: MediaRef::Generator(tone),
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
