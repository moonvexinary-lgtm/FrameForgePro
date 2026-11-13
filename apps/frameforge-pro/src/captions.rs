//! Caption editor (T26).
//!
//! The engine owns captions: `captions.list` reports every caption track (with
//! the engine's own `C1`/`C2` slot labels and its timecodes) and every caption
//! on it, and the `captions.*` commands write them as one undoable project edit
//! each. The editor is a floating window over the timeline: it lists the tracks
//! and their captions by time, adds a track and a caption at the playhead,
//! selects a row through the engine's caption selection, edits the selected
//! caption's text and timing, and deletes it.
//!
//! The list itself is a query (`captions.list` is journal-free), so opening the
//! window or refreshing it never dirties the project or adds an undo step.

use filmcraft_time::{TICKS_PER_SECOND, Tick};
use serde_json::{Value, json};

use crate::app::App;
use crate::ui;

/// One caption track, as `captions.list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackRow {
    pub id: u64,
    /// The engine's own slot label: `C1`, `C2`, ...
    pub label: String,
    pub name: String,
    pub format: String,
    /// How many captions the track holds.
    pub count: usize,
}

/// One caption, as `captions.list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionRow {
    pub id: u64,
    pub track: u64,
    /// The engine's timecode at the sequence rate.
    pub in_tc: String,
    pub duration: String,
    pub text: String,
    /// The caption's own bounds, in ticks, for seeding the draft fields.
    pub start: i64,
    pub end: i64,
}

/// The open Caption editor. Editor state: not saved with the project and not
/// part of the undo history.
#[derive(Clone, Debug)]
pub struct CaptionEditor {
    pub tracks: Vec<TrackRow>,
    /// Every caption on every track, in timeline order.
    pub captions: Vec<CaptionRow>,
    /// The track the window adds to.
    pub track: Option<u64>,
    /// The caption the engine's caption selection names.
    pub selected: Option<u64>,
    /// Draft text for the selected caption (and the next Add).
    pub text: String,
    /// Draft length in seconds for the next Add.
    pub seconds: f64,
    /// Draft start and length in seconds for the selected caption.
    pub start: f64,
    pub length: f64,
    /// Why the list could not be read, when it could not.
    pub error: Option<String>,
}

impl CaptionEditor {
    fn new() -> Self {
        Self { tracks: Vec::new(), captions: Vec::new(), track: None, selected: None, text: String::new(), seconds: 3.0, start: 0.0, length: 3.0, error: None }
    }
}

/// Open the editor over the active sequence.
pub fn open(app: &mut App) {
    let mut editor = CaptionEditor::new();
    refresh(app, &mut editor);
    app.captions = Some(editor);
}

/// Read `captions.list` into the editor. A query: no undo step, no dirty flag.
pub fn refresh(app: &mut App, editor: &mut CaptionEditor) {
    match app.session.execute("captions.list", json!({})) {
        Ok(value) => {
            let (tracks, captions, selection) = rows_from_list(&value);
            editor.tracks = tracks;
            editor.captions = captions;
            editor.error = None;
            // The engine's caption selection is the truth; the window follows it.
            editor.selected = selection.first().copied();
            if editor.track.is_none_or(|id| !editor.tracks.iter().any(|t| t.id == id)) {
                editor.track = editor.tracks.first().map(|t| t.id);
            }
            if let Some(row) = editor.selected.and_then(|id| editor.captions.iter().find(|r| r.id == id)).cloned() {
                seed(editor, &row);
            }
        }
        Err(error) => {
            editor.tracks.clear();
            editor.captions.clear();
            editor.selected = None;
            editor.error = Some(error.to_string());
        }
    }
}

/// Point the draft fields at a row the document holds.
fn seed(editor: &mut CaptionEditor, row: &CaptionRow) {
    editor.text = row.text.clone();
    editor.start = row.start as f64 / TICKS_PER_SECOND as f64;
    editor.length = (row.end - row.start) as f64 / TICKS_PER_SECOND as f64;
    editor.track = Some(row.track);
}

/// Split a `captions.list` reply into the track rows, the caption rows (in
/// timeline order) and the engine's caption selection.
pub fn rows_from_list(value: &Value) -> (Vec<TrackRow>, Vec<CaptionRow>, Vec<u64>) {
    let mut tracks = Vec::new();
    let mut rows = Vec::new();
    for track in value.get("tracks").and_then(Value::as_array).into_iter().flatten() {
        let id = track.get("id").and_then(Value::as_u64).unwrap_or_default();
        let captions = track.get("captions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        tracks.push(TrackRow {
            id,
            label: track.get("label").and_then(Value::as_str).unwrap_or_default().to_string(),
            name: track.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            format: track.get("format").and_then(Value::as_str).unwrap_or_default().to_string(),
            count: captions.len(),
        });
        for caption in captions {
            rows.push(CaptionRow {
                id: caption.get("id").and_then(Value::as_u64).unwrap_or_default(),
                track: id,
                in_tc: caption.get("in").and_then(Value::as_str).unwrap_or_default().to_string(),
                duration: caption.get("duration").and_then(Value::as_str).unwrap_or_default().to_string(),
                text: caption.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
                start: caption.get("start").and_then(Value::as_i64).unwrap_or_default(),
                end: caption.get("end").and_then(Value::as_i64).unwrap_or_default(),
            });
        }
    }
    rows.sort_by_key(|r| (r.start, r.id));
    let selection = value.get("selection").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
    (tracks, rows, selection)
}

/// Add a subtitle-format caption track and make it the window's target.
pub fn add_track(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let value = app.execute("captions.newTrack", json!({"format": "Subtitle"}))?;
    let id = value.get("track").and_then(Value::as_u64);
    refresh(app, editor);
    if let Some(id) = id {
        editor.track = Some(id);
    }
    app.status = "Added caption track".to_string();
    Ok(())
}

/// Add a caption at the playhead on the window's track, using the drafts.
pub fn add_caption(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    // The engine creates a track on demand; do it here instead so the track is
    // its own undo step and the window can list it before the caption lands.
    if editor.tracks.is_empty() {
        add_track(app, editor)?;
    }
    let mut params = json!({"time": app.playhead.0, "durationSeconds": editor.seconds});
    if let Some(track) = editor.track {
        params["track"] = json!(track);
    }
    let text = editor.text.trim().to_string();
    if !text.is_empty() {
        params["text"] = json!(text);
    }
    let value = app.execute("captions.add", params)?;
    let id = value.get("caption").and_then(Value::as_u64);
    refresh(app, editor);
    if let Some(id) = id {
        editor.selected = Some(id);
        if let Some(row) = editor.captions.iter().find(|r| r.id == id).cloned() {
            seed(editor, &row);
        }
    }
    app.status = "Added caption".to_string();
    Ok(())
}

/// Make `id` the engine's caption selection and seed the drafts from its row.
pub fn select(app: &mut App, editor: &mut CaptionEditor, id: u64) -> Result<(), String> {
    app.session.execute("captions.select", json!({"captions": [id]})).map_err(|e| e.to_string())?;
    editor.selected = Some(id);
    if let Some(row) = editor.captions.iter().find(|r| r.id == id).cloned() {
        seed(editor, &row);
    }
    app.status = "Selected caption".to_string();
    Ok(())
}

/// Write the text draft to the selected caption.
pub fn apply_text(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let id = editor.selected.ok_or_else(|| "select a caption first".to_string())?;
    app.execute("captions.setText", json!({"caption": id, "text": editor.text}))?;
    app.status = "Edited caption text".to_string();
    Ok(())
}

/// Write the start/length drafts to the selected caption as one edit.
pub fn apply_times(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let id = editor.selected.ok_or_else(|| "select a caption first".to_string())?;
    let end = (editor.start + editor.length).max(editor.start);
    app.execute("captions.setTimes", json!({"caption": id, "startSeconds": editor.start, "endSeconds": end}))?;
    app.status = format!("Caption at {:.2} s for {:.2} s", editor.start, editor.length);
    Ok(())
}

/// Delete the selected caption.
pub fn delete(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let id = editor.selected.ok_or_else(|| "select a caption first".to_string())?;
    app.execute("captions.delete", json!({"captions": [id]}))?;
    editor.selected = None;
    refresh(app, editor);
    app.status = "Deleted caption".to_string();
    Ok(())
}

/// Step the playhead to the next or previous caption start.
pub fn step(app: &mut App, editor: &mut CaptionEditor, forward: bool) -> Result<(), String> {
    let command = if forward { "captions.next" } else { "captions.previous" };
    let value = app.execute(command, json!({}))?;
    if let Some(time) = value.get("time").and_then(Value::as_i64) {
        app.playhead = Tick(time);
    }
    refresh(app, editor);
    app.status = if forward { "Next caption".to_string() } else { "Previous caption".to_string() };
    Ok(())
}

fn action(ui: &mut egui::Ui, id: &str, label: &str, accent: bool) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), ui::TEXT_HI).size().x + 20.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    let response = ui.interact(rect, egui::Id::new(id), egui::Sense::click());
    let fill = if accent {
        if response.hovered() { ui::ACCENT } else { ui::ACCENT_DIM }
    } else if response.hovered() {
        ui::BG_HOVER
    } else {
        ui::BG_INSET
    };
    let color = if accent { ui::TEXT_HI } else { ui::TEXT_MID };
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
    response
}

/// Draw the editor when one is open. Called every frame from `App::ui_frame`,
/// so it floats over the timeline whose document it edits.
pub fn show(app: &mut App, ui: &egui::Ui) {
    let Some(mut editor) = app.captions.take() else { return };
    let mut close = false;
    let mut command: Option<Result<(), String>> = None;
    egui::Window::new("Captions")
        .id(egui::Id::new("ff-caption-window"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 84.0))
        // A fixed size, not an auto-sized anchored window: a contents-filling
        // scroll area inside an auto-sized window hands its own size back and
        // the window resizes every other frame, so a click would land where a
        // control was read rather than where it is.
        .fixed_size(egui::vec2(460.0, 560.0))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Captions").size(11.0).strong().color(ui::TEXT_HI));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if action(ui, "ff-caption-close", "Close", false).clicked() {
                        close = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                if action(ui, "ff-caption-add-track", "Add Track", false).clicked() {
                    command = Some(add_track(app, &mut editor));
                }
                if action(ui, "ff-caption-previous", "Prev", false).clicked() {
                    command = Some(step(app, &mut editor, false));
                }
                if action(ui, "ff-caption-next", "Next", false).clicked() {
                    command = Some(step(app, &mut editor, true));
                }
            });
            for track in editor.tracks.clone() {
                let selected = editor.track == Some(track.id);
                let label = format!("{} {}", track.label, track.name);
                let hover = format!("{} · {} captions", track.format, track.count);
                if action(ui, &format!("ff-caption-track-{}", track.id), &label, selected).on_hover_text(hover).clicked() {
                    editor.track = Some(track.id);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut editor.text).desired_width(220.0).hint_text("Caption text"));
                ui.add(egui::DragValue::new(&mut editor.seconds).speed(0.1).range(0.1..=60.0).suffix(" s"));
                if action(ui, "ff-caption-add", "Add", true).clicked() {
                    command = Some(add_caption(app, &mut editor));
                }
            });
            if let Some(error) = editor.error.clone() {
                ui.label(egui::RichText::new(error).size(10.0).color(ui::TEXT_LO));
            }
            if editor.captions.is_empty() {
                ui.label(ui::hint("No captions yet. Add a track, then a caption at the playhead."));
            } else {
                // The row handler runs after the scroll area: a click needs
                // `&mut editor`, which the row list is borrowing to draw.
                let mut clicked = None;
                egui::ScrollArea::vertical().id_salt("caption-list").max_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
                    for row in &editor.captions {
                        let width = ui.available_width();
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
                        let selected = editor.selected == Some(row.id);
                        let hit = ui.interact(rect, egui::Id::new(format!("ff-caption-row-{}", row.id)), egui::Sense::click());
                        let fill = if selected {
                            ui::ACCENT_DIM
                        } else if hit.hovered() {
                            ui::BG_HOVER
                        } else {
                            ui::BG_INSET
                        };
                        ui.painter().rect_filled(rect, 3.0, fill);
                        let label = format!("{}  {}  {}", row.in_tc, row.duration, row.text);
                        ui.painter().text(
                            egui::pos2(rect.left() + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(10.0),
                            ui::TEXT_MID,
                        );
                        if hit.clicked() {
                            clicked = Some(row.id);
                        }
                    }
                });
                if let Some(id) = clicked {
                    command = Some(select(app, &mut editor, id));
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                let enabled = editor.selected.is_some();
                ui.label(ui::hint("Start"));
                ui.add_enabled(enabled, egui::DragValue::new(&mut editor.start).speed(0.1).range(0.0..=3600.0).suffix(" s"));
                ui.label(ui::hint("Length"));
                ui.add_enabled(enabled, egui::DragValue::new(&mut editor.length).speed(0.1).range(0.1..=60.0).suffix(" s"));
                if ui.add_enabled(enabled, egui::Button::new("Apply")).clicked() {
                    command = Some(apply_text(app, &mut editor).and_then(|()| apply_times(app, &mut editor)));
                }
                if ui.add_enabled(enabled, egui::Button::new("Delete")).clicked() {
                    command = Some(delete(app, &mut editor));
                }
            });
        });
    match command {
        Some(Err(error)) => editor.error = Some(error),
        Some(Ok(())) => refresh(app, &mut editor),
        None => {}
    }
    if close {
        app.captions = None;
    } else {
        app.captions = Some(editor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use filmcraft_time::{FrameRate, Tick};
    use serde_json::json;

    /// A 24 fps sequence with one 10 s Bars & Tone pair on V1/A1.
    fn caption_app() -> App {
        let mut s = filmcraft_engine::Session::default();
        s.execute("file.newProject", json!({"name": "Captions"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        s.execute("file.newBarsAndTone", json!({"seconds": 10.0})).unwrap();
        App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test")
    }

    fn track_count(app: &App) -> usize {
        app.sequence().map_or(0, |q| q.caption_tracks.len())
    }

    fn caption_list(app: &App) -> Vec<(i64, i64, String)> {
        app.sequence()
            .map(|q| q.caption_tracks.iter().flat_map(|t| t.captions.iter().map(|c| (c.start.0, c.end().0, c.text.clone()))).collect())
            .unwrap_or_default()
    }

    fn frames(n: i64) -> i64 {
        FrameRate::FPS_24.tick_of(n).0
    }

    #[test]
    fn the_editor_lists_the_engines_own_tracks_and_captions_in_time_order() {
        let mut app = caption_app();
        let created = app.session.execute("captions.newTrack", json!({"name": "Dialogue"})).unwrap();
        let first = created["track"].as_u64().unwrap();
        let created = app.session.execute("captions.newTrack", json!({"name": "Notes"})).unwrap();
        let second = created["track"].as_u64().unwrap();
        for (track, text, seconds) in [(first, "later", 4.0), (first, "earlier", 1.0), (second, "on the other track", 2.0)] {
            app.session.execute("captions.add", json!({"track": track, "text": text, "seconds": seconds, "durationSeconds": 1.5})).unwrap();
        }
        let list = app.session.execute("captions.list", json!({})).unwrap();
        let (tracks, rows, selection) = rows_from_list(&list);
        assert_eq!(
            tracks.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
            ["C1", "C2"],
            "the engine's slot labels, top first (a new track is inserted at the top)"
        );
        assert_eq!(tracks.iter().map(|t| t.count).collect::<Vec<_>>(), [1, 2], "each track counts its own captions");
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["earlier", "on the other track", "later"], "captions read in timeline order");
        assert_eq!(rows[0].in_tc, "00:00:01:00", "the engine's own timecode at 24 fps");
        assert_eq!(rows[0].duration, "00:00:01:12", "a 1.5 s caption is 36 frames");
        assert!(selection.is_empty(), "nothing is selected yet");
    }

    #[test]
    fn adding_a_track_and_a_caption_at_the_playhead_snaps_to_frames_and_undoes() {
        let mut app = caption_app();
        app.playhead = Tick(TICKS_PER_SECOND);
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        assert!(editor.tracks.is_empty(), "the fixture sequence has no caption track");

        add_track(&mut app, &mut editor).expect("the track is added");
        assert_eq!(track_count(&app), 1, "the engine has one caption track");
        assert_eq!(editor.tracks.len(), 1, "the editor lists it");
        assert_eq!(editor.track, editor.tracks.first().map(|t| t.id), "the new track is the target");

        editor.text = "Hello world".to_string();
        editor.seconds = 2.5;
        add_caption(&mut app, &mut editor).expect("the caption is added");
        assert_eq!(
            caption_list(&app),
            vec![(frames(24), frames(24 + 60), "Hello world".to_string())],
            "the playhead start, a 2.5 s (60-frame) length and the draft text"
        );
        assert_eq!(editor.selected, editor.captions.first().map(|c| c.id), "the new caption is selected");
        assert_eq!(editor.text, "Hello world", "the drafts keep the text that was written");

        app.undo();
        assert!(caption_list(&app).is_empty(), "one undo removes the caption");
        assert_eq!(track_count(&app), 1, "the track is its own undo step and stays");
        app.undo();
        assert_eq!(track_count(&app), 0, "a second undo removes the track");
    }

    #[test]
    fn editing_the_selected_captions_text_and_times_is_one_undo_each() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        add_caption(&mut app, &mut editor).expect("the caption is added");
        let id = editor.selected.expect("the new caption is selected");

        editor.text = "A line of dialogue".to_string();
        apply_text(&mut app, &mut editor).expect("the text is written");
        assert_eq!(caption_list(&app)[0].2, "A line of dialogue", "the engine holds the new text");

        select(&mut app, &mut editor, id).expect("the caption is selected");
        editor.start = 2.0;
        editor.length = 2.5;
        apply_times(&mut app, &mut editor).expect("the times are written");
        assert_eq!(caption_list(&app), vec![(frames(48), frames(108), "A line of dialogue".to_string())], "a 2 s start and a 2.5 s (60-frame) length");

        app.undo();
        assert_eq!(caption_list(&app)[0], (0, frames(72), "A line of dialogue".to_string()), "one undo restores the 3 s default at 0");
        app.undo();
        assert_eq!(caption_list(&app)[0].2, "New caption", "a second undo restores the text");
    }

    #[test]
    fn selecting_a_row_names_it_in_the_engines_caption_selection_and_seeds_the_drafts() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        editor.text = "Second line".to_string();
        editor.seconds = 2.0;
        add_caption(&mut app, &mut editor).expect("the caption is added");
        let second = editor.selected.expect("the new caption is selected");
        editor.text = String::new();
        editor.length = 3.0;

        select(&mut app, &mut editor, second).expect("the caption is selected");
        assert_eq!(app.session.state.caption_selection.iter().map(|c| c.0).collect::<Vec<_>>(), vec![second], "the engine's caption selection names it");
        assert!(app.session.state.selection.is_empty(), "caption selection clears the clip selection, as the engine does");
        assert_eq!(editor.text, "Second line", "the text draft follows the row");
        assert!((editor.length - 2.0).abs() < 1e-9, "the length draft follows the row: {}", editor.length);
    }

    #[test]
    fn deleting_the_selected_caption_removes_it_and_undo_restores_it() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        add_caption(&mut app, &mut editor).expect("the caption is added");
        assert_eq!(caption_list(&app).len(), 1);

        delete(&mut app, &mut editor).expect("the caption is deleted");
        assert!(caption_list(&app).is_empty(), "the caption is gone");
        assert_eq!(editor.captions.len(), 0, "the list follows");
        assert_eq!(editor.selected, None, "nothing is selected");

        app.undo();
        assert_eq!(caption_list(&app).len(), 1, "one undo restores it");
    }
}
