//! The Edit workspace: the project panel (media items), the program monitor
//! (real frames through the engine) and the timeline (real clips, real
//! playhead, real engine commands). Laid out by rect — this egui generation
//! has no panel helpers.

use std::collections::HashMap;

use filmcraft_engine::Targeting;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_project::{
    Bin, BinEntry, BinId, ClipId, ItemId, ItemKind, Label, Marker, MarkerId, MarkerKind, MediaClip, MediaRef, Project, ProjectItem, Sequence, TrackId,
};
use filmcraft_time::TICKS_PER_SECOND;
use filmcraft_time::Tick;

use crate::app::{App, InspectorTab, Monitor, ProjectTab, SEQUENCE_H, SEQUENCE_RATE, SEQUENCE_W, TimelineDrag, Workspace};
use crate::pixels;
use crate::ui;

const ROW_H: f32 = 34.0;
pub(crate) const RULER_H: f32 = 18.0;
const TIMELINE_HEADER_H: f32 = 50.0;
const GENERATOR_SECONDS: f64 = 10.0;
/// The fixed track-header column: target, source-patch, visibility/mute/solo,
/// lock and the track name. The timeline body (ruler and clips) starts to its
/// right.
const TRACK_HEAD_W: f32 = 140.0;

/// The engine's `timeline.setTrack` keys for the header toggles.
const TRACK_ENABLED: &str = "enabled";
const TRACK_MUTED: &str = "muted";
const TRACK_SOLO: &str = "solo";
const TRACK_LOCKED: &str = "locked";

/// A track-header click, applied after the read-only drawing pass.
enum TrackAction {
    Target { track: TrackId, on: bool },
    Patch { track: TrackId, on: bool },
    Setting { track: TrackId, key: &'static str, on: bool },
}

/// The command a header click runs. Targeting/source patching are editor state;
/// visibility, mute, solo and lock are real sequence document state and undoable.
fn track_action_command(action: TrackAction) -> (&'static str, serde_json::Value, &'static str) {
    match action {
        TrackAction::Target { track, on } => ("timeline.setTargeting", serde_json::json!({"track": track.0, "targeted": on}), "Target"),
        TrackAction::Patch { track, on } => ("timeline.setTargeting", serde_json::json!({"track": track.0, "sourcePatch": on}), "Source patch"),
        TrackAction::Setting { track, key, on } => {
            let what = if key == TRACK_ENABLED {
                "Visibility"
            } else if key == TRACK_MUTED {
                "Mute"
            } else if key == TRACK_SOLO {
                "Solo"
            } else if key == TRACK_LOCKED {
                "Lock"
            } else {
                "Track setting"
            };
            ("timeline.setTrack", serde_json::json!({"track": track.0, key: on}), what)
        }
    }
}

/// A marker's label colour (the engine stores Premiere-style labels).
fn marker_color(label: Label) -> egui::Color32 {
    match label {
        Label::Violet => egui::Color32::from_rgb(150, 110, 220),
        Label::Iris => egui::Color32::from_rgb(122, 96, 220),
        Label::Caribbean => egui::Color32::from_rgb(46, 190, 180),
        Label::Lavender => egui::Color32::from_rgb(170, 150, 230),
        Label::Cerulean => egui::Color32::from_rgb(70, 150, 220),
        Label::Forest => egui::Color32::from_rgb(70, 160, 90),
        Label::Rose => egui::Color32::from_rgb(220, 90, 120),
        Label::Mango => egui::Color32::from_rgb(230, 150, 60),
        Label::Purple => egui::Color32::from_rgb(150, 80, 200),
        Label::Blue => egui::Color32::from_rgb(80, 120, 230),
        Label::Teal => egui::Color32::from_rgb(50, 180, 170),
        Label::Magenta => egui::Color32::from_rgb(210, 80, 190),
        Label::Tan => egui::Color32::from_rgb(200, 170, 130),
        Label::Green => egui::Color32::from_rgb(90, 200, 110),
        Label::Brown => egui::Color32::from_rgb(160, 120, 80),
        Label::Yellow => egui::Color32::from_rgb(230, 210, 70),
    }
}

/// Add a sequence marker at the playhead through `markers.add` (frame-snapped,
/// undoable).
pub(crate) fn add_marker(app: &mut App) {
    if app.sequence().is_none() {
        app.status = "No sequence".to_string();
        return;
    }
    let at = app.session.sequence_rate().snap_nearest(app.playhead.max(Tick::ZERO));
    app.playhead = at;
    match app.execute("markers.add", serde_json::json!({"time": at.0})) {
        Ok(_) => app.status = "Marker added".to_string(),
        Err(e) => app.status = e,
    }
}

/// Clear the marker at the playhead through `markers.clearCurrent` (undoable).
pub(crate) fn clear_marker(app: &mut App) {
    let has_one = app.sequence().is_some_and(|s| s.markers.iter().any(|m| m.start == app.playhead));
    if !has_one {
        app.status = "No marker at the playhead".to_string();
        return;
    }
    // The engine's clearCurrent looks at its own playhead; keep it in step.
    app.session.set_playhead(app.playhead);
    match app.execute("markers.clearCurrent", serde_json::json!({})) {
        Ok(_) => app.status = "Marker cleared".to_string(),
        Err(e) => app.status = e,
    }
}

/// Update the sequence marker at the playhead (name, comment, length and
/// colour) through a custom, undoable project edit. The engine has no
/// marker-edit command, so the shell edits the sequence's marker list by id
/// (the identity ripples keep); all four values come from the header fields.
pub(crate) fn update_marker_at_playhead(app: &mut App) {
    let name = app.marker_name.trim().to_string();
    if name.is_empty() {
        app.status = "Type a marker name first".to_string();
        return;
    }
    let comment = app.marker_comment.trim().to_string();
    let frames = app.marker_duration_frames.min(6000);
    let duration = app.session.sequence_rate().tick_of(frames as i64);
    let color = app.marker_color;
    let color_name = color.name();
    let at = app.session.sequence_rate().snap_nearest(app.playhead.max(Tick::ZERO));
    let Some(seq_id) = app.sequence_id() else {
        app.status = "No sequence".to_string();
        return;
    };
    let found = app.session.project.sequence(seq_id).and_then(|q| q.markers.iter().find(|m| m.start == at).map(|m| (m.id, m.name.clone())));
    let Some((marker_id, old_name)) = found else {
        app.status = "No marker at the playhead".to_string();
        return;
    };
    let new_name = name.clone();
    let new_comment = comment.clone();
    let result = app.edit_project("Update Marker", move |project, _| {
        if let Some(item) = project.items.get_mut(&seq_id)
            && let ItemKind::Sequence(seq) = &mut item.kind
        {
            let q = std::sync::Arc::make_mut(seq);
            if let Some(m) = q.markers.iter_mut().find(|m| m.id == marker_id) {
                m.name = new_name;
                m.comment = new_comment;
                m.duration = duration;
                m.color = color;
            }
        }
        Ok(())
    });
    match result {
        Ok(_) => {
            app.playhead = at;
            app.status = if old_name == name {
                format!("Marker \"{name}\" updated ({frames} frames, {color_name})")
            } else {
                format!("Marker renamed to \"{name}\" ({frames} frames, {color_name})")
            };
        }
        Err(e) => app.status = e.to_string(),
    }
}

/// RFC 4180 field: always quoted, with embedded quotes doubled. Free-text
/// fields (name and comment) always take this path so commas, quotes and
/// newlines in a user's marker name cannot break the columns.
fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// SMPTE-ish timecode for a marker start, at the sequence rate.
fn marker_timecode(rate: filmcraft_time::FrameRate, at: Tick) -> String {
    let total = rate.frame_at(at);
    let fps = rate.as_f64().round().max(1.0) as i64;
    format!("{:02}:{:02}:{:02}:{:02}", total / (fps * 3600), (total / (fps * 60)) % 60, (total / fps) % 60, total % fps)
}

/// The sequence markers as CSV: a header plus one row per marker, sorted as the
/// engine keeps them (by start). Ticks are exact; the timecode column is for
/// people and NLE-side checks.
pub(crate) fn markers_csv(q: &Sequence) -> String {
    let rate = q.settings.frame_rate;
    let mut out = String::from("name,comment,start_ticks,duration_ticks,start_timecode,label\n");
    for m in &q.markers {
        out.push_str(&csv_field(&m.name));
        out.push(',');
        out.push_str(&csv_field(&m.comment));
        out.push(',');
        out.push_str(&m.start.0.to_string());
        out.push(',');
        out.push_str(&m.duration.0.to_string());
        out.push(',');
        out.push_str(&marker_timecode(rate, m.start));
        out.push(',');
        out.push_str(m.color.name());
        out.push('\n');
    }
    out
}

/// One marker row read from CSV, before it lands in the sequence.
struct CsvMarker {
    name: String,
    comment: String,
    start: Tick,
    duration: Tick,
    color: Label,
}

/// Split RFC 4180 text into records. Quoted fields may hold commas, doubled
/// quotes and line breaks; blank lines are dropped. Kept separate from the
/// marker rules so the parser can be read on its own.
fn csv_records(text: &str) -> Vec<Vec<String>> {
    // A char vector rather than a peekable iterator: quotes and CRLF both need
    // one-character lookahead, and an index keeps the state machine flat.
    let chars: Vec<char> = text.chars().collect();
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut touched = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if in_quotes {
            if c == '"' {
                if chars.get(i) == Some(&'"') {
                    field.push('"');
                    i += 1;
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                in_quotes = true;
                touched = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                touched = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.get(i) == Some(&'\n') {
                    i += 1;
                }
                if touched || !field.is_empty() {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                touched = false;
            }
            _ => {
                field.push(c);
                touched = true;
            }
        }
    }
    if touched || !field.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// Read marker rows from the exported CSV format: `name, comment,
/// start_ticks, duration_ticks, start_timecode, label`, with the header
/// optional. `start_ticks` wins when both start columns are filled; the
/// timecode is parsed at the sequence rate otherwise. Texts are kept verbatim
/// (quotes are not trimmed); numbers and labels are strict, and an error names
/// the spreadsheet row so a bad file can be fixed.
fn parse_markers_csv(text: &str, rate: filmcraft_time::FrameRate) -> Result<Vec<CsvMarker>, String> {
    let records = csv_records(text);
    let has_header = records.first().and_then(|r| r.first()).is_some_and(|f| f.trim().eq_ignore_ascii_case("name"));
    let offset = usize::from(has_header);
    let rows = if has_header { &records[1..] } else { &records[..] };
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let row_no = index + offset + 1;
        let field = |n: usize| row.get(n).map(|s| s.trim()).unwrap_or("");
        let name = row.first().cloned().unwrap_or_default();
        let comment = row.get(1).cloned().unwrap_or_default();
        let start = if field(2).is_empty() {
            let timecode = field(4);
            if timecode.is_empty() {
                return Err(format!("row {row_no}: no start_ticks or start_timecode"));
            }
            let frames = filmcraft_time::parse_timecode(timecode, rate, false, 0).map_err(|e| format!("row {row_no}: bad timecode `{timecode}`: {e}"))?;
            rate.tick_of(frames)
        } else {
            let ticks: i64 = field(2).parse().map_err(|_| format!("row {row_no}: bad start_ticks `{}`", field(2)))?;
            if ticks < 0 {
                return Err(format!("row {row_no}: start_ticks is negative"));
            }
            Tick(ticks)
        };
        let duration = if field(3).is_empty() {
            Tick::ZERO
        } else {
            let ticks: i64 = field(3).parse().map_err(|_| format!("row {row_no}: bad duration_ticks `{}`", field(3)))?;
            if ticks < 0 {
                return Err(format!("row {row_no}: duration_ticks is negative"));
            }
            Tick(ticks)
        };
        let color =
            if field(5).is_empty() { Label::Green } else { Label::from_name(field(5)).ok_or_else(|| format!("row {row_no}: unknown label `{}`", field(5)))? };
        out.push(CsvMarker { name, comment, start, duration, color });
    }
    Ok(out)
}

/// Import sequence markers from the exported CSV format (RFC 4180 text).
/// Returns how many markers were added; one undoable project edit adds them all,
/// and a bad row aborts the whole import.
pub(crate) fn import_markers_csv_text(app: &mut App, text: &str) -> Result<usize, String> {
    let rate = app.session.sequence_rate();
    let parsed = parse_markers_csv(text, rate)?;
    if parsed.is_empty() {
        return Err("The CSV has no marker rows".to_string());
    }
    apply_imported_markers(app, parsed)
}

/// Import the sequence markers from a CSV chosen by the user. The parse runs
/// before the project edit, so a bad file leaves the sequence untouched.
pub(crate) fn import_markers_csv(app: &mut App) {
    let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).pick_file() else {
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            app.status = format!("Marker CSV import failed: {e}");
            return;
        }
    };
    match import_markers_csv_text(app, &text) {
        Ok(count) => app.status = format!("Imported {count} markers from {}", path.display()),
        Err(e) => app.status = format!("Marker CSV import failed: {e}"),
    }
}

/// CMX 3600 / Avid locator colour words, mapped onto the project's labels.
fn edl_marker_color(word: &str) -> Option<Label> {
    Some(match word.to_ascii_uppercase().as_str() {
        "RED" => Label::Rose,
        "GREEN" => Label::Green,
        "BLUE" => Label::Blue,
        "CYAN" => Label::Teal,
        "MAGENTA" => Label::Magenta,
        "YELLOW" => Label::Yellow,
        "WHITE" => Label::Lavender,
        "BLACK" => Label::Brown,
        _ => return None,
    })
}

/// Read the `* LOC:` locator lines of an EDL: `* LOC: <record tc> [COLOR] <name>`.
/// The record timecodes carry the hour the list starts at, so the hour of the
/// first locator is the base and the markers land relative to it. Every other
/// line (title, events, comments) is ignored, so a full cut list can be
/// imported. A bad locator aborts with its line number.
fn parse_markers_edl(text: &str, rate: filmcraft_time::FrameRate) -> Result<Vec<CsvMarker>, String> {
    let mut locators: Vec<(usize, i64, String, Label)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let Some(rest) = line.trim_start().strip_prefix('*') else { continue };
        let rest = rest.trim_start();
        if !rest.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("LOC:")) {
            continue;
        }
        let body = rest[4..].trim();
        let (timecode, tail) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        let frames = filmcraft_time::parse_timecode(timecode, rate, false, 0).map_err(|e| format!("line {line_no}: bad locator timecode `{timecode}`: {e}"))?;
        let words: Vec<&str> = tail.split_whitespace().collect();
        let (color, name) = match words.split_first() {
            Some((first, rest)) => match edl_marker_color(first) {
                Some(color) => (color, rest.join(" ")),
                None => (Label::Green, words.join(" ")),
            },
            None => (Label::Green, String::new()),
        };
        locators.push((line_no, frames, name, color));
    }
    if locators.is_empty() {
        return Err("The EDL has no `* LOC:` marker lines".to_string());
    }
    let (_, hour, _, _, _) = filmcraft_time::frames_to_fields(locators[0].1, rate, false);
    let base = filmcraft_time::fields_to_frames(hour, 0, 0, 0, rate, false);
    let mut out = Vec::with_capacity(locators.len());
    for (line_no, frames, name, color) in locators {
        let offset = frames - base;
        if offset < 0 {
            return Err(format!("line {line_no}: locator `{name}` is before the EDL's hour"));
        }
        out.push(CsvMarker { name, comment: String::new(), start: rate.tick_of(offset), duration: Tick::ZERO, color });
    }
    Ok(out)
}

/// Import sequence markers from an EDL's `* LOC:` locator lines.
pub(crate) fn import_markers_edl_text(app: &mut App, text: &str) -> Result<usize, String> {
    let rate = app.session.sequence_rate();
    let parsed = parse_markers_edl(text, rate)?;
    apply_imported_markers(app, parsed)
}

/// Add parsed markers to the active sequence as one undoable edit.
fn apply_imported_markers(app: &mut App, parsed: Vec<CsvMarker>) -> Result<usize, String> {
    let Some(seq_id) = app.sequence_id() else { return Err("No sequence".to_string()) };
    let count = parsed.len();
    let result = app.edit_project("Import Markers", move |project, _| {
        let ids: Vec<MarkerId> = (0..parsed.len()).map(|_| MarkerId(project.alloc_id())).collect();
        let item = project.items.get_mut(&seq_id).ok_or_else(|| filmcraft_engine::EngineError::Other("the sequence is gone".into()))?;
        let ItemKind::Sequence(seq) = &mut item.kind else {
            return Err(filmcraft_engine::EngineError::Other("the active item is not a sequence".into()));
        };
        let q = std::sync::Arc::make_mut(seq);
        for (id, m) in ids.into_iter().zip(parsed) {
            q.markers.push(Marker { id, start: m.start, duration: m.duration, name: m.name, comment: m.comment, kind: MarkerKind::Comment, color: m.color });
        }
        q.markers.sort_by_key(|m| m.start);
        Ok(())
    });
    match result {
        Ok(()) => {
            app.status = format!("Imported {count} markers");
            Ok(count)
        }
        Err(error) => {
            let message = error.to_string();
            app.status = message.clone();
            Err(message)
        }
    }
}

/// Import sequence markers from an EDL chosen by the user.
pub(crate) fn import_markers_edl(app: &mut App) {
    let Some(path) = rfd::FileDialog::new().add_filter("EDL", &["edl"]).pick_file() else {
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            app.status = format!("Marker EDL import failed: {e}");
            return;
        }
    };
    match import_markers_edl_text(app, &text) {
        Ok(count) => app.status = format!("Imported {count} markers from {}", path.display()),
        Err(e) => app.status = format!("Marker EDL import failed: {e}"),
    }
}

/// Write the sequence markers to `path`; returns the number of markers written.
/// Kept separate from the file dialog so the bytes can be tested.
pub(crate) fn write_markers_csv(app: &mut App, path: &std::path::Path) -> std::io::Result<usize> {
    let Some(q) = app.sequence() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no sequence"));
    };
    let count = q.markers.len();
    let csv = markers_csv(q);
    std::fs::write(path, csv)?;
    app.status = format!("Exported {count} markers to {}", path.display());
    Ok(count)
}

/// Export the sequence markers to a CSV chosen by the user. Nothing is written
/// for a sequence with no markers, so an empty export cannot look successful.
pub(crate) fn export_markers_csv(app: &mut App) {
    let Some(q) = app.sequence() else {
        app.status = "No sequence".to_string();
        return;
    };
    if q.markers.is_empty() {
        app.status = "No markers to export".to_string();
        return;
    }
    let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("markers.csv").save_file() else {
        return;
    };
    if let Err(e) = write_markers_csv(app, &path) {
        app.status = format!("Marker CSV export failed: {e}");
    }
}

/// Set the sequence (Program) In or Out point at the playhead through
/// `markers.markIn` / `markers.markOut` (frame-snapped, undoable). The Source
/// panel uses the same commands with `target: "source"` when a source clip is
/// open; this is the Program side.
pub(crate) fn program_mark(app: &mut App, is_in: bool) {
    if app.sequence().is_none() {
        app.status = "No sequence".to_string();
        return;
    }
    let at = app.session.sequence_rate().snap_nearest(app.playhead.max(Tick::ZERO));
    app.playhead = at;
    app.session.set_playhead(at);
    let command = if is_in { "markers.markIn" } else { "markers.markOut" };
    match app.execute(command, serde_json::json!({"time": at.0})) {
        Ok(_) => app.status = if is_in { "Sequence In marked".to_string() } else { "Sequence Out marked".to_string() },
        Err(e) => app.status = e,
    }
}

/// Clear the sequence (Program) In/Out points through `markers.clearInOut`
/// (undoable). Source marks on media items are untouched.
pub(crate) fn program_clear_in_out(app: &mut App) {
    match app.execute("markers.clearInOut", serde_json::json!({"target": "program"})) {
        Ok(_) => app.status = "Sequence In/Out cleared".to_string(),
        Err(e) => app.status = e,
    }
}

/// The width of a Program In/Out handle on the ruler, in points.
pub(crate) const SEQUENCE_MARK_HANDLE_W: f32 = 9.0;

/// The ruler grab targets for the sequence (Program) In and Out marks. Time
/// maps linearly from the timeline's left edge at `zoom` points per second, the
/// same mapping the ruler, the playhead and `timeline_clip_rect` use.
pub(crate) fn sequence_mark_rects(alloc: egui::Rect, mark_in: Option<Tick>, mark_out: Option<Tick>, zoom: f32) -> (Option<egui::Rect>, Option<egui::Rect>) {
    let handle = |t: Tick| {
        let x = alloc.left() + t.seconds() as f32 * zoom;
        let half = SEQUENCE_MARK_HANDLE_W / 2.0;
        egui::Rect::from_min_max(egui::pos2(x - half, alloc.top()), egui::pos2(x + half, alloc.top() + RULER_H))
    };
    (mark_in.map(handle), mark_out.map(handle))
}

/// The width of a sequence marker tab on the ruler, in points.
pub(crate) const SEQUENCE_MARKER_HANDLE_W: f32 = 7.0;

/// The ruler grab targets for the sequence markers: the ruler's top band, so a
/// marker stays grabbable over the Program handles' lower tab and the split
/// tabs. The mapping is the ruler's own.
pub(crate) fn sequence_marker_rects(alloc: egui::Rect, markers: &[(u64, Tick)], zoom: f32) -> Vec<egui::Rect> {
    let half = SEQUENCE_MARKER_HANDLE_W / 2.0;
    markers
        .iter()
        .map(|(_, t)| {
            let x = alloc.left() + t.seconds() as f32 * zoom;
            egui::Rect::from_min_max(egui::pos2(x - half, alloc.top()), egui::pos2(x + half, alloc.top() + 8.0))
        })
        .collect()
}

/// Move one sequence marker by `dx` points on the ruler. The start lands on the
/// sequence frame grid and is clamped to the sequence (zero at the left; at the
/// sequence end, or the marker's own start when that is already past the end,
/// on the right). The marker keeps its id, name, comment, length and colour,
/// and the write is a merged project edit, so a whole drag is one undo step.
pub(crate) fn drag_sequence_marker(app: &mut App, marker_id: u64, dx: f32, zoom: f32) {
    let Some(seq_id) = app.sequence_id() else { return };
    let Some(sequence) = app.session.active_sequence() else { return };
    let Some(current) = sequence.markers.iter().find(|m| m.id.0 == marker_id).map(|m| m.start) else { return };
    let rate = app.session.sequence_rate();
    let delta = Tick((dx as f64 / zoom.max(0.01) as f64 * TICKS_PER_SECOND as f64).round() as i64);
    if delta.0 == 0 {
        return;
    }
    let hi = app.sequence_end().max(current);
    let next = rate.snap_nearest(current + delta).clamp(Tick::ZERO, hi);
    if next == current {
        return;
    }
    let result = app.edit_project_merged("Move Marker", "sequence-marker-drag", |project, _| {
        let q = project.sequence_mut(seq_id).ok_or_else(|| filmcraft_engine::EngineError::Other("the sequence is gone".into()))?;
        let marker = q.markers.iter_mut().find(|m| m.id.0 == marker_id).ok_or_else(|| filmcraft_engine::EngineError::Other("the marker is gone".into()))?;
        marker.start = next;
        q.markers.sort_by_key(|m| m.start);
        Ok(())
    });
    match result {
        Ok(()) => app.status = "Marker moved".to_string(),
        Err(error) => app.status = error.to_string(),
    }
}

/// The floating timeline marker list, when one is open. Selecting a row seeks
/// to its marker; each row carries its own delete. Both are editor state; the
/// seek moves the Program playhead, and the delete is one undoable edit.
#[derive(Default)]
pub struct MarkerList {
    /// The id of the row the user selected, if any.
    pub selected: Option<u64>,
}

/// Open the floating marker list (editor state: not undoable, not dirtying).
pub(crate) fn open_marker_list(app: &mut App) {
    app.marker_list = Some(MarkerList::default());
}

/// Seek to one sequence marker and name it in the status bar. Returns whether
/// the marker still exists: the list can outlive an undo that removed it.
pub(crate) fn select_marker(app: &mut App, marker_id: u64) -> bool {
    let Some(seq_id) = app.sequence_id() else { return false };
    let Some((at, name)) = app.session.project.sequence(seq_id).and_then(|q| q.markers.iter().find(|m| m.id.0 == marker_id).map(|m| (m.start, m.name.clone())))
    else {
        return false;
    };
    app.playhead = at;
    app.session.set_playhead(at);
    let label = if name.is_empty() { format!("Marker {marker_id}") } else { format!("Marker \"{name}\"") };
    app.status = format!("{label} at {}", marker_timecode(app.session.sequence_rate(), at));
    true
}

/// Delete one sequence marker by id through a custom, undoable project edit
/// (the engine has no by-id marker delete, only clear-at-playhead and
/// clear-all). Returns whether it was deleted.
pub(crate) fn delete_marker(app: &mut App, marker_id: u64) -> bool {
    let Some(seq_id) = app.sequence_id() else { return false };
    if !app.session.project.sequence(seq_id).is_some_and(|q| q.markers.iter().any(|m| m.id.0 == marker_id)) {
        app.status = "The marker is gone".to_string();
        return false;
    }
    let result = app.edit_project("Delete Marker", move |project, _| {
        let q = project.sequence_mut(seq_id).ok_or_else(|| filmcraft_engine::EngineError::Other("the sequence is gone".into()))?;
        q.markers.retain(|m| m.id.0 != marker_id);
        Ok(())
    });
    match result {
        Ok(()) => {
            app.status = "Marker deleted".to_string();
            true
        }
        Err(error) => {
            app.status = error.to_string();
            false
        }
    }
}

/// Draw the floating marker list when one is open. Called every frame from
/// `App::ui_frame`, so the window floats over whatever workspace is showing.
/// Rows have explicit ids, so a scripted click reaches them like the relink
/// dialog's.
pub(crate) fn marker_list_window(app: &mut App, ui: &egui::Ui) {
    let Some(mut list) = app.marker_list.take() else { return };
    let markers = app.sequence().map(|q| q.markers.clone()).unwrap_or_default();
    let rate = app.session.sequence_rate();
    let mut closed = false;
    egui::Window::new("Markers")
        .id(egui::Id::new("ff-marker-list-window"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 84.0))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            // Pin the width: a scroll area that auto-shrinks horizontally would
            // otherwise take the whole screen on the window's first layout pass,
            // which an anchored window would then place off-screen.
            ui.set_width(360.0);
            ui.horizontal(|ui| {
                let count = format!("{} marker{}", markers.len(), if markers.len() == 1 { "" } else { "s" });
                ui.label(egui::RichText::new(count).size(11.0).strong().color(ui::TEXT_HI));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Close").clicked() {
                        closed = true;
                    }
                });
            });
            if markers.is_empty() {
                ui.label(ui::hint("No markers on this sequence. Add one with M."));
                return;
            }
            egui::ScrollArea::vertical().id_salt("marker-list").max_height(280.0).auto_shrink([false, false]).show(ui, |ui| {
                for marker in &markers {
                    let id = marker.id.0;
                    let width = ui.available_width();
                    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
                    let del_rect = egui::Rect::from_min_max(egui::pos2(row.right() - 22.0, row.top() + 7.0), egui::pos2(row.right(), row.bottom() - 7.0));
                    let seek = ui.interact(row, egui::Id::new(("ff-marker-list-row", id)), egui::Sense::click());
                    let del = ui.interact(del_rect, egui::Id::new(("ff-marker-list-del", id)), egui::Sense::click());
                    let selected = list.selected == Some(id);
                    let fill = if seek.hovered() || del.hovered() {
                        ui::BG_HOVER
                    } else if selected {
                        ui::ACCENT_SEL
                    } else {
                        ui::BG_INSET
                    };
                    ui.painter().rect_filled(row, 3.0, fill);
                    if selected {
                        ui::selected_bar(ui.painter(), row);
                    }
                    ui.painter().circle_filled(egui::pos2(row.left() + 12.0, row.center().y), 4.0, marker_color(marker.color));
                    let clipped =
                        ui.painter_at(egui::Rect::from_min_max(egui::pos2(row.left() + 22.0, row.top()), egui::pos2(del_rect.left() - 6.0, row.bottom())));
                    let name = if marker.name.is_empty() { "Marker".to_string() } else { marker.name.clone() };
                    clipped.text(
                        egui::pos2(row.left() + 22.0, row.center().y - 6.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{}  {name}", marker_timecode(rate, marker.start)),
                        egui::FontId::monospace(9.5),
                        ui::TEXT_MID,
                    );
                    if !marker.comment.is_empty() {
                        clipped.text(
                            egui::pos2(row.left() + 22.0, row.center().y + 7.0),
                            egui::Align2::LEFT_CENTER,
                            &marker.comment,
                            egui::FontId::proportional(9.0),
                            ui::TEXT_LO,
                        );
                    }
                    ui.painter().text(del_rect.center(), egui::Align2::CENTER_CENTER, "x", egui::FontId::proportional(10.0), ui::TEXT_LO);
                    if del.clicked() {
                        if delete_marker(app, id) && list.selected == Some(id) {
                            list.selected = None;
                        }
                    } else if seek.clicked() {
                        list.selected = Some(id);
                        select_marker(app, id);
                    }
                }
            });
        });
    if !closed {
        app.marker_list = Some(list);
    }
}

/// Drag the sequence (Program) In or Out mark by `dx` points on the ruler. The
/// mark lands on the sequence frame grid and is clamped so In never passes Out
/// (and Out never passes In, instead of the engine's `markers.markIn`/`markOut`
/// clearing the other end). The write is a merged project edit, so a whole drag
/// is one undo step; moving a mark clears its split points exactly as those
/// commands do.
pub(crate) fn drag_sequence_mark(app: &mut App, is_in: bool, dx: f32, zoom: f32) {
    let Some(seq_id) = app.sequence_id() else { return };
    let Some(sequence) = app.session.active_sequence() else { return };
    let current = if is_in { sequence.mark_in } else { sequence.mark_out };
    let Some(current) = current else { return };
    let other = if is_in { sequence.mark_out } else { sequence.mark_in };
    let end = app.sequence_end();
    let rate = app.session.sequence_rate();
    let delta = Tick((dx as f64 / zoom.max(0.01) as f64 * TICKS_PER_SECOND as f64).round() as i64);
    if delta.0 == 0 {
        return;
    }
    let snapped = rate.snap_nearest(current + delta);
    let next = if is_in { snapped.clamp(Tick::ZERO, other.unwrap_or(end)) } else { snapped.clamp(other.unwrap_or(Tick::ZERO), end) };
    if next == current {
        return;
    }
    let result = app.edit_project_merged("Mark", "sequence-range-drag", |project, _| {
        let q = project.sequence_mut(seq_id).ok_or_else(|| filmcraft_engine::EngineError::Other("the sequence is gone".into()))?;
        if is_in {
            q.mark_in = Some(next);
            q.split.video_in = None;
            q.split.audio_in = None;
        } else {
            q.mark_out = Some(next);
            q.split.video_out = None;
            q.split.audio_out = None;
        }
        Ok(())
    });
    match result {
        Ok(()) => app.status = if is_in { "Sequence In moved".to_string() } else { "Sequence Out moved".to_string() },
        Err(error) => app.status = error.to_string(),
    }
}

// ---- sequence split (video/audio) In/Out -----------------------------------

/// One of the sequence's four split points: a video-only or audio-only In/Out
/// that the engine's `markers.markSplit*` commands set, distinct from the
/// ordinary Program In/Out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SequenceSplit {
    VideoIn,
    VideoOut,
    AudioIn,
    AudioOut,
}

impl SequenceSplit {
    pub(crate) const ALL: [Self; 4] = [Self::VideoIn, Self::VideoOut, Self::AudioIn, Self::AudioOut];

    fn is_in(self) -> bool {
        matches!(self, Self::VideoIn | Self::AudioIn)
    }

    fn is_video(self) -> bool {
        matches!(self, Self::VideoIn | Self::VideoOut)
    }

    fn label(self) -> &'static str {
        match self {
            Self::VideoIn => "Video In",
            Self::VideoOut => "Video Out",
            Self::AudioIn => "Audio In",
            Self::AudioOut => "Audio Out",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::VideoIn => "V In",
            Self::VideoOut => "V Out",
            Self::AudioIn => "A In",
            Self::AudioOut => "A Out",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::VideoIn => "markers.markSplitVideoIn",
            Self::VideoOut => "markers.markSplitVideoOut",
            Self::AudioIn => "markers.markSplitAudioIn",
            Self::AudioOut => "markers.markSplitAudioOut",
        }
    }

    fn button_id(self) -> &'static str {
        match self {
            Self::VideoIn => "ff-sequence-split-video-in",
            Self::VideoOut => "ff-sequence-split-video-out",
            Self::AudioIn => "ff-sequence-split-audio-in",
            Self::AudioOut => "ff-sequence-split-audio-out",
        }
    }

    fn handle_id(self) -> &'static str {
        match self {
            Self::VideoIn => "ff-sequence-split-handle-video-in",
            Self::VideoOut => "ff-sequence-split-handle-video-out",
            Self::AudioIn => "ff-sequence-split-handle-audio-in",
            Self::AudioOut => "ff-sequence-split-handle-audio-out",
        }
    }
}

/// The width of a split-point tab on the ruler, in points.
pub(crate) const SPLIT_HANDLE_W: f32 = 8.0;

/// Read one split point from the sequence.
fn split_mark(sequence: &Sequence, side: SequenceSplit) -> Option<Tick> {
    match side {
        SequenceSplit::VideoIn => sequence.split.video_in,
        SequenceSplit::VideoOut => sequence.split.video_out,
        SequenceSplit::AudioIn => sequence.split.audio_in,
        SequenceSplit::AudioOut => sequence.split.audio_out,
    }
}

/// The ruler grab targets for the four split points. A video point owns the top
/// half of the ruler and an audio point the bottom half, so points at the same
/// frame stay separately grabbable; time maps exactly as it does for the
/// Program handles.
pub(crate) fn sequence_split_rects(
    alloc: egui::Rect,
    video_in: Option<Tick>,
    video_out: Option<Tick>,
    audio_in: Option<Tick>,
    audio_out: Option<Tick>,
    zoom: f32,
) -> [Option<egui::Rect>; 4] {
    let mid = alloc.top() + RULER_H / 2.0;
    let handle = |t: Tick, top: f32| {
        let x = alloc.left() + t.seconds() as f32 * zoom;
        let half = SPLIT_HANDLE_W / 2.0;
        egui::Rect::from_min_max(egui::pos2(x - half, top), egui::pos2(x + half, top + RULER_H / 2.0))
    };
    [video_in.map(|t| handle(t, alloc.top())), video_out.map(|t| handle(t, alloc.top())), audio_in.map(|t| handle(t, mid)), audio_out.map(|t| handle(t, mid))]
}

/// Set the sequence split point named by `side` at the playhead through the
/// engine's `markers.markSplit*` command (frame-snapped, undoable). The engine
/// clears only the same channel's opposite point when the new one crosses it,
/// which the drag below mirrors with a clamp.
pub(crate) fn program_split_mark(app: &mut App, side: SequenceSplit) {
    if app.sequence().is_none() {
        app.status = "No sequence".to_string();
        return;
    }
    let at = app.session.sequence_rate().snap_nearest(app.playhead.max(Tick::ZERO));
    app.playhead = at;
    app.session.set_playhead(at);
    match app.execute(side.command(), serde_json::json!({"time": at.0})) {
        Ok(_) => app.status = format!("Split {} marked", side.label()),
        Err(e) => app.status = e,
    }
}

/// Drag a sequence split point by `dx` points on the ruler. The point lands on
/// the sequence frame grid and stops at its same-channel effective opposite
/// (the split point of that channel, or the ordinary Program mark it inherits),
/// so a drag never clears the counterpart the way the engine's `Side::set`
/// would. The write is one merged project edit, so a whole drag is one undo
/// step.
pub(crate) fn drag_split_mark(app: &mut App, side: SequenceSplit, dx: f32, zoom: f32) {
    let Some(seq_id) = app.sequence_id() else { return };
    let Some(sequence) = app.session.active_sequence() else { return };
    let Some(current) = split_mark(sequence, side) else { return };
    let other = match side {
        SequenceSplit::VideoIn => sequence.split.video_out.or(sequence.mark_out),
        SequenceSplit::VideoOut => sequence.split.video_in.or(sequence.mark_in),
        SequenceSplit::AudioIn => sequence.split.audio_out.or(sequence.mark_out),
        SequenceSplit::AudioOut => sequence.split.audio_in.or(sequence.mark_in),
    };
    let end = app.sequence_end();
    let rate = app.session.sequence_rate();
    let delta = Tick((dx as f64 / zoom.max(0.01) as f64 * TICKS_PER_SECOND as f64).round() as i64);
    if delta.0 == 0 {
        return;
    }
    let snapped = rate.snap_nearest(current + delta);
    let next = if side.is_in() { snapped.clamp(Tick::ZERO, other.unwrap_or(end)) } else { snapped.clamp(other.unwrap_or(Tick::ZERO), end) };
    if next == current {
        return;
    }
    let result = app.edit_project_merged("Mark Split", "sequence-split-drag", |project, _| {
        let q = project.sequence_mut(seq_id).ok_or_else(|| filmcraft_engine::EngineError::Other("the sequence is gone".into()))?;
        match side {
            SequenceSplit::VideoIn => q.split.video_in = Some(next),
            SequenceSplit::VideoOut => q.split.video_out = Some(next),
            SequenceSplit::AudioIn => q.split.audio_in = Some(next),
            SequenceSplit::AudioOut => q.split.audio_out = Some(next),
        }
        Ok(())
    });
    match result {
        Ok(()) => app.status = format!("Split {} moved", side.label()),
        Err(error) => app.status = error.to_string(),
    }
}

/// Lift the sequence In/Out range through `sequence.lift`: the targeted tracks
/// lose the range and keep a gap. The marks stay set.
pub(crate) fn lift_in_out(app: &mut App) {
    match app.execute("sequence.lift", serde_json::json!({})) {
        Ok(_) => app.status = "Lifted the In/Out range (gap kept)".to_string(),
        Err(e) => app.status = e,
    }
}

/// Extract the sequence In/Out range through `sequence.extract`: targeted and
/// sync-locked tracks lose the range and close the gap. The engine clears the
/// marks and parks its playhead at the range start, which the shell follows.
pub(crate) fn extract_in_out(app: &mut App) {
    match app.execute("sequence.extract", serde_json::json!({})) {
        Ok(_) => {
            app.playhead = app.session.playhead();
            app.status = "Extracted the In/Out range (gap closed)".to_string();
        }
        Err(e) => app.status = e,
    }
}

/// Jump to the next / previous marker (editor navigation, no document edit).
pub(crate) fn go_marker(app: &mut App, forward: bool) {
    let next = app.sequence().and_then(|s| {
        if forward {
            s.markers.iter().map(|m| m.start).find(|t| *t > app.playhead)
        } else {
            s.markers.iter().rev().map(|m| m.start).find(|t| *t < app.playhead)
        }
    });
    match next {
        Some(t) => {
            app.playhead = t;
            app.status = "Marker".to_string();
        }
        None => app.status = if forward { "No later marker".to_string() } else { "No earlier marker".to_string() },
    }
}

/// Snap radius in screen pixels (converted to ticks with the zoom).
const SNAP_PX: f32 = 8.0;

/// The nearest target within `tolerance`, or the candidate unchanged.
fn snap_tick(candidate: Tick, targets: &[Tick], tolerance: Tick) -> Tick {
    let mut best = candidate;
    let mut best_distance = tolerance.0.max(0).saturating_add(1);
    for t in targets {
        let d = (t.0 - candidate.0).abs();
        if d <= tolerance.0 && d < best_distance {
            best_distance = d;
            best = *t;
        }
    }
    best
}

/// Everything a move or trim can snap to: zero, the playhead, every sequence
/// marker and every clip edge except those of the clips being dragged. A
/// linked A/V pair moves as one, so the dragged clip's other half is left out
/// too: the pair would otherwise snap straight back onto itself.
fn snap_targets(seq: &Sequence, exclude: Option<filmcraft_project::ClipId>, playhead: Tick) -> Vec<Tick> {
    let mut targets = vec![Tick::ZERO, playhead.max(Tick::ZERO)];
    targets.extend(seq.markers.iter().map(|marker| marker.start));
    let exclude_link = exclude.and_then(|id| seq.find_item(id)).and_then(|(_, item)| item.link);
    for track in seq.video_tracks.iter().chain(&seq.audio_tracks) {
        for item in &track.items {
            if Some(item.id) == exclude || exclude_link.is_some_and(|link| item.link == Some(link)) {
                continue;
            }
            targets.push(item.start);
            targets.push(item.start + item.duration);
        }
    }
    targets
}

fn snap_tolerance(zoom: f32) -> Tick {
    Tick(((SNAP_PX as f64 / zoom.max(1.0) as f64) * TICKS_PER_SECOND as f64) as i64)
}

/// Pick between the start and end candidates. A candidate that actually
/// snapped beats one that did not: an unchanged edge has zero distance, so a
/// plain nearest-distance comparison would always keep it and a move would
/// never snap.
fn choose_snap(raw: Tick, by_start: Tick, by_end: Tick) -> Tick {
    match (by_start.0 != raw.0, by_end.0 != raw.0) {
        (true, false) => by_start,
        (false, true) => by_end,
        (true, true) => {
            if (by_start.0 - raw.0).abs() <= (by_end.0 - raw.0).abs() {
                by_start
            } else {
                by_end
            }
        }
        (false, false) => raw,
    }
}

/// Snap a moved clip: whichever of its start or end is closer to a target wins.
fn snap_move(app: &App, clip: filmcraft_project::ClipId, raw: Tick, zoom: f32) -> Tick {
    if !app.session.state.snapping {
        return raw;
    }
    let Some(seq) = app.sequence() else { return raw };
    let targets = snap_targets(seq, Some(clip), app.playhead);
    let tol = snap_tolerance(zoom);
    let duration = seq.find_item(clip).map(|(_, i)| i.duration).unwrap_or_default();
    let by_start = snap_tick(raw, &targets, tol);
    let by_end = Tick(snap_tick(raw + duration, &targets, tol).0 - duration.0);
    choose_snap(raw, by_start, by_end)
}

/// Snap a trimmed edge to a nearby target (the delta is adjusted to land there).
fn snap_trim(app: &App, clip: filmcraft_project::ClipId, edge: &str, delta: Tick, zoom: f32) -> Tick {
    if !app.session.state.snapping {
        return delta;
    }
    let Some(seq) = app.sequence() else { return delta };
    let Some((_, item)) = seq.find_item(clip) else { return delta };
    let base = if edge == "in" { item.start } else { item.start + item.duration };
    let raw_edge = Tick((base.0 + delta.0).max(0));
    let targets = snap_targets(seq, Some(clip), app.playhead);
    Tick(snap_tick(raw_edge, &targets, snap_tolerance(zoom)).0 - base.0)
}

/// Where a placement will go: one time (kept A/V together), the patched
/// destination tracks and their names for the status line.
struct Placement {
    at: Tick,
    video: Option<(TrackId, String)>,
    audio: Option<(TrackId, String)>,
}

impl Placement {
    fn label(&self) -> String {
        match (&self.video, &self.audio) {
            (Some((_, v)), Some((_, a))) => format!("{v} + {a}"),
            (Some((_, v)), None) => v.clone(),
            (None, Some((_, a))) => a.clone(),
            (None, None) => "no track".to_string(),
        }
    }
}

/// The end of a patched destination track (zero when nothing is on it).
fn track_end(seq: &Sequence, id: Option<TrackId>, video: bool) -> Tick {
    let Some(id) = id else { return Tick::ZERO };
    let tracks = if video { &seq.video_tracks } else { &seq.audio_tracks };
    tracks.iter().find(|t| t.id == id).map(|t| t.items.iter().map(|i| i.start + i.duration).max().unwrap_or_default()).unwrap_or_default()
}

/// The track's label in the sequence (`V2`, `A1`).
fn track_name(seq: &Sequence, id: TrackId, video: bool) -> Option<String> {
    let (tracks, prefix) = if video { (&seq.video_tracks, "V") } else { (&seq.audio_tracks, "A") };
    tracks.iter().position(|t| t.id == id).map(|i| format!("{prefix}{}", i + 1))
}

/// Plan a placement on the engine's real source-patched destinations. A
/// picture goes to the patched video track and its sound to the patched audio
/// track; audio-only media uses the audio destination alone. Clips append to
/// the destination tracks (so patching an empty V2 lays a clip over V1 rather
/// than after it). Both streams share one start, keeping A/V in sync.
fn plan_placement(project: &Project, seq: &Sequence, media: ItemId, tg: &Targeting) -> Result<Placement, String> {
    let Some((_, clip, _)) = project.resolve_media(media) else {
        return Err("The media item is missing".to_string());
    };
    let has_video = clip.info.video.is_some();
    let has_audio = clip.info.has_audio();
    if !has_video && !has_audio {
        return Err("Nothing to place: the media has no streams".to_string());
    }
    if has_video && tg.video_dest.is_none() {
        return Err("No video destination: click S on a video track first".to_string());
    }
    if has_audio && tg.audio_dest.is_none() {
        return Err("No audio destination: click S on an audio track first".to_string());
    }
    let video = if has_video { tg.video_dest.and_then(|id| track_name(seq, id, true).map(|n| (id, n))) } else { None };
    let audio = if has_audio { tg.audio_dest.and_then(|id| track_name(seq, id, false).map(|n| (id, n))) } else { None };
    if has_video && video.is_none() {
        return Err("The patched video destination is not in this sequence: patch a video track".to_string());
    }
    if has_audio && audio.is_none() {
        return Err("The patched audio destination is not in this sequence: patch an audio track".to_string());
    }
    let v_end = if video.is_some() { track_end(seq, tg.video_dest, true) } else { Tick::ZERO };
    let a_end = if audio.is_some() { track_end(seq, tg.audio_dest, false) } else { Tick::ZERO };
    let at = if video.is_some() && audio.is_some() {
        v_end.max(a_end)
    } else if video.is_some() {
        v_end
    } else {
        a_end
    };
    Ok(Placement { at, video, audio })
}

/// The engine's `timeline.place` parameters for a plan. An audio-only clip
/// names the audio track as `track` (the engine's sound-only form); a picture
/// names `track` and, when linked, its sound's `audioTrack`.
fn placement_params(media: ItemId, sequence: ItemId, plan: &Placement) -> serde_json::Value {
    let mut params = serde_json::json!({"item": media.0, "time": plan.at.0, "sequence": sequence.0});
    match (&plan.video, &plan.audio) {
        (Some((v, _)), Some((a, _))) => {
            params["track"] = serde_json::json!(v.0);
            params["audioTrack"] = serde_json::json!(a.0);
        }
        (Some((v, _)), None) => params["track"] = serde_json::json!(v.0),
        (None, Some((a, _))) => params["track"] = serde_json::json!(a.0),
        (None, None) => {}
    }
    params
}

pub fn show(app: &mut App, ui: &mut egui::Ui, body: egui::Rect) {
    // The design's three-column edit desk: a 440 px source/media column, the
    // flexible program + timeline centre, and a 460 px inspector. The values
    // stay draggable but are clamped so the centre never disappears.
    let left_w = app.project_w.clamp(260.0, 440.0);
    let right_w = app.inspector_w.clamp(260.0, 420.0);
    let timeline_h = app.timeline_h.clamp(180.0, (body.height() - 260.0).max(180.0));

    let left = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + left_w, body.max.y));
    let inspector = egui::Rect::from_min_max(egui::pos2(body.max.x - right_w, body.min.y), body.max);
    let centre = egui::Rect::from_min_max(egui::pos2(left.max.x, body.min.y), egui::pos2(inspector.min.x, body.max.y));

    let source_h = (left.height() * 0.46).clamp(210.0, 500.0);
    let source = egui::Rect::from_min_max(left.min, egui::pos2(left.max.x, left.min.y + source_h));
    let browser = egui::Rect::from_min_max(egui::pos2(left.min.x, source.max.y), left.max);

    let program = egui::Rect::from_min_max(centre.min, egui::pos2(centre.max.x, centre.max.y - timeline_h));
    let timeline = egui::Rect::from_min_max(egui::pos2(centre.min.x, program.max.y), centre.max);

    source_monitor_panel(app, ui, source);
    project_panel(app, ui, browser);
    crate::app::gutter_v_id(ui, "edit-left-gutter", left.max.x, body.min.y, body.max.y, &mut app.project_w);

    monitor_panel(app, ui, program);
    crate::app::gutter_h_id(ui, "edit-timeline-gutter", program.max.y, centre.min.x, centre.max.x, &mut app.timeline_h);
    timeline_panel(app, ui, timeline);

    crate::app::gutter_v_id(ui, "edit-inspector-gutter", inspector.min.x, body.min.y, body.max.y, &mut app.inspector_w);
    inspector_panel(app, ui, inspector);
}

/// Generators the engine has commands for (undoable, with the registry's
/// validation and defaults).
fn add_generator(app: &mut App, command: &str) {
    match app.execute(command, serde_json::json!({"seconds": GENERATOR_SECONDS})) {
        Ok(_) => {
            // A new item can take an id an undo put back; drop previews keyed by id.
            app.previews.clear();
            app.status = "Media added".to_string();
        }
        Err(e) => app.status = e,
    }
}

/// Tone has no engine command yet: a real, undoable `Session::edit` closure —
/// the engine's own pattern for custom edits.
fn add_tone(app: &mut App) {
    let hz = 440.0_f32;
    let db = -6.0_f32;
    let bin = app.media_bin;
    let result = app.edit_project("New Tone", |project, state| {
        let source = GeneratorSource::new(Generator::Tone { hz, db }, SEQUENCE_W, SEQUENCE_H, SEQUENCE_RATE, Tick(10 * TICKS_PER_SECOND));
        let info = source.info().clone();
        let name = info.name.clone();
        let id = project.add_item(
            name.as_str(),
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::Generator(Generator::Tone { hz, db }),
                info,
                interpret: Default::default(),
                mark_in: None,
                mark_out: None,
                markers: Vec::new(),
                offline: false,
                proxy: None,
                identity: None,
            }),
            bin,
        );
        state.project_selection = vec![id];
        Ok(id)
    });
    match result {
        Ok(_) => {
            app.previews.clear();
            app.status = "Media added".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

fn add_file(app: &mut App, path: std::path::PathBuf) {
    let mut params = serde_json::json!({"paths": [path.display().to_string()]});
    if let Some(bin) = app.media_bin {
        params["bin"] = serde_json::json!(bin.0);
    }
    match app.execute("file.import", params) {
        Ok(_) => {
            app.previews.clear();
            app.status = "Imported".to_string();
        }
        Err(e) => app.status = e,
    }
}

/// Place `media` at the end of the patched destination tracks through the
/// engine's place command (undoable, with its validation). The track headers'
/// S buttons choose the destinations; double-clicking a clip's media never
/// silently picks V1/A1 behind the editor's back.
fn place_media(app: &mut App, media: ItemId) {
    let Some(seq_id) = app.sequence_id() else {
        app.status = "No sequence".to_string();
        return;
    };
    let tg = app.session.targeting();
    let Some(seq) = app.sequence() else {
        app.status = "No sequence".to_string();
        return;
    };
    let plan = match plan_placement(&app.session.project, seq, media, &tg) {
        Ok(p) => p,
        Err(e) => {
            app.status = e;
            return;
        }
    };
    let label = plan.label();
    match app.execute("timeline.place", placement_params(media, seq_id, &plan)) {
        Ok(_) => app.status = format!("Placed clip on {label}"),
        Err(e) => app.status = e,
    }
}

/// Insert or overwrite the selected bin media at the playhead on the patched
/// destination tracks. This is the engine's real `insert` mode: insert ripples
/// the existing clips right, overwrite trims or removes whatever is under it.
pub(crate) fn place_at_playhead(app: &mut App, media: ItemId, insert: bool) {
    let Some(seq_id) = app.sequence_id() else {
        app.status = "No sequence".to_string();
        return;
    };
    let at = app.playhead.max(Tick::ZERO);
    let tg = app.session.targeting();
    let plan = {
        let Some(seq) = app.session.active_sequence() else {
            app.status = "No sequence".to_string();
            return;
        };
        match plan_placement(&app.session.project, seq, media, &tg) {
            Ok(mut p) => {
                p.at = at;
                p
            }
            Err(e) => {
                app.status = e;
                return;
            }
        }
    };
    let label = plan.label();
    let mut params = placement_params(media, seq_id, &plan);
    params["insert"] = serde_json::json!(insert);
    match app.execute("timeline.place", params) {
        Ok(_) => app.status = format!("{} on {label}", if insert { "Inserted" } else { "Overwrote" }),
        Err(e) => app.status = e,
    }
}

/// The engine's Source-monitor view (`source.inspect`), or `None` when nothing
/// is open. This is the engine's own view, so the panel cannot drift from the
/// state the source commands act on.
pub(crate) fn source_inspect(app: &mut App) -> Option<serde_json::Value> {
    let value = app.session.execute("source.inspect", serde_json::json!({})).ok()?;
    value.get("item").is_some_and(|i| !i.is_null()).then_some(value)
}

fn source_rate(view: &serde_json::Value) -> filmcraft_time::FrameRate {
    filmcraft_time::FrameRate::from_f64(view["fps"].as_f64().unwrap_or(24.0))
}

/// The marked span of a source, Out inclusive, clamped to the source. One rule
/// shared by the panel, the range drag and the transport, so they cannot drift.
pub(crate) fn selected_range(start: Tick, end: Tick, mark_in: Option<Tick>, mark_out: Option<Tick>, fd: Tick) -> filmcraft_time::TimeRange {
    let a = mark_in.unwrap_or(start).clamp(start, end);
    let b = mark_out.map(|o| o + fd).unwrap_or(end).clamp(a, end);
    filmcraft_time::TimeRange::from_bounds(a, b.max(a))
}

/// The effective selected range with the marks applied (Out is inclusive).
fn source_selected(app: &mut App) -> Option<filmcraft_time::TimeRange> {
    let view = source_inspect(app)?;
    let rate = source_rate(&view);
    let start = Tick(view["start"].as_i64()?);
    let end = Tick(view["end"].as_i64()?);
    Some(selected_range(start, end, view["markIn"].as_i64().map(Tick), view["markOut"].as_i64().map(Tick), rate.frame_duration()))
}

/// The width of a Source range handle, in points.
pub(crate) const SOURCE_RANGE_HANDLE_W: f32 = 9.0;

/// The Source range bar's three grab targets, in screen coordinates. Time maps
/// linearly from `bounds` onto `bar`, so the draw loop, the drag conversion and
/// the tests all share one geometry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceRangeRects {
    pub in_handle: egui::Rect,
    pub out_handle: egui::Rect,
    /// The selected span between the handles: dragging it slides both marks.
    pub body: egui::Rect,
}

pub(crate) fn source_range_rects(bar: egui::Rect, bounds: filmcraft_time::TimeRange, range: filmcraft_time::TimeRange) -> SourceRangeRects {
    let span = bounds.duration.0.max(1) as f32;
    let x = |t: Tick| (bar.left() + ((t.0 - bounds.start.0) as f32 / span) * bar.width()).clamp(bar.left(), bar.right());
    let (a, b) = (x(range.start), x(range.end()));
    let half = SOURCE_RANGE_HANDLE_W / 2.0;
    SourceRangeRects {
        in_handle: egui::Rect::from_min_max(egui::pos2(a - half, bar.top()), egui::pos2(a + half, bar.bottom())),
        out_handle: egui::Rect::from_min_max(egui::pos2(b - half, bar.top()), egui::pos2(b + half, bar.bottom())),
        body: egui::Rect::from_min_max(egui::pos2(a + half, bar.top()), egui::pos2((b - half).max(a + half), bar.bottom())),
    }
}

/// Drag one edge (or the body) of the Source range by `dx` points on `bar`.
/// The engine's `adjust_range` owns the clamping, the frame grid and the
/// "Out is inclusive" convention; the document write is a merged project edit,
/// so a whole drag is one undo step rather than one per frame.
pub(crate) fn drag_source_range(app: &mut App, edge: filmcraft_engine::source_monitor::RangeEdge, dx: f32, bar: egui::Rect) {
    let Some(view) = source_inspect(app) else { return };
    if !view["subclip"].is_null() {
        // A subclip's range is its definition; dragging the parent's marks
        // would silently change what the subclip is.
        return;
    }
    let item = ItemId(view["item"].as_u64().unwrap_or(0));
    let rate = source_rate(&view);
    let (start, end) = (Tick(view["start"].as_i64().unwrap_or(0)), Tick(view["end"].as_i64().unwrap_or(0)));
    let bounds = filmcraft_time::TimeRange::from_bounds(start, end);
    let current = selected_range(start, end, view["markIn"].as_i64().map(Tick), view["markOut"].as_i64().map(Tick), rate.frame_duration());
    let span = bounds.duration.0.max(1) as f32;
    let delta = Tick((dx / bar.width().max(1.0) * span).round() as i64);
    if delta.0 == 0 {
        return;
    }
    let Ok(next) = filmcraft_engine::source_monitor::adjust_range(bounds, current, rate, edge, delta) else { return };
    let out = Tick(next.end().0 - rate.frame_duration().0);
    let result = app.edit_project_merged("Mark", "source-range-drag", |project, _| {
        let it = project.item_mut(item).ok_or_else(|| filmcraft_engine::EngineError::Other("the Source item is gone".into()))?;
        // The marks move as `project.setMarks` would move them: the side being
        // dragged loses its split point, and a body drag moves both sides.
        match edge {
            filmcraft_engine::source_monitor::RangeEdge::In => {
                it.split.video_in = None;
                it.split.audio_in = None;
            }
            filmcraft_engine::source_monitor::RangeEdge::Out => {
                it.split.video_out = None;
                it.split.audio_out = None;
            }
            filmcraft_engine::source_monitor::RangeEdge::Move => it.split = Default::default(),
        }
        match &mut it.kind {
            ItemKind::Media(m) => {
                m.mark_in = Some(next.start);
                m.mark_out = Some(out);
            }
            ItemKind::Sequence(q) => {
                let q = std::sync::Arc::make_mut(q);
                q.mark_in = Some(next.start);
                q.mark_out = Some(out);
            }
            _ => return Err(filmcraft_engine::EngineError::Other("this source cannot hold marks".into())),
        }
        Ok(())
    });
    if let Err(error) = result {
        app.status = error.to_string();
    }
}

/// The width of a Source marker tab on the range bar, in points.
pub(crate) const SOURCE_MARKER_HANDLE_W: f32 = 7.0;

/// The bar grab targets for the Source markers. The mapping matches
/// `source_range_rects`; the targets own just the bar's top 8 points, so the
/// range's In/Out grips keep the rest of the bar.
pub(crate) fn source_marker_rects(bar: egui::Rect, bounds: filmcraft_time::TimeRange, starts: &[Tick]) -> Vec<egui::Rect> {
    let span = bounds.duration.0.max(1) as f32;
    let half = SOURCE_MARKER_HANDLE_W / 2.0;
    starts
        .iter()
        .map(|t| {
            let x = (bar.left() + ((t.0 - bounds.start.0) as f32 / span) * bar.width()).clamp(bar.left(), bar.right());
            egui::Rect::from_min_max(egui::pos2(x - half, bar.top()), egui::pos2(x + half, bar.top() + 8.0))
        })
        .collect()
}

/// Move one Source marker by `dx` points on the range bar. The start lands on
/// the source frame grid and is clamped to the source bounds; the marker keeps
/// its id, name, comment, length and colour, and the write is a merged project
/// edit, so a whole drag is one undo step.
pub(crate) fn drag_source_marker(app: &mut App, marker_id: u64, dx: f32, bar: egui::Rect) {
    let Some(view) = source_inspect(app) else { return };
    let media = ItemId(view["media"].as_u64().unwrap_or(0));
    let rate = source_rate(&view);
    let (start, end) = (Tick(view["start"].as_i64().unwrap_or(0)), Tick(view["end"].as_i64().unwrap_or(0)));
    let Some(current) =
        view["markers"].as_array().and_then(|markers| markers.iter().find(|m| m["id"].as_u64() == Some(marker_id)).and_then(|m| m["start"].as_i64())).map(Tick)
    else {
        return;
    };
    let span = (end.0 - start.0).max(1) as f32;
    let delta = Tick((dx / bar.width().max(1.0) * span).round() as i64);
    if delta.0 == 0 {
        return;
    }
    let next = rate.snap_nearest(current + delta).clamp(start, end);
    if next == current {
        return;
    }
    let result = app.edit_project_merged("Move Source Marker", "source-marker-drag", |project, _| {
        let item = project.item_mut(media).ok_or_else(|| filmcraft_engine::EngineError::Other("the Source item is gone".into()))?;
        let list = match &mut item.kind {
            ItemKind::Media(m) => &mut m.markers,
            ItemKind::Sequence(q) => &mut std::sync::Arc::make_mut(q).markers,
            _ => return Err(filmcraft_engine::EngineError::Other("the Source item has no markers".into())),
        };
        let marker =
            list.iter_mut().find(|m| m.id == MarkerId(marker_id)).ok_or_else(|| filmcraft_engine::EngineError::Other("the Source marker is gone".into()))?;
        marker.start = next;
        list.sort_by_key(|m| m.start);
        Ok(())
    });
    match result {
        Ok(()) => app.status = "Source marker moved".to_string(),
        Err(error) => app.status = error.to_string(),
    }
}

/// Map a click on the Source range bar to a source tick and park the playhead
/// there. The bar, the drag and the paint loop share this one linear mapping.
fn seek_source(app: &mut App, bar: egui::Rect, bounds: filmcraft_time::TimeRange, pos: egui::Pos2) {
    let span = bounds.duration.0.max(1) as f32;
    let offset = (pos.x - bar.left()) / bar.width().max(1.0) * span;
    let t = Tick(bounds.start.0 + offset.round() as i64).clamp(bounds.start, bounds.end());
    source_set_playhead(app, t);
}

/// A small painted chip with an explicit id, so a scripted click reaches a
/// custom-painted control by name.
fn chip(ui: &mut egui::Ui, id: egui::Id, label: &str) -> egui::Response {
    let font = egui::FontId::proportional(10.0);
    let width = ui.painter().layout_no_wrap(label.to_string(), font.clone(), ui::TEXT_LO).size().x + 12.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), egui::Sense::hover());
    let response = ui.interact(rect, id, egui::Sense::click());
    let (fill, color) = if response.hovered() { (ui::BG_HOVER, ui::TEXT_HI) } else { (ui::BG_INSET, ui::TEXT_LO) };
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, fill);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, font, color);
    response
}

/// Open `item` in the Source monitor. Editor state: not undoable, not dirtying.
pub(crate) fn open_source(app: &mut App, item: ItemId) {
    match app.session.execute("source.open", serde_json::json!({"item": item.0})) {
        Ok(_) => {
            app.source_monitor = None;
            source_stop(app);
            app.bin_selection = Some(item);
            app.status = "Opened in the Source monitor".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

pub(crate) fn close_source(app: &mut App) {
    let _ = app.session.execute("source.closeAll", serde_json::json!({}));
    app.source_monitor = None;
    source_stop(app);
    app.status = "Source monitor closed".to_string();
}

pub(crate) fn source_set_playhead(app: &mut App, t: Tick) {
    let _ = app.session.execute("source.setPlayhead", serde_json::json!({"time": t.0}));
    if !app.source_audio {
        return;
    }
    // The audio clock is anchored at the playhead it started from; a seek moves
    // the playhead, so re-anchor there or the next frame would overwrite it.
    let selected = source_selected(app);
    if let Some(range) = selected
        && source_start_audio(app, range).is_err()
    {
        app.source_audio = false;
    }
}

/// Step the source playhead by `frames` on the source's frame grid.
pub(crate) fn source_step(app: &mut App, frames: i64) {
    let Some(view) = source_inspect(app) else { return };
    let rate = source_rate(&view);
    let start = Tick(view["start"].as_i64().unwrap_or(0));
    let end = Tick(view["end"].as_i64().unwrap_or(0));
    let last = Tick((end.0 - rate.frame_duration().0).max(start.0));
    let frame = rate.frame_at(app.session.state.source_playhead) + frames;
    source_set_playhead(app, rate.tick_of(frame).clamp(start, last));
}

pub(crate) fn source_goto(app: &mut App, to_out: bool) {
    let command = if to_out { "markers.goToOut" } else { "markers.goToIn" };
    let _ = app.session.execute(command, serde_json::json!({"target": "source"}));
}

pub(crate) fn source_mark(app: &mut App, is_in: bool) {
    let command = if is_in { "markers.markIn" } else { "markers.markOut" };
    match app.session.execute(command, serde_json::json!({"target": "source"})) {
        Ok(_) => app.status = if is_in { "Source In set".to_string() } else { "Source Out set".to_string() },
        Err(e) => app.status = e.to_string(),
    }
}

/// Clear Source marks: `markers.clearIn`, `markers.clearOut` or
/// `markers.clearInOut`.
pub(crate) fn source_clear(app: &mut App, command: &str) {
    let _ = app.session.execute(command, serde_json::json!({"target": "source"}));
    app.status = "Source marks cleared".to_string();
}

/// Add a Source marker on the Source playhead, named after its position in the
/// list so an untouched marker is still identifiable. Undoable through the
/// engine's own `markers.add`.
pub(crate) fn source_add_marker(app: &mut App) {
    let count = source_inspect(app).and_then(|v| v["markers"].as_array().map(Vec::len)).unwrap_or(0);
    match app.execute("markers.add", serde_json::json!({"target": "source", "name": format!("Marker {}", count + 1)})) {
        Ok(_) => app.status = "Source marker added".to_string(),
        Err(e) => app.status = e,
    }
}

/// Step the Source playhead to the next/previous Source marker.
pub(crate) fn source_go_marker(app: &mut App, next: bool) {
    let command = if next { "markers.goNext" } else { "markers.goPrev" };
    let _ = app.session.execute(command, serde_json::json!({"target": "source"}));
}

/// Remove the marker at `at`: park the Source playhead on it (the engine's
/// `clearCurrent` deletes at the cursor) and clear it. One undo step.
pub(crate) fn source_delete_marker(app: &mut App, at: Tick) {
    source_set_playhead(app, at);
    match app.execute("markers.clearCurrent", serde_json::json!({"target": "source"})) {
        Ok(_) => app.status = "Source marker removed".to_string(),
        Err(e) => app.status = e,
    }
}

/// Play / pause the Source monitor. When the open media has sound the shared
/// output stream drives the Source playhead (the audio clock, as in the Program
/// monitor); otherwise a wall clock stands in and the transport says it is
/// silent. Starting Source stops Program: they share one output stream.
pub(crate) fn source_toggle_play(app: &mut App) {
    if app.session.state.source_item.is_none() {
        app.status = "Open a clip in the Source monitor first".to_string();
        return;
    }
    if app.source_playing {
        source_stop(app);
        app.status = "Source stopped".to_string();
        return;
    }
    let Some(range) = source_selected(app) else { return };
    if app.session.state.source_playhead >= range.end() {
        app.session.state.source_playhead = range.start;
    }
    if app.playing {
        app.stop_playback();
    }
    app.source_playing = true;
    app.source_last = None;
    app.source_clock_start = app.session.state.source_playhead;
    match source_start_audio(app, range) {
        Ok(()) => {
            app.source_audio = true;
            app.status = "Source playing".to_string();
        }
        Err(why) => {
            app.source_audio = false;
            app.status = format!("Source playing (silent — {why})");
        }
    }
}

/// Stop the Source transport: silence the shared output stream when Source was
/// the one playing and clear both clock states.
pub(crate) fn source_stop(app: &mut App) {
    if app.source_playing || app.source_audio {
        app.player.stop();
    }
    app.source_playing = false;
    app.source_audio = false;
    app.source_last = None;
    app.source_clock_start = Tick::ZERO;
}

/// Hand the open source's own audio to the shared player for `range`. `Err`
/// names why the transport is silent (no audio stream, offline media, no
/// device); the caller keeps the wall clock instead.
fn source_start_audio(app: &mut App, range: filmcraft_time::TimeRange) -> Result<(), String> {
    let Some(view) = source_inspect(app) else { return Err("source is unavailable".to_string()) };
    let item = ItemId(view["item"].as_u64().unwrap_or(0));
    if !crate::audio::item_has_audio(&app.session.project, item) {
        return Err("this source has no audio".to_string());
    }
    let start = app.session.state.source_playhead.max(range.start);
    let end = range.end();
    if end <= start {
        return Err("the source range is empty".to_string());
    }
    let sources = app.built_sources().ok_or("the media sources are unavailable")?;
    app.player.play_item(sources, item, start, end)?;
    app.source_clock_start = start;
    Ok(())
}

/// Where the Source playhead goes next and whether it reached the end.
/// `elapsed` is the audio-clock advance (device frames as ticks) or the
/// wall-clock frame delta; the rule is the same, which is why it is pure.
fn advance_source(current: Tick, elapsed: Tick, start: Tick, end: Tick) -> (Tick, bool) {
    let next = current + elapsed;
    if next >= end { (Tick((end.0 - 1).max(start.0)), true) } else { (next, false) }
}

/// Advance source playback (called once per frame from `App::ui_frame`).
pub(crate) fn source_tick(app: &mut App) {
    if !app.source_playing {
        return;
    }
    let Some(range) = source_selected(app) else {
        source_stop(app);
        return;
    };
    // With sound the audio clock is authoritative: the playhead is the source
    // time the device is actually rendering. Without it, advance by wall time.
    let (current, elapsed) = if app.source_audio {
        (app.source_clock_start + app.player.position(), Tick::ZERO)
    } else {
        let now = std::time::Instant::now();
        let elapsed = app.source_last.map_or(std::time::Duration::ZERO, |last| now.duration_since(last));
        app.source_last = Some(now);
        let ticks = Tick::from_units(i64::try_from(elapsed.as_nanos()).unwrap_or(i64::MAX / 4).min(i64::MAX / 8), 1_000_000_000);
        (app.session.state.source_playhead, ticks)
    };
    let (next, ended) = advance_source(current, elapsed, range.start, range.end());
    app.session.state.source_playhead = next;
    if ended || (app.source_audio && app.player.is_finished()) {
        source_stop(app);
    }
}

/// Insert or overwrite the open source at the sequence playhead. This is the
/// engine's three-point edit: the source marks define the range and the
/// ripple-sequence-markers state is honored.
pub(crate) fn source_edit(app: &mut App, insert: bool) {
    if app.session.state.source_item.is_none() {
        app.status = "Open a clip in the Source monitor first".to_string();
        return;
    }
    let command = if insert { "source.insert" } else { "source.overwrite" };
    // The engine's three-point edit uses its own sequence playhead; keep it in
    // step with the timeline playhead the user sees.
    app.session.set_playhead(app.playhead);
    match app.execute(command, serde_json::json!({})) {
        Ok(_) => {
            // The three-point edit parks the engine playhead at the end of the
            // placed range; follow it so the timeline stays in step.
            app.playhead = app.session.playhead();
            app.status = if insert { "Inserted from Source".to_string() } else { "Overwrote from Source".to_string() };
        }
        Err(e) => app.status = e,
    }
}

/// The edit point next to the selected clip: its own out edge with the following
/// clip, or the previous clip's out edge with its in edge.
fn roll_pair(app: &App) -> Option<(ClipId, ClipId)> {
    let (clip, _) = app.selected_clip()?;
    let q = app.sequence()?;
    for track in q.video_tracks.iter().chain(&q.audio_tracks) {
        let Some(idx) = track.items.iter().position(|i| i.id == clip) else { continue };
        let item = &track.items[idx];
        if let Some(next) = track.items.get(idx + 1).filter(|n| n.start == item.end()) {
            return Some((clip, next.id));
        }
        if let Some(prev) = idx.checked_sub(1).map(|i| &track.items[i]).filter(|p| p.end() == item.start) {
            return Some((prev.id, clip));
        }
    }
    None
}

/// Roll the selected edit point by `frames` (one frame per step) through the
/// engine's `timeline.roll`. The clips' surrounding timeline positions and the
/// sequence duration stay fixed.
pub(crate) fn roll_selected(app: &mut App, frames: i64) {
    let Some((left, right)) = roll_pair(app) else {
        app.status = "Roll needs a selected clip with an adjacent clip".to_string();
        return;
    };
    match app.execute("timeline.roll", serde_json::json!({"left": left.0, "right": right.0, "deltaFrames": frames})) {
        Ok(value) => {
            let applied = value["delta"].as_i64().unwrap_or(0);
            let step = app.session.sequence_rate().frame_duration().0.max(1);
            app.status = format!("Rolled {} frame(s){}", applied / step, if applied == 0 { " (clamped)" } else { "" });
        }
        Err(e) => app.status = e,
    }
}

/// Slip the selected clip by `frames`: its timeline start/end and duration stay
/// put while its source In/Out move together.
pub(crate) fn slip_selected(app: &mut App, frames: i64) {
    let Some((clip, _)) = app.selected_clip() else {
        app.status = "Select a clip to slip".to_string();
        return;
    };
    match app.execute("timeline.slip", serde_json::json!({"clip": clip.0, "deltaFrames": frames})) {
        Ok(_) => app.status = format!("Slipped {frames} frame(s)"),
        Err(e) => app.status = e,
    }
}

/// Slide the selected clip by `frames`: it moves in timeline time while the
/// adjacent clips' source ranges absorb the change, keeping the sequence bounds.
pub(crate) fn slide_selected(app: &mut App, frames: i64) {
    let Some((clip, _)) = app.selected_clip() else {
        app.status = "Select a clip to slide".to_string();
        return;
    };
    match app.execute("timeline.slide", serde_json::json!({"clip": clip.0, "deltaFrames": frames})) {
        Ok(_) => app.status = format!("Slid {frames} frame(s)"),
        Err(e) => app.status = e,
    }
}

fn clip_is_video(app: &App, clip: ClipId) -> Option<bool> {
    let q = app.sequence()?;
    if q.video_tracks.iter().any(|t| t.items.iter().any(|i| i.id == clip)) {
        Some(true)
    } else if q.audio_tracks.iter().any(|t| t.items.iter().any(|i| i.id == clip)) {
        Some(false)
    } else {
        None
    }
}

fn linked_clip(app: &App, clip: ClipId, video: bool) -> Option<ClipId> {
    let q = app.sequence()?;
    let (_, item) = q.find_item(clip)?;
    let link = item.link?;
    let tracks = if video { &q.video_tracks } else { &q.audio_tracks };
    tracks.iter().flat_map(|t| &t.items).find(|i| i.id != clip && i.link == Some(link)).map(|i| i.id)
}

/// Apply a real transition at the selected clip's In edge: `sequence.applyVideoTransition`
/// (cross dissolve) or `sequence.applyAudioTransition` (constant-power crossfade).
/// With a linked pair selected, the matching half is found automatically, so
/// selecting either the picture or the sound applies the right kind.
pub(crate) fn apply_dissolve(app: &mut App, video: bool) {
    let Some((clip, _)) = app.selected_clip() else {
        app.status = if video { "Select a clip for the video cross-dissolve" } else { "Select a clip for the audio crossfade" }.to_string();
        return;
    };
    let Some(selected_video) = clip_is_video(app, clip) else {
        app.status = "The selected clip is no longer in the sequence".to_string();
        return;
    };
    let target = if selected_video == video { Some(clip) } else { linked_clip(app, clip, video) };
    let Some(target) = target else {
        app.status = if video {
            "Select a linked A/V clip or a video clip for the cross-dissolve".to_string()
        } else {
            "Select a linked A/V clip or an audio clip for the crossfade".to_string()
        };
        return;
    };
    let (command, label) = if video { ("sequence.applyVideoTransition", "Video cross-dissolve") } else { ("sequence.applyAudioTransition", "Audio crossfade") };
    let frames = app.transition_frames.clamp(1, 600);
    match app.execute(command, serde_json::json!({"clip": target.0, "edge": "in", "frames": frames})) {
        Ok(value) => {
            let id = value["transition"].as_u64().unwrap_or(0);
            app.status = format!("{label} applied at the selected edit ({frames} frames, transition {id})");
        }
        Err(e) => app.status = e,
    }
}

/// Points at the timeline's zoom as engine ticks.
fn tick_from_points(dx: f32, zoom: f32) -> Tick {
    Tick((dx as f64 / zoom.max(0.01) as f64 * TICKS_PER_SECOND as f64).round() as i64)
}

/// The span a transition drag would produce. The engine's own `drag_span`
/// decides it: the result is clamped to the two clips it joins, stays at least
/// a frame long and, between two clips, stays over their cut (`edge` is
/// `Some("in")` / `Some("out")` for an edge drag, `None` for the body, which
/// slides it over the cut). The ghost preview and the release use the same
/// function, so a drag shows exactly what it commits.
fn dragged_transition_span(app: &App, id: u64, edge: Option<&str>, dx: f32, zoom: f32) -> Option<(Tick, Tick)> {
    let q = app.sequence()?;
    let (track, transition) = q.all_tracks().find_map(|t| t.transitions.iter().find(|x| x.id.0 == id).map(|x| (t, x)))?;
    let edge = match edge {
        Some("in") => Some(filmcraft_edit::Edge::In),
        Some("out") => Some(filmcraft_edit::Edge::Out),
        _ => None,
    };
    let frame = app.session.sequence_rate().frame_duration();
    filmcraft_edit::transitions::drag_span(track, transition, edge, tick_from_points(dx, zoom), frame)
}

/// Drag a transition by `dx` points on the timeline: `sequence.setTransition`
/// with the span `dragged_transition_span` clamped, so a whole drag is one
/// Ctrl+Z and a drag the clamp leaves where it was adds no undo step at all.
pub(crate) fn drag_transition_edge(app: &mut App, id: u64, edge: Option<&str>, dx: f32, zoom: f32) {
    let Some((start, duration)) = dragged_transition_span(app, id, edge, dx, zoom) else { return };
    let unchanged =
        app.sequence().is_some_and(|q| q.all_tracks().any(|t| t.transitions.iter().any(|x| x.id.0 == id && (x.start, x.duration) == (start, duration))));
    if unchanged {
        return;
    }
    let frames = app.session.sequence_rate().frame_at(duration).max(1);
    let params = serde_json::json!({"transition": id, "start": start.0, "duration": duration.0});
    match app.execute("sequence.setTransition", params) {
        Ok(_) => app.status = format!("Transition {frames} frames"),
        Err(e) => app.status = e,
    }
}

/// Make a transition the engine's selection and select the clip it lands on, so
/// the browser's heading, Effect Controls and Clear Transition all follow the
/// one a person clicked on the track. A query: no history step, no dirtying.
pub(crate) fn select_transition(app: &mut App, id: u64) {
    let found = app.sequence().and_then(|q| {
        q.all_tracks().find_map(|t| {
            t.transitions.iter().find(|x| x.id.0 == id).map(|x| (x.to, x.effect.def().map_or_else(|| x.effect.effect.clone(), |d| d.name.to_string())))
        })
    });
    let Some((to, name)) = found else {
        app.status = "That transition is gone".to_string();
        return;
    };
    let clips: Vec<u64> = to.map(|clip| clip.0).into_iter().collect();
    match app.session.execute("timeline.select", serde_json::json!({"clips": clips, "transitions": [id]})) {
        Ok(_) => {
            app.selection = to;
            app.status = format!("Selected {name} (transition {id})");
        }
        Err(error) => app.status = error.to_string(),
    }
}

/// Set the frames and alignment of the transition at the selected clip's In
/// edge through `sequence.setTransition` (one engine edit, one undo step).
/// Applying a transition only takes frames, so alignment is chosen here.
pub(crate) fn update_transition(app: &mut App) {
    let Some((clip, _)) = app.selected_clip() else {
        app.status = "Select the incoming clip of a transition".to_string();
        return;
    };
    let Some(video) = clip_is_video(app, clip) else {
        app.status = "The selected clip is no longer in the sequence".to_string();
        return;
    };
    let frames = app.transition_frames.clamp(1, 600) as i64;
    let align = match app.transition_align {
        filmcraft_project::TransitionAlign::CenterAtCut => "center",
        filmcraft_project::TransitionAlign::StartAtCut => "start",
        filmcraft_project::TransitionAlign::EndAtCut => "end",
    };
    let found = app.sequence().and_then(|q| {
        let tracks = if video { &q.video_tracks } else { &q.audio_tracks };
        tracks
            .iter()
            .find_map(|t| t.transitions.iter().find(|x| x.to == Some(clip)).map(|x| (x.id, x.effect.def().map_or("Transition", |d| d.name).to_string())))
    });
    let Some((id, name)) = found else {
        app.status = "No transition at the selected clip's In edge (apply one first)".to_string();
        return;
    };
    match app.execute("sequence.setTransition", serde_json::json!({"transition": id.0, "frames": frames, "align": align})) {
        Ok(_) => app.status = format!("{name} at transition {} is now {frames} frames, {align} at the cut", id.0),
        Err(e) => app.status = e,
    }
}

/// Ripple delete the selected clip through `edit.rippleDelete`: remove it (the
/// engine expands to its linked A/V), then close the gap on the tracks that lost
/// material plus every sync-locked track. A locked track refuses the whole edit
/// rather than letting a linked pair drift apart.
pub(crate) fn ripple_delete_selected(app: &mut App) {
    let Some((clip, _)) = app.selected_clip() else {
        app.status = "Select a clip to ripple delete".to_string();
        return;
    };
    let locked = app.sequence().is_some_and(|q| q.video_tracks.iter().chain(&q.audio_tracks).any(|t| t.locked && t.items.iter().any(|i| i.id == clip)));
    if locked {
        app.status = "Ripple Delete refused: the clip's track is locked".to_string();
        return;
    }
    match app.execute("edit.rippleDelete", serde_json::json!({"clips": [clip.0]})) {
        Ok(_) => {
            app.selection = None;
            app.status = "Ripple deleted: gap closed on the affected and sync-locked tracks".to_string();
        }
        Err(e) => app.status = e,
    }
}

/// Remove `media` and its clips (undoable). Its composition stays keyed: an
/// undo restores the item under the same id, and the composition with it.
fn remove_media(app: &mut App, media: ItemId) {
    let result = app.edit_project("Remove Media", |project, _| {
        for seq in project.items.values_mut() {
            if let ItemKind::Sequence(s) = &mut seq.kind {
                let s = std::sync::Arc::make_mut(s);
                for track in s.video_tracks.iter_mut().chain(s.audio_tracks.iter_mut()) {
                    track.items.retain(|it| it.item != media);
                    track.sort();
                }
            }
        }
        project.root.remove_item(media);
        Ok(())
    });
    match result {
        Ok(()) => {
            if app.bin_selection == Some(media) {
                app.bin_selection = None;
            }
            app.status = "Media removed".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

fn duration_text(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as i64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Draw an audio clip's envelope: the peak columns of the whole file, walked
/// across the media span this clip shows (`source_in` for `duration * speed`
/// seconds). Symmetric around the middle, faint enough that the clip name on
/// top stays readable, and never outside the clip rect.
fn draw_waveform(painter: &egui::Painter, rect: egui::Rect, peaks: &[f32], media_seconds: f64, source_in: Tick, duration: Tick, speed: f64) {
    if peaks.is_empty() || media_seconds <= 0.0 || rect.width() < 4.0 || rect.height() < 8.0 {
        return;
    }
    let span = (duration.seconds() * speed.abs()).max(0.0).min(media_seconds);
    let from = source_in.seconds().clamp(0.0, media_seconds);
    let to = (from + span).min(media_seconds);
    if to <= from {
        return;
    }
    let mid = rect.center().y;
    let half = (rect.height() / 2.0 - 3.0).max(1.0);
    let colour = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 140);
    let step = 2.0_f32;
    let mut x = rect.left() + 2.0;
    while x < rect.right() - 2.0 {
        let fraction = f64::from((x - rect.left()) / rect.width());
        if let Some(index) = crate::previews::peak_index(fraction, from, to, media_seconds, peaks.len()) {
            let amplitude = peaks[index].clamp(0.0, 1.0);
            let height = (amplitude * half).max(0.5);
            painter.line_segment([egui::pos2(x, mid - height), egui::pos2(x, mid + height)], egui::Stroke::new(1.0, colour));
        }
        x += step;
    }
}

fn project_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("project"));
    ui::panel_header(&mut panel, &format!("Project: {}", app.session.project.name));
    let selected_tab = match app.project_tab {
        ProjectTab::Media => 0,
        ProjectTab::Markers => 1,
        ProjectTab::History => 2,
    };
    if let Some(index) = ui::tabs(&mut panel, "project-tabs", &["Media Browser", "Markers", "History"], selected_tab) {
        app.project_tab = match index {
            1 => ProjectTab::Markers,
            2 => ProjectTab::History,
            _ => ProjectTab::Media,
        };
    }
    panel.add_space(4.0);
    match app.project_tab {
        ProjectTab::Media => media_browser(app, &mut panel),
        ProjectTab::Markers => markers_browser(app, &mut panel),
        ProjectTab::History => history_browser(app, &mut panel),
    }
}

fn media_browser(app: &mut App, panel: &mut egui::Ui) {
    let bins = all_bins(&app.session.project.root);
    let mut create_bin = false;
    let mut move_to: Option<Option<BinId>> = None;
    panel.horizontal(|ui| {
        if ui.button("+ Import").on_hover_text("Import into the current bin").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Media", &["mp4", "mov", "mkv", "webm", "wav", "mp3", "flac", "png", "jpg", "jpeg", "tiff", "bmp", "tga"])
                .pick_file()
        {
            add_file(app, path);
        }
        ui.menu_button("+ New", |ui| {
            if ui.button("Bars & Tone").clicked() {
                add_generator(app, "file.newBarsAndTone");
                ui.close();
            }
            if ui.button("Color Matte").clicked() {
                add_generator(app, "file.newColorMatte");
                ui.close();
            }
            if ui.button("Tone").clicked() {
                add_tone(app);
                ui.close();
            }
        });
        if ui.add_enabled(app.bin_selection.is_some(), egui::Button::new("Open Source")).clicked()
            && let Some(id) = app.bin_selection
        {
            open_source(app, id);
            app.status = "Opened in Source monitor".to_string();
        }
        ui.menu_button("+ Bin", |ui| {
            ui.label(ui::hint("Create a bin in the current location"));
            let response = ui.add(egui::TextEdit::singleline(&mut app.new_bin_name).hint_text("Bin name").desired_width(150.0));
            let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Create").clicked() || entered {
                create_bin = true;
                ui.close();
            }
        });
        ui.add_enabled_ui(app.bin_selection.is_some(), |ui| {
            ui.menu_button("Move to", |ui| {
                if ui.button("Project (root)").clicked() {
                    move_to = Some(None);
                    ui.close();
                }
                for (id, path) in &bins {
                    if ui.button(path.as_str()).clicked() {
                        move_to = Some(Some(*id));
                        ui.close();
                    }
                }
            });
        });
    });
    if create_bin {
        new_bin(app);
    }
    if let (Some(target), Some(item)) = (move_to, app.bin_selection) {
        move_item_to_bin(app, item, target);
    }

    // The offline badge on its own line: it appears the moment a project opens
    // with files the scan cannot find, and it is the button that opens the
    // relink dialog. A painted chip with a stable id, so a scripted click
    // reaches it and the badge cannot drift from the dialog.
    if app.has_offline_media() {
        let count = app.offline_rows.len();
        panel.horizontal(|ui| {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
            let response = ui.interact(rect, egui::Id::new("ff-relink-open"), egui::Sense::click());
            let fill = if response.hovered() { ui::ACCENT } else { ui::ACCENT_DIM };
            ui.painter().rect_filled(rect, 3.0, fill);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("⚠ OFFLINE · Relink… ({count})"),
                egui::FontId::proportional(10.5),
                ui::TEXT_HI,
            );
            if response.on_hover_text("Named media files moved or are offline — reconnect them").clicked() {
                let dialog = crate::relink::RelinkDialog::open(app);
                app.relink = Some(dialog);
                app.status = format!("{count} media file(s) need relinking");
            }
        });
    }

    let width = panel.available_width();
    panel.add_sized([width, 22.0], egui::TextEdit::singleline(&mut app.media_search).hint_text("Search names, tags and metadata..."));
    panel.add_space(3.0);

    // --- where we are ------------------------------------------------------
    let trail = app.media_bin.and_then(|id| bin_trail(&app.session.project.root, id));
    if app.media_bin.is_some() && trail.is_none() {
        app.media_bin = None;
        app.status = "That bin no longer exists".to_string();
    }
    let mut go: Option<Option<BinId>> = None;
    panel.horizontal(|ui| {
        if ui.selectable_label(app.media_bin.is_none(), "Project").clicked() {
            go = Some(None);
        }
        if let Some(trail) = &trail {
            for (id, name) in trail {
                ui.label(ui::hint(">"));
                if ui.selectable_label(app.media_bin == Some(*id), name.as_str()).clicked() {
                    go = Some(Some(*id));
                }
            }
        }
    });
    if let Some(target) = go {
        app.media_bin = target;
    }

    let tags = collect_tags(&app.session.project);
    panel.horizontal(|ui| {
        ui.label(ui::meta("Tag"));
        let selected = app.media_tag_filter.clone().unwrap_or_else(|| "Any tag".to_string());
        egui::ComboBox::from_id_salt("media-tag-filter").selected_text(selected).show_ui(ui, |ui| {
            if ui.selectable_label(app.media_tag_filter.is_none(), "Any tag").clicked() {
                app.media_tag_filter = None;
            }
            for tag in &tags {
                if ui.selectable_label(app.media_tag_filter.as_deref() == Some(tag.as_str()), tag.as_str()).clicked() {
                    app.media_tag_filter = Some(tag.clone());
                }
            }
        });
        if app.media_tag_filter.is_some() && ui.small_button("Clear").clicked() {
            app.media_tag_filter = None;
        }
        if let Some(id) = app.media_bin
            && ui.small_button("Up").clicked()
        {
            app.media_bin = bin_parent(&app.session.project.root, id);
        }
    });

    // --- the selected item's tags -----------------------------------------
    if app.bin_selection != app.media_tags_for {
        app.media_tags_for = app.bin_selection;
        app.media_tags_draft =
            app.bin_selection.and_then(|id| app.session.project.items.get(&id)).and_then(|item| item.metadata.get("Tags").cloned()).unwrap_or_default();
    }
    if let Some(item) = app.bin_selection {
        let mut draft = app.media_tags_draft.clone();
        let mut apply = false;
        panel.horizontal(|ui| {
            ui.label(ui::meta("Tags"));
            ui.add(egui::TextEdit::singleline(&mut draft).hint_text("comma separated").desired_width(170.0));
            apply = ui.small_button("Apply").clicked();
        });
        app.media_tags_draft = draft.clone();
        if apply {
            set_tags(app, item, &draft);
        }
    }

    // --- rows --------------------------------------------------------------
    let needle = app.media_search.trim().to_lowercase();
    let tag_filter = app.media_tag_filter.clone();
    let filtering = !needle.is_empty() || tag_filter.is_some();
    let root = &app.session.project.root;
    let current = app.media_bin.and_then(|id| root.find_bin(id)).unwrap_or(root);
    let children: Vec<(BinId, String, usize)> = if filtering { Vec::new() } else { child_bins(current) };
    let mut ids = Vec::new();
    if filtering {
        root.all_items(&mut ids);
    } else {
        for entry in &current.children {
            if let BinEntry::Item(id) = entry {
                ids.push(*id);
            }
        }
    }
    let mut rows = Vec::new();
    for id in ids {
        let Some(item) = app.session.project.items.get(&id) else { continue };
        let ItemKind::Media(clip) = &item.kind else { continue };
        if !item_matches(item, &needle, tag_filter.as_deref()) {
            continue;
        }
        let video = clip.info.video.is_some();
        let audio = clip.info.has_audio();
        let kind = if video {
            "V"
        } else if audio {
            "A"
        } else {
            "?"
        };
        let location = if filtering { item_location(root, id) } else { String::new() };
        rows.push((
            id,
            item.name.clone(),
            kind,
            duration_text(clip.info.duration.seconds()),
            app.compositions.contains_key(&id),
            video,
            tags_of(item),
            location,
        ));
    }
    let empty = rows.is_empty() && children.is_empty();
    // Pictures are made for the rows on screen only; each one is requested once.
    let listed: Vec<ItemId> = rows.iter().map(|row| row.0).collect();
    app.request_previews(&listed);
    let mut chosen = None;
    let mut append = None;
    let mut remove = None;
    let mut enter: Option<BinId> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(panel, |ui| {
        for (id, name, count) in &children {
            let (row, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
            if ui.rect_contains_pointer(row) {
                ui.painter().rect_filled(row, 3.0, ui::BG_HOVER);
            }
            ui.painter().text(row.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, "\u{1f4c1}", egui::FontId::proportional(11.0), ui::ACCENT);
            ui.painter().text(row.left_center() + egui::vec2(26.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(11.0), ui::TEXT_MID);
            ui.painter().text(
                row.right_center() - egui::vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                format!("{count}"),
                egui::FontId::monospace(9.5),
                ui::TEXT_LO,
            );
            if response.on_hover_text("Open this bin").clicked() {
                enter = Some(*id);
            }
        }
        for (id, name, kind, duration, has_comp, video, item_tags, location) in rows {
            let selected = app.bin_selection == Some(id);
            let (card, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
            if ui.rect_contains_pointer(card) {
                ui.painter().rect_filled(card, 3.0, if selected { ui::ACCENT_SEL } else { ui::BG_HOVER });
            } else {
                ui::card(ui, card, selected);
            }
            if selected {
                ui::selected_bar(ui.painter(), card);
            }
            let icon = egui::Rect::from_min_size(card.min + egui::vec2(8.0, 7.0), egui::vec2(32.0, 22.0));
            let texture = if video { app.previews.thumbnail_texture(ui.ctx(), id) } else { None };
            match texture {
                Some(texture) => {
                    let size = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
                    let scale = (icon.width() / size.x.max(1.0)).min(icon.height() / size.y.max(1.0));
                    let draw = egui::Rect::from_center_size(icon.center(), size * scale);
                    ui.painter().image(texture.id(), draw, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                    ui.painter().rect_stroke(icon, 2.0, egui::Stroke::new(1.0, ui::LINE_DARK), egui::StrokeKind::Inside);
                }
                None => {
                    let fill = if video { ui::CLIP_VIDEO } else { ui::CLIP_AUDIO };
                    ui.painter().rect_filled(icon, 2.0, fill);
                    ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, kind, egui::FontId::proportional(10.0), ui::TEXT_HI);
                }
            }
            let text_at = egui::pos2(icon.right() + 8.0, card.center().y);
            let clipped =
                ui.painter_at(egui::Rect::from_min_max(egui::pos2(text_at.x, card.top() + 4.0), egui::pos2(card.right() - 54.0, card.bottom() - 4.0)));
            clipped.text(text_at, egui::Align2::LEFT_CENTER, &name, egui::FontId::proportional(11.0), if selected { ui::TEXT_HI } else { ui::TEXT_MID });
            if has_comp {
                ui.painter().circle_filled(egui::pos2(card.right() - 44.0, card.center().y), 3.0, ui::ACCENT);
            }
            if !item_tags.is_empty() {
                ui.painter().circle_filled(egui::pos2(card.right() - 56.0, card.center().y), 3.0, ui::CLIP_AUDIO);
            }
            ui.painter().text(egui::pos2(card.right() - 8.0, card.center().y), egui::Align2::RIGHT_CENTER, duration, egui::FontId::monospace(9.5), ui::TEXT_LO);
            let tags_text = if item_tags.is_empty() { String::new() } else { format!("\nTags: {}", item_tags.join(", ")) };
            let location_text = if location.is_empty() { String::new() } else { format!("\n{location}") };
            let failure_text = app.previews.failure(id).map(|error| format!("\nPreview: {error}")).unwrap_or_default();
            let response =
                response.on_hover_text(format!("Click to select, double-click to append, right-click to remove{tags_text}{location_text}{failure_text}"));
            if response.double_clicked() {
                append = Some(id);
            } else if response.secondary_clicked() {
                remove = Some(id);
            } else if response.clicked() {
                chosen = Some(id);
            }
        }
    });
    if let Some(id) = enter {
        app.media_bin = Some(id);
    }
    if let Some(id) = chosen {
        app.bin_selection = Some(id);
        app.status = "Selected for Insert/Overwrite".to_string();
    }
    if let Some(id) = append {
        place_media(app, id);
    }
    if let Some(id) = remove {
        remove_media(app, id);
    }
    if empty {
        panel.add_space(8.0);
        panel.label(ui::hint(if filtering {
            "No media matches this search or tag."
        } else if app.media_bin.is_some() {
            "This bin is empty. Import a file, create a bin, or move an item here."
        } else {
            "No media yet. Import a file or create bars, a matte or a tone."
        }));
    }
    panel.add_space(4.0);
    panel.label(ui::hint("Double-click appends at the patched tracks. Drag a clip or use Insert/Overwrite for a precise edit."));
}

// ---- bins, tags and search ------------------------------------------------

/// Create a bin under the bin the browser is showing.
fn new_bin(app: &mut App) {
    let name = app.new_bin_name.trim().to_string();
    if name.is_empty() {
        app.status = "Type a bin name first".to_string();
        return;
    }
    let parent = app.media_bin;
    match app.edit_project("New Bin", |project, _| Ok(project.add_bin(&name, parent))) {
        Ok(id) => {
            app.media_bin = Some(id);
            app.new_bin_name.clear();
            app.status = format!("Created the bin {name}");
        }
        Err(error) => app.status = error.to_string(),
    }
}

/// Move one item into `target` (`None` is the project root).
fn move_item_to_bin(app: &mut App, item: ItemId, target: Option<BinId>) {
    match app.edit_project("Move to Bin", |project, _| Ok(project.move_to_bin(&[item], target))) {
        Ok(Some(0)) | Ok(None) => app.status = "The item is already in that bin".to_string(),
        Ok(Some(moved)) => app.status = format!("Moved {moved} item to the bin"),
        Err(error) => app.status = error.to_string(),
    }
}

/// Replace an item's tags. An empty list removes the tag field entirely, so an
/// untagged item does not carry an empty string around.
fn set_tags(app: &mut App, item: ItemId, text: &str) {
    let tags = parse_tags(text);
    let empty = tags.is_empty();
    let joined = tags.join(", ");
    let result = app.edit_project("Edit Tags", move |project, _| {
        let Some(entry) = project.items.get_mut(&item) else { return Ok(false) };
        if tags.is_empty() {
            entry.metadata.remove("Tags");
        } else {
            entry.metadata.insert("Tags".to_string(), tags.join(", "));
        }
        Ok(true)
    });
    match result {
        Ok(true) if empty => app.status = "Cleared the tags".to_string(),
        Ok(true) => app.status = format!("Tags: {joined}"),
        Ok(false) => app.status = "That item no longer exists".to_string(),
        Err(error) => app.status = error.to_string(),
    }
}

/// Comma-separated, trimmed, empty-free, case-insensitively unique.
fn parse_tags(text: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for part in text.split(',') {
        let tag = part.trim();
        if !tag.is_empty() && !tags.iter().any(|existing| existing.eq_ignore_ascii_case(tag)) {
            tags.push(tag.to_string());
        }
    }
    tags
}

fn tags_of(item: &ProjectItem) -> Vec<String> {
    item.metadata.get("Tags").map(|value| parse_tags(value)).unwrap_or_default()
}

/// Every distinct tag in the project, sorted case-insensitively.
fn collect_tags(project: &Project) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for item in project.items.values() {
        for tag in tags_of(item) {
            if !tags.iter().any(|existing| existing.eq_ignore_ascii_case(&tag)) {
                tags.push(tag);
            }
        }
    }
    tags.sort_by_key(|tag| tag.to_lowercase());
    tags
}

/// A row is shown when the tag filter (if any) matches and the search text is in
/// the name, a tag or any other metadata value. An empty search matches all.
fn item_matches(item: &ProjectItem, needle: &str, tag: Option<&str>) -> bool {
    let tags = tags_of(item);
    if let Some(tag) = tag
        && !tags.iter().any(|existing| existing.eq_ignore_ascii_case(tag))
    {
        return false;
    }
    if needle.is_empty() {
        return true;
    }
    item.name.to_lowercase().contains(needle)
        || tags.iter().any(|tag| tag.to_lowercase().contains(needle))
        || item.metadata.values().any(|value| value.to_lowercase().contains(needle))
}

/// The bins reachable from `bin`, as `(id, path)` in tree order.
fn all_bins(root: &Bin) -> Vec<(BinId, String)> {
    fn walk(bin: &Bin, prefix: &str, out: &mut Vec<(BinId, String)>) {
        for entry in &bin.children {
            if let BinEntry::Bin(child) = entry {
                let path = if prefix.is_empty() { child.name.clone() } else { format!("{prefix} / {}", child.name) };
                out.push((child.id, path.clone()));
                walk(child, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

/// The path from the root to `target`, excluding the root itself.
fn bin_trail(root: &Bin, target: BinId) -> Option<Vec<(BinId, String)>> {
    fn walk(bin: &Bin, target: BinId, trail: &mut Vec<(BinId, String)>) -> bool {
        for entry in &bin.children {
            if let BinEntry::Bin(child) = entry {
                trail.push((child.id, child.name.clone()));
                if child.id == target || walk(child, target, trail) {
                    return true;
                }
                trail.pop();
            }
        }
        false
    }
    let mut trail = Vec::new();
    walk(root, target, &mut trail).then_some(trail)
}

fn bin_parent(root: &Bin, target: BinId) -> Option<BinId> {
    fn walk(bin: &Bin, target: BinId, root_id: BinId) -> Option<Option<BinId>> {
        for entry in &bin.children {
            if let BinEntry::Bin(child) = entry {
                if child.id == target {
                    return Some((bin.id != root_id).then_some(bin.id));
                }
                if let Some(found) = walk(child, target, root_id) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(root, target, root.id).flatten()
}

fn child_bins(bin: &Bin) -> Vec<(BinId, String, usize)> {
    bin.children
        .iter()
        .filter_map(|entry| match entry {
            BinEntry::Bin(child) => {
                let mut ids = Vec::new();
                child.all_items(&mut ids);
                Some((child.id, child.name.clone(), ids.len()))
            }
            BinEntry::Item(_) => None,
        })
        .collect()
}

/// The bin an item lives in, as a readable path ("Project / Footage").
fn item_location(root: &Bin, item: ItemId) -> String {
    fn walk(bin: &Bin, item: ItemId, label: &str, trail: &mut Vec<String>) -> Option<String> {
        trail.push(label.to_string());
        for entry in &bin.children {
            match entry {
                BinEntry::Item(id) if *id == item => return Some(trail.join(" / ")),
                BinEntry::Bin(child) => {
                    if let Some(found) = walk(child, item, &child.name, trail) {
                        return Some(found);
                    }
                }
                BinEntry::Item(_) => {}
            }
        }
        trail.pop();
        None
    }
    let mut trail = Vec::new();
    walk(root, item, "Project", &mut trail).unwrap_or_else(|| "Project".to_string())
}

fn markers_browser(app: &mut App, panel: &mut egui::Ui) {
    let markers: Vec<(Tick, String, f64, Label)> =
        app.sequence().map(|seq| seq.markers.iter().map(|m| (m.start, m.name.clone(), m.duration.seconds(), m.color)).collect()).unwrap_or_default();
    if markers.is_empty() {
        panel.add_space(8.0);
        panel.label(ui::hint("No sequence markers yet. Press M at the playhead or use Add Marker."));
    }
    let mut jump = None;
    egui::ScrollArea::vertical().max_height(220.0).show(panel, |ui| {
        for (start, name, duration, color) in &markers {
            let (row, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 2.0, ui::BG_HOVER);
            }
            ui.painter().circle_filled(row.left_center() + egui::vec2(10.0, 0.0), 4.0, marker_color(*color));
            ui.painter().text(row.left_center() + egui::vec2(22.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(11.0), ui::TEXT_MID);
            ui.painter().text(
                row.right_center() - egui::vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                marker_timecode(SEQUENCE_RATE, *start),
                egui::FontId::monospace(9.5),
                ui::TEXT_LO,
            );
            if response.clicked() {
                jump = Some(*start);
            }
            if *duration > 0.0 {
                ui.painter().text(
                    row.right_center() - egui::vec2(58.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{duration:.1}s"),
                    egui::FontId::monospace(9.0),
                    ui::TEXT_OFF,
                );
            }
        }
    });
    if let Some(start) = jump {
        app.playhead = start;
        app.status = "Moved to marker".to_string();
    }
    ui::spaced_rule(panel);
    panel.label(egui::RichText::new("Marker at playhead").size(10.5).strong().color(ui::TEXT_LO));
    panel.add_sized([panel.available_width(), 22.0], egui::TextEdit::singleline(&mut app.marker_name).hint_text("Name"));
    panel.add_sized([panel.available_width(), 22.0], egui::TextEdit::singleline(&mut app.marker_comment).hint_text("Comment"));
    panel.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut app.marker_duration_frames).range(0..=6000).suffix(" fr"));
        egui::ComboBox::from_id_salt("marker-color-browser").selected_text(app.marker_color.name()).show_ui(ui, |ui| {
            for color in Label::ALL {
                ui.selectable_value(&mut app.marker_color, color, color.name());
            }
        });
    });
    if ui::full_button(panel, "Update Marker").clicked() {
        update_marker_at_playhead(app);
    }
    if ui::full_button(panel, "Add Marker at Playhead").clicked() {
        add_marker(app);
    }
    if ui::full_button(panel, "Export Markers as CSV...").clicked() {
        export_markers_csv(app);
    }
}

fn history_browser(app: &mut App, panel: &mut egui::Ui) {
    let undo = app.session.history.undo.len();
    let redo = app.session.history.redo.len();
    panel.add_space(6.0);
    ui::kv(panel, "Undo steps", undo.to_string());
    ui::kv(panel, "Redo steps", redo.to_string());
    ui::kv(panel, "Revision", app.revision.to_string());
    ui::kv(panel, "Document", if app.dirty { "Modified" } else { "Saved" });
    panel.add_space(6.0);
    panel.horizontal(|ui| {
        if ui.add_enabled(app.can_undo(), egui::Button::new("Undo")).clicked() {
            app.undo();
        }
        if ui.add_enabled(app.can_redo(), egui::Button::new("Redo")).clicked() {
            app.redo();
        }
    });
    panel.add_space(8.0);
    panel.label(ui::hint("The editing project and its compositing compositions share one chronological undo order."));
    panel.add_space(4.0);
    panel.label(egui::RichText::new(&app.status).size(10.5).color(ui::TEXT_LO));
}

fn timeline_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    let mut header = ui
        .new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), TIMELINE_HEADER_H))).id_salt("timeline-head"));
    header.add_space(3.0);
    header.horizontal(|ui| {
        ui.label(egui::RichText::new("Timeline").size(11.0).strong().color(ui::TEXT_HI));
        ui.label(egui::RichText::new(app.sequence_name().unwrap_or("No sequence")).size(10.0).color(ui::TEXT_LO));
        if ui.button("Razor").on_hover_text("Add an edit on every track at the playhead (Ctrl+K)").clicked() {
            app.add_edit_all_tracks();
        }
        let has_bin_item = app.bin_selection.is_some();
        if ui
            .add_enabled(has_bin_item, egui::Button::new("Insert"))
            .on_hover_text("Insert the selected bin item at the playhead (ripples existing clips right)")
            .clicked()
            && let Some(item) = app.bin_selection
        {
            place_at_playhead(app, item, true);
        }
        if ui.add_enabled(has_bin_item, egui::Button::new("Overwrite")).on_hover_text("Overwrite the selected bin item at the playhead").clicked()
            && let Some(item) = app.bin_selection
        {
            place_at_playhead(app, item, false);
        }

        ui.menu_button("Markers", |ui| {
            if ui.button("Marker List...").clicked() {
                open_marker_list(app);
                ui.close();
            }
            ui.separator();
            if ui.button("Add Marker (M)").clicked() {
                add_marker(app);
                ui.close();
            }
            if ui.button("Previous Marker").clicked() {
                go_marker(app, false);
                ui.close();
            }
            if ui.button("Next Marker").clicked() {
                go_marker(app, true);
                ui.close();
            }
            if ui.button("Clear Marker").clicked() {
                clear_marker(app);
                ui.close();
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut app.marker_name).desired_width(140.0).hint_text("Marker name"));
            ui.add(egui::TextEdit::singleline(&mut app.marker_comment).desired_width(190.0).hint_text("Comment"));
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut app.marker_duration_frames).range(0..=6000).suffix(" fr"));
                egui::ComboBox::from_id_salt("marker-color").selected_text(app.marker_color.name()).show_ui(ui, |ui| {
                    for color in Label::ALL {
                        ui.selectable_value(&mut app.marker_color, color, color.name());
                    }
                });
            });
            if ui.button("Update Marker").clicked() {
                update_marker_at_playhead(app);
                ui.close();
            }
            if ui.add_enabled(app.sequence().is_some(), egui::Button::new("Import Markers CSV...")).clicked() {
                import_markers_csv(app);
                ui.close();
            }
            if ui.add_enabled(app.sequence().is_some(), egui::Button::new("Import Markers EDL...")).clicked() {
                import_markers_edl(app);
                ui.close();
            }
            if ui.add_enabled(app.sequence().is_some_and(|q| !q.markers.is_empty()), egui::Button::new("Export Markers as CSV...")).clicked() {
                export_markers_csv(app);
                ui.close();
            }
        });

        if chip(ui, egui::Id::new("ff-marker-list-open"), "Marker List").on_hover_text("Select, seek to and delete timeline markers").clicked() {
            open_marker_list(app);
        }

        if app.selection.is_some()
            && chip(ui, egui::Id::new("ff-speed-open"), "Speed").on_hover_text("Speed, reverse and freeze frame for the selected clip").clicked()
        {
            crate::speed::open(app);
        }

        if app.selection.is_some()
            && chip(ui, egui::Id::new("ff-transition-open"), "Transitions")
                .on_hover_text("Browse video and audio transitions and apply one at the selected edit")
                .clicked()
        {
            crate::transitions::open(app);
        }

        if chip(ui, egui::Id::new("ff-caption-open"), "Captions").on_hover_text("List, add and edit the sequence's captions").clicked() {
            crate::captions::open(app);
        }

        let marked = app.sequence().is_some_and(|q| q.mark_in.is_some() || q.mark_out.is_some());
        ui.menu_button("In / Out", |ui| {
            if ui.button("Mark In").clicked() {
                program_mark(app, true);
                ui.close();
            }
            if ui.button("Mark Out").clicked() {
                program_mark(app, false);
                ui.close();
            }
            if ui.add_enabled(marked, egui::Button::new("Clear In/Out")).clicked() {
                program_clear_in_out(app);
                ui.close();
            }
            ui.separator();
            if ui.add_enabled(marked, egui::Button::new("Lift (;)")).clicked() {
                lift_in_out(app);
                ui.close();
            }
            if ui.add_enabled(marked, egui::Button::new("Extract (')")).clicked() {
                extract_in_out(app);
                ui.close();
            }
        });

        let has_clip = app.selection.is_some();
        let has_pair = roll_pair(app).is_some();
        ui.menu_button("Trim", |ui| {
            if ui.add_enabled(has_pair, egui::Button::new("Roll -1")).clicked() {
                roll_selected(app, -1);
                ui.close();
            }
            if ui.add_enabled(has_pair, egui::Button::new("Roll +1")).clicked() {
                roll_selected(app, 1);
                ui.close();
            }
            ui.separator();
            if ui.add_enabled(has_clip, egui::Button::new("Slip -1")).clicked() {
                slip_selected(app, -1);
                ui.close();
            }
            if ui.add_enabled(has_clip, egui::Button::new("Slip +1")).clicked() {
                slip_selected(app, 1);
                ui.close();
            }
            if ui.add_enabled(has_clip, egui::Button::new("Slide -1")).clicked() {
                slide_selected(app, -1);
                ui.close();
            }
            if ui.add_enabled(has_clip, egui::Button::new("Slide +1")).clicked() {
                slide_selected(app, 1);
                ui.close();
            }
            ui.separator();
            ui.label("Transition");
            ui.add(egui::DragValue::new(&mut app.transition_frames).range(1..=600).suffix(" fr"));
            let align_label = match app.transition_align {
                filmcraft_project::TransitionAlign::CenterAtCut => "Center",
                filmcraft_project::TransitionAlign::StartAtCut => "Start",
                filmcraft_project::TransitionAlign::EndAtCut => "End",
            };
            egui::ComboBox::from_id_salt("transition-align").selected_text(align_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut app.transition_align, filmcraft_project::TransitionAlign::CenterAtCut, "Center at cut");
                ui.selectable_value(&mut app.transition_align, filmcraft_project::TransitionAlign::StartAtCut, "Start at cut");
                ui.selectable_value(&mut app.transition_align, filmcraft_project::TransitionAlign::EndAtCut, "End at cut");
            });
            if ui.add_enabled(has_clip, egui::Button::new("Apply Video Dissolve")).clicked() {
                apply_dissolve(app, true);
                ui.close();
            }
            if ui.add_enabled(has_clip, egui::Button::new("Apply Audio Crossfade")).clicked() {
                apply_dissolve(app, false);
                ui.close();
            }
            if ui.add_enabled(has_clip, egui::Button::new("Update Transition")).clicked() {
                update_transition(app);
                ui.close();
            }
        });

        if ui
            .add_enabled(app.selection.is_some(), egui::Button::new("Ripple Delete"))
            .on_hover_text("Remove the selected clip and close the gap (Shift+Delete)")
            .clicked()
        {
            ripple_delete_selected(app);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Slider::new(&mut app.zoom, 20.0..=400.0).show_value(false));
            ui.label(egui::RichText::new("Zoom").size(10.0).color(ui::TEXT_LO));
        });
    });
    header.horizontal(|ui| {
        let mut snapping = app.session.state.snapping;
        if ui.checkbox(&mut snapping, "Snap").changed() {
            match app.session.execute("sequence.snap", serde_json::json!({"on": snapping})) {
                Ok(_) => app.status = if snapping { "Snapping on".to_string() } else { "Snapping off".to_string() },
                Err(e) => app.status = e.to_string(),
            }
        }
        let mut ripple = app.session.state.ripple_sequence_markers;
        if ui.checkbox(&mut ripple, "Ripple Markers").changed() {
            match app.session.execute("markers.rippleSequenceMarkers", serde_json::json!({"on": ripple})) {
                Ok(_) => app.status = if ripple { "Ripple markers on".to_string() } else { "Ripple markers off".to_string() },
                Err(e) => app.status = e.to_string(),
            }
        }
        ui.separator();
        // The four split points, at the sequence playhead. Video and audio are
        // marked separately, exactly like the engine's Mark Split submenu.
        for side in SequenceSplit::ALL {
            let response =
                chip(ui, egui::Id::new(side.button_id()), side.short()).on_hover_text(format!("Set the {} split point at the sequence playhead", side.label()));
            if response.clicked() {
                program_split_mark(app, side);
            }
        }
        ui.separator();
        ui.label(
            egui::RichText::new("T/S target/patch · V/M/L track states · drag clips to move · drag edges to trim · Ctrl+Z undo").size(9.5).color(ui::TEXT_OFF),
        );
    });

    // Envelopes are read from the bounded preview cache before the sequence is
    // borrowed for drawing: one peak column list per audio item, cloned so the
    // painter stays independent of the app.
    let audio_items: Vec<ItemId> =
        app.sequence().map(|seq| seq.audio_tracks.iter().flat_map(|track| track.items.iter().map(|item| item.item)).collect()).unwrap_or_default();
    app.request_previews(&audio_items);
    let waves: HashMap<ItemId, (Vec<f32>, f64)> =
        audio_items.iter().filter_map(|id| app.previews.waveform(*id).map(|wave| (*id, (wave.peaks.clone(), wave.media_seconds)))).collect();

    let Some(s) = app.sequence() else {
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(rect.min + egui::vec2(0.0, TIMELINE_HEADER_H), rect.max)));
        body.label("No sequence");
        return;
    };
    let rows = s.video_tracks.len() + s.audio_tracks.len();
    let strip = egui::Rect::from_min_max(rect.min + egui::vec2(0.0, TIMELINE_HEADER_H), rect.max);
    let head_w = TRACK_HEAD_W.min((strip.width() * 0.4).max(0.0));
    let head = egui::Rect::from_min_max(strip.min, egui::pos2(strip.min.x + head_w, strip.max.y));
    let body = egui::Rect::from_min_max(egui::pos2(strip.min.x + head_w, strip.min.y), strip.max);
    let width = ((app.sequence_end().seconds() + 2.0) * app.zoom as f64).max(body.width() as f64) as f32;
    let desired = egui::vec2(width, RULER_H + rows as f32 * ROW_H);
    let (alloc, response) = {
        let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("timeline-body"));
        // Allocate the scroll extent, then register the same rect under a
        // stable id with the drag sense. Tests need to know where the timeline
        // body is, and egui's auto ids are not addressable from outside.
        let (alloc, _) = body_ui.allocate_exact_size(desired, egui::Sense::hover());
        let response = body_ui.interact(alloc, egui::Id::new("ff-timeline-body"), egui::Sense::click_and_drag());
        (alloc, response)
    };
    let painter = ui.painter_at(alloc);
    let header_painter = ui.painter_at(strip);
    let zoom = app.zoom;
    let to_x = |t: Tick| alloc.left() + t.seconds() as f32 * zoom;

    painter.rect_filled(alloc, 0.0, ui::BG_INSET);
    let seconds = (alloc.width() / app.zoom).ceil() as i64;
    for tick in 0..=seconds {
        let x = alloc.left() + tick as f32 * app.zoom;
        painter.line_segment([egui::pos2(x, alloc.top()), egui::pos2(x, alloc.top() + RULER_H)], egui::Stroke::new(1.0, ui::LINE_DARK));
        if tick % 5 == 0 {
            painter.text(egui::pos2(x + 3.0, alloc.top() + 2.0), egui::Align2::LEFT_TOP, format!("{tick}s"), egui::FontId::monospace(10.0), ui::TEXT_LO);
        }
    }

    // The Program In/Out range: a faint band across the ruler between the
    // marks, then the two grab handles painted over the markers below.
    let (in_rect, out_rect) = sequence_mark_rects(alloc, s.mark_in, s.mark_out, zoom);
    if let (Some(a), Some(b)) = (s.mark_in, s.mark_out) {
        let (x1, x2) = (to_x(a), to_x(b));
        if x2 > x1 {
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x1, alloc.top() + 1.0), egui::pos2(x2, alloc.top() + RULER_H - 1.0)),
                0.0,
                egui::Color32::from_rgba_unmultiplied(53, 194, 181, 40),
            );
        }
    }

    // Sequence markers live in the ruler: a coloured stem with a round head.
    let mut marker_hits: Vec<(f32, Tick, String)> = Vec::new();
    for m in &s.markers {
        let x = to_x(m.start);
        if x < alloc.left() - 6.0 || x > alloc.right() + 6.0 {
            continue;
        }
        let color = marker_color(m.color);
        painter.line_segment([egui::pos2(x, alloc.top() + 3.0), egui::pos2(x, alloc.top() + RULER_H)], egui::Stroke::new(2.0, color));
        painter.circle_filled(egui::pos2(x, alloc.top() + 3.0), 3.0, color);
        marker_hits.push((x, m.start, if m.name.is_empty() { format!("marker {}", m.id.0) } else { m.name.clone() }));
    }

    // The marker grab targets: the ruler's top band, shared by the paint above
    // and the drag below.
    let marker_starts: Vec<(u64, Tick)> = s.markers.iter().map(|m| (m.id.0, m.start)).collect();
    let marker_rects = sequence_marker_rects(alloc, &marker_starts, zoom);

    // Handles last, so a marker at the same frame cannot hide one. In is the
    // accent colour; Out is amber, far enough from the red playhead.
    for (rect, color) in [(in_rect, ui::ACCENT), (out_rect, egui::Color32::from_rgb(214, 155, 68))] {
        let Some(rect) = rect else { continue };
        let x = rect.center().x;
        painter.line_segment([egui::pos2(x, alloc.top() + 1.0), egui::pos2(x, alloc.top() + RULER_H)], egui::Stroke::new(2.0, color));
        painter.rect_filled(egui::Rect::from_center_size(egui::pos2(x, alloc.top() + 4.0), egui::vec2(SEQUENCE_MARK_HANDLE_W, 7.0)), 1.0, color);
    }

    // Split points paint as channel-coloured tabs: video in the ruler's top
    // half, audio in the bottom half, so both stay legible and grabbable at the
    // same frame. They go last, over the Program handles.
    let split_rects = sequence_split_rects(alloc, s.split.video_in, s.split.video_out, s.split.audio_in, s.split.audio_out, zoom);
    for (rect, side) in split_rects.iter().zip(SequenceSplit::ALL) {
        let Some(rect) = rect else { continue };
        let color = if side.is_video() { ui::CLIP_VIDEO_HI } else { ui::CLIP_AUDIO_HI };
        let x = rect.center().x;
        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(2.0, color));
        painter.rect_filled(egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(SPLIT_HANDLE_W, 6.0)), 1.0, color);
    }

    let mut hit_rects: Vec<(egui::Rect, filmcraft_project::ClipId)> = Vec::new();
    // A transition's painted band over the cut, with the engine id the commands
    // take. Transitions are hit before clips: they sit on top of what they
    // join, exactly where a person looks for them.
    let mut transition_rects: Vec<(egui::Rect, u64)> = Vec::new();
    let selected_transitions: Vec<u64> = app.session.state.transition_selection.iter().map(|t| t.0).collect();
    let mut actions: Vec<TrackAction> = Vec::new();
    let tg = app.session.targeting();
    // Corner above the headers, then one header per track. The target square
    // (T) selects the engine's targeted tracks (navigation/paste, mute/solo);
    // the source square (S) patches where new media lands for that stream.
    header_painter.rect_filled(egui::Rect::from_min_max(strip.min, egui::pos2(head.right(), strip.top() + RULER_H)), 0.0, ui::BG_INSET);
    header_painter.text(
        egui::pos2(head.left() + 6.0, strip.top() + RULER_H / 2.0),
        egui::Align2::LEFT_CENTER,
        "Tracks",
        egui::FontId::proportional(10.0),
        ui::TEXT_LO,
    );
    for (row, track) in s.video_tracks.iter().chain(&s.audio_tracks).enumerate() {
        let y = alloc.top() + RULER_H + row as f32 * ROW_H;
        let is_video = row < s.video_tracks.len();
        let row_rect = egui::Rect::from_min_max(egui::pos2(alloc.left(), y), egui::pos2(alloc.right(), y + ROW_H));
        painter.rect_filled(row_rect, 0.0, if row % 2 == 0 { ui::BG_PANEL } else { ui::BG_INSET });
        let name = if is_video { format!("V{}", row + 1) } else { format!("A{}", row + 1 - s.video_tracks.len()) };
        let head_rect = egui::Rect::from_min_size(egui::pos2(head.left(), y), egui::vec2(head.width(), ROW_H));
        header_painter.rect_filled(head_rect, 0.0, ui::BG_HEADER);
        header_painter.line_segment(
            [egui::pos2(head.right(), y), egui::pos2(head.right(), y + ROW_H)],
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        let targeted = tg.targeted.contains(&track.id);
        let patched = if is_video { tg.video_dest == Some(track.id) } else { tg.audio_dest == Some(track.id) };
        let button_rect = |x: f32| egui::Rect::from_min_size(egui::pos2(head_rect.left() + x, y + ROW_H / 2.0 - 7.0), egui::vec2(15.0, 15.0));
        let target_rect = button_rect(6.0);
        let patch_rect = button_rect(25.0);
        let on = ui::ACCENT_SEL;
        let off = ui::BG_CONTROL;
        header_painter.rect_filled(target_rect, 2.0, if targeted { on } else { off });
        header_painter.text(target_rect.center(), egui::Align2::CENTER_CENTER, "T", egui::FontId::monospace(9.0), egui::Color32::WHITE);
        header_painter.rect_filled(patch_rect, 2.0, if patched { egui::Color32::from_rgb(176, 112, 36) } else { off });
        header_painter.text(patch_rect.center(), egui::Align2::CENTER_CENTER, "S", egui::FontId::monospace(9.0), egui::Color32::WHITE);
        let target_hint = if targeted { "Stop targeting this track" } else { "Target this track (navigation, paste, mute/solo)" };
        if ui.interact(target_rect, egui::Id::new(("ff-track-target", track.id.0)), egui::Sense::click()).on_hover_text(target_hint).clicked() {
            actions.push(TrackAction::Target { track: track.id, on: !targeted });
        }
        let patch_hint = if patched { "Unpatch this track (new media will not land here)" } else { "Patch new media to this track" };
        if ui.interact(patch_rect, egui::Id::new(("ff-track-patch", track.id.0)), egui::Sense::click()).on_hover_text(patch_hint).clicked() {
            actions.push(TrackAction::Patch { track: track.id, on: !patched });
        }

        // Visibility / mute, solo and lock: real `timeline.setTrack` document
        // state (undoable, dirtying), not editor-only targeting state.
        if is_video {
            let visible = track.enabled;
            let visibility_rect = button_rect(44.0);
            header_painter.rect_filled(visibility_rect, 2.0, if visible { egui::Color32::from_rgb(64, 128, 80) } else { off });
            header_painter.text(visibility_rect.center(), egui::Align2::CENTER_CENTER, "V", egui::FontId::monospace(9.0), egui::Color32::WHITE);
            let hint = if visible { "Hide this video track (visibility)" } else { "Show this video track" };
            if ui.interact(visibility_rect, egui::Id::new(("ff-track-visible", track.id.0)), egui::Sense::click()).on_hover_text(hint).clicked() {
                actions.push(TrackAction::Setting { track: track.id, key: TRACK_ENABLED, on: !visible });
            }
        } else {
            let muted = track.muted;
            let mute_rect = button_rect(44.0);
            header_painter.rect_filled(mute_rect, 2.0, if muted { egui::Color32::from_rgb(170, 80, 60) } else { off });
            header_painter.text(mute_rect.center(), egui::Align2::CENTER_CENTER, "M", egui::FontId::monospace(9.0), egui::Color32::WHITE);
            let hint = if muted { "Unmute this audio track" } else { "Mute this audio track" };
            if ui.interact(mute_rect, egui::Id::new(("ff-track-mute", track.id.0)), egui::Sense::click()).on_hover_text(hint).clicked() {
                actions.push(TrackAction::Setting { track: track.id, key: TRACK_MUTED, on: !muted });
            }

            let soloed = track.solo;
            let solo_rect = button_rect(63.0);
            header_painter.rect_filled(solo_rect, 2.0, if soloed { egui::Color32::from_rgb(170, 140, 55) } else { off });
            if soloed {
                header_painter.circle_filled(solo_rect.center(), 3.8, egui::Color32::WHITE);
            } else {
                header_painter.circle_stroke(solo_rect.center(), 3.8, egui::Stroke::new(1.0, ui.visuals().text_color()));
            }
            let hint = if soloed { "Stop soloing this audio track" } else { "Solo this audio track" };
            if ui.interact(solo_rect, egui::Id::new(("ff-track-solo", track.id.0)), egui::Sense::click()).on_hover_text(hint).clicked() {
                actions.push(TrackAction::Setting { track: track.id, key: TRACK_SOLO, on: !soloed });
            }
        }

        let locked = track.locked;
        let lock_rect = button_rect(82.0);
        header_painter.rect_filled(lock_rect, 2.0, if locked { egui::Color32::from_rgb(185, 70, 55) } else { off });
        header_painter.text(lock_rect.center(), egui::Align2::CENTER_CENTER, "L", egui::FontId::monospace(9.0), egui::Color32::WHITE);
        let hint = if locked { "Unlock this track" } else { "Lock this track (edits rejected)" };
        if ui.interact(lock_rect, egui::Id::new(("ff-track-lock", track.id.0)), egui::Sense::click()).on_hover_text(hint).clicked() {
            actions.push(TrackAction::Setting { track: track.id, key: TRACK_LOCKED, on: !locked });
        }

        header_painter.text(
            egui::pos2(head_rect.left() + 102.0, y + ROW_H / 2.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(11.0),
            ui::TEXT_MID,
        );
        for item in &track.items {
            let clip_rect = timeline_clip_rect(alloc, row, item.start, item.duration, zoom);
            let selected = app.selection == Some(item.id);
            let fill = if item.name.to_lowercase().contains("title") { ui::CLIP_TITLE } else { ui::clip_color(is_video, selected) };
            painter.rect_filled(clip_rect, 3.0, fill);
            if !is_video && let Some((peaks, media_seconds)) = waves.get(&item.item) {
                draw_waveform(&painter, clip_rect, peaks, *media_seconds, item.source_in, item.duration, item.speed);
            }
            painter.rect_stroke(clip_rect, 3.0, egui::Stroke::new(1.0, if selected { ui::ACCENT_HI } else { ui::LINE_DARK }), egui::StrokeKind::Inside);
            let label_painter = painter.with_clip_rect(clip_rect.shrink(3.0));
            label_painter.text(
                clip_rect.left_top() + egui::vec2(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                item.name.clone(),
                egui::FontId::proportional(10.5),
                ui::TEXT_HI,
            );
            hit_rects.push((clip_rect, item.id));
        }
        for tr in &track.transitions {
            // A transition paints over the cut it joins: a dark band with the
            // accent stroke and a bowtie, so a dissolve is visible between its
            // two clips instead of only in the Effects row.
            let rect = timeline_clip_rect(alloc, row, tr.start, tr.duration, zoom);
            let selected = selected_transitions.contains(&tr.id.0);
            painter.rect_filled(rect, 3.0, if selected { ui::ACCENT_SEL } else { egui::Color32::from_rgba_unmultiplied(18, 22, 28, 200) });
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(if selected { 1.6 } else { 1.0 }, if selected { ui::ACCENT_HI } else { ui::ACCENT_DIM }),
                egui::StrokeKind::Inside,
            );
            painter.line_segment([rect.left_top(), egui::pos2(rect.right(), rect.bottom())], egui::Stroke::new(1.0, ui::ACCENT_DIM));
            painter.line_segment([egui::pos2(rect.right(), rect.top()), egui::pos2(rect.left(), rect.bottom())], egui::Stroke::new(1.0, ui::ACCENT_DIM));
            transition_rects.push((rect, tr.id.0));
        }
    }

    // Applying target/source changes after the drawing loop (the sequence was
    // borrowed read-only above). Targeting is editor state, not the document,
    // so it is neither undoable nor dirtying — matching the engine. Track
    // visibility/mute/solo/lock go through App::execute so they are part of the
    // combined undo history and invalidate the monitor.
    for action in actions {
        let (command, params, what) = track_action_command(action);
        let result =
            if command == "timeline.setTrack" { app.execute(command, params) } else { app.session.execute(command, params).map_err(|e| e.to_string()) };
        match result {
            Ok(_) => app.status = format!("{what} updated"),
            Err(e) => app.status = e,
        }
    }

    let px = to_x(app.playhead);
    painter.line_segment([egui::pos2(px, alloc.top()), egui::pos2(px, alloc.bottom())], egui::Stroke::new(2.0, ui::DANGER));

    // Sequence markers are grabbed first, so a Program or split handle at the
    // same frame keeps the grab of the tab it paints over the marker.
    for ((marker_id, _), rect) in marker_starts.iter().zip(&marker_rects) {
        let handle = ui.interact(*rect, egui::Id::new(("ff-sequence-marker-handle", *marker_id)), egui::Sense::drag());
        if handle.dragged() {
            drag_sequence_marker(app, *marker_id, handle.drag_delta().x, zoom);
        }
        if handle.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    // Program In/Out are grabbed on the ruler. Registering them after the body
    // widget puts them on top, so pressing a handle is a mark drag rather than
    // a clip drag or a scrub.
    for (rect, is_in) in [(in_rect, true), (out_rect, false)] {
        let Some(rect) = rect else { continue };
        let id = if is_in { egui::Id::new("ff-sequence-mark-in") } else { egui::Id::new("ff-sequence-mark-out") };
        let handle = ui.interact(rect, id, egui::Sense::drag());
        if handle.dragged() {
            drag_sequence_mark(app, is_in, handle.drag_delta().x, zoom);
        }
        if handle.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    // The split points follow, so at the same frame a split tab wins over the
    // Program handle painting it.
    for (rect, side) in split_rects.iter().zip(SequenceSplit::ALL) {
        let Some(rect) = rect else { continue };
        let handle = ui.interact(*rect, egui::Id::new(side.handle_id()), egui::Sense::drag());
        if handle.dragged() {
            drag_split_mark(app, side, handle.drag_delta().x, zoom);
        }
        if handle.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    // Interaction: drag a clip to move it, its edges to trim (engine
    // commands on release — one undo step); click empty space to scrub.
    const EDGE: f32 = 8.0;
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        // A transition wins the press over the clips under it: its body moves
        // it over the cut, an edge resizes it. It also becomes the selection,
        // like a clip drag does.
        if let Some((rect, id)) = transition_rects.iter().find(|(r, _)| r.contains(pos)).copied() {
            let edge = if pos.x < rect.left() + EDGE {
                Some("in")
            } else if pos.x > rect.right() - EDGE {
                Some("out")
            } else {
                None
            };
            select_transition(app, id);
            app.timeline_drag = Some(TimelineDrag::Transition { transition: id, edge, grab_x: pos.x, last_x: pos.x });
        } else {
            // Read-only phase: what clip, what track, what gesture.
            let decision = hit_rects.iter().find(|(r, _)| r.contains(pos)).copied().and_then(|(rect, id)| {
                let (track, start) = app.sequence().and_then(|s| s.find_item(id).map(|(t, i)| (t, i.start)))?;
                let kind = if pos.x < rect.left() + EDGE {
                    0
                } else if pos.x > rect.right() - EDGE {
                    1
                } else {
                    2
                };
                Some((id, track, start, kind))
            });
            match decision {
                Some((id, track, start, kind)) => {
                    app.selection = Some(id);
                    app.timeline_drag = match kind {
                        0 => Some(TimelineDrag::Trim { clip: id, edge: "in", grab_x: pos.x, last_x: pos.x }),
                        1 => Some(TimelineDrag::Trim { clip: id, edge: "out", grab_x: pos.x, last_x: pos.x }),
                        _ => Some(TimelineDrag::Move { clip: id, track, grab_x: pos.x, origin: start, last_x: pos.x }),
                    };
                }
                None => app.timeline_drag = None,
            }
        }
    }

    if response.dragged()
        && app.timeline_drag.is_some()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match &mut app.timeline_drag {
            Some(TimelineDrag::Move { last_x, .. }) => *last_x = pos.x,
            Some(TimelineDrag::Trim { last_x, .. }) => *last_x = pos.x,
            Some(TimelineDrag::Transition { last_x, .. }) => *last_x = pos.x,
            None => {}
        }
    }

    // A transition drag previews through the engine's own clamp, so the ghost
    // is exactly what release commits.
    if let Some(TimelineDrag::Transition { transition, edge, grab_x, last_x }) = &app.timeline_drag
        && let Some((preview_rect, _)) = transition_rects.iter().find(|(_, t)| t == transition).copied()
        && let Some((start, duration)) = dragged_transition_span(app, *transition, *edge, last_x - grab_x, zoom)
    {
        let x = |t: Tick| alloc.left() + t.seconds() as f32 * zoom;
        let ghost = egui::Rect::from_min_max(egui::pos2(x(start), preview_rect.top()), egui::pos2(x(start + duration), preview_rect.bottom()));
        painter.rect_filled(ghost, 3.0, egui::Color32::from_rgba_unmultiplied(53, 194, 181, 70));
        painter.rect_stroke(ghost, 3.0, egui::Stroke::new(1.5, ui::ACCENT), egui::StrokeKind::Inside);
    }

    // Ghost preview of the drag (the document only changes on release).
    if let Some(drag) = &app.timeline_drag
        && let Some((rect, _)) = hit_rects.iter().find(|(_, id)| Some(*id) == drag_clip(drag)).copied()
    {
        let delta_x = match drag {
            TimelineDrag::Move { grab_x, last_x, .. } => last_x - grab_x,
            TimelineDrag::Trim { edge, grab_x, last_x, .. } => {
                if *edge == "in" {
                    (last_x - grab_x).min(rect.width() - 4.0)
                } else {
                    last_x - grab_x
                }
            }
            // Unreachable: a transition drag has no clip rect to find, and it
            // previewed through its own ghost above.
            TimelineDrag::Transition { .. } => 0.0,
        };
        let ghost = match drag {
            TimelineDrag::Move { .. } => rect.translate(egui::vec2(delta_x.max(-rect.left() + alloc.left()), 0.0)),
            TimelineDrag::Trim { edge, .. } => {
                if *edge == "in" {
                    egui::Rect::from_min_max(egui::pos2(rect.left() + delta_x.max(0.0).min(rect.width() - 4.0), rect.min.y), rect.max)
                } else {
                    egui::Rect::from_min_max(rect.min, egui::pos2((rect.right() + delta_x).max(rect.left() + 4.0), rect.max.y))
                }
            }
            TimelineDrag::Transition { .. } => rect,
        };
        painter.rect_filled(ghost, 3.0, egui::Color32::from_rgba_unmultiplied(53, 194, 181, 70));
        painter.rect_stroke(ghost, 3.0, egui::Stroke::new(1.5, ui::ACCENT), egui::StrokeKind::Inside);
    }

    if response.drag_stopped()
        && let Some(drag) = app.timeline_drag.take()
    {
        let zoom = app.zoom;
        match drag {
            TimelineDrag::Move { clip, track, grab_x, last_x, origin, .. } => {
                let delta = ((last_x - grab_x) / zoom) as f64 * TICKS_PER_SECOND as f64;
                let raw = Tick((origin.0 as f64 + delta).max(0.0) as i64);
                let time = snap_move(app, clip, raw, zoom);
                let params = serde_json::json!({"moves": [{"clip": clip.0, "track": track.0, "time": time.0}], "insert": false, "linked": true});
                match app.execute("timeline.move", params) {
                    Ok(_) => app.status = if time == raw { "Moved clip".to_string() } else { "Moved clip (snapped)".to_string() },
                    Err(e) => app.status = e,
                }
            }
            TimelineDrag::Trim { clip, edge, grab_x, last_x, .. } => {
                let delta = Tick((((last_x - grab_x) / zoom) as f64 * TICKS_PER_SECOND as f64) as i64);
                let snapped = snap_trim(app, clip, edge, delta, zoom);
                let params = serde_json::json!({"clip": clip.0, "edge": edge, "mode": "regular", "delta": snapped.0});
                match app.execute("timeline.trim", params) {
                    Ok(_) => app.status = if snapped == delta { format!("Trimmed {edge} edge") } else { format!("Trimmed {edge} edge (snapped)") },
                    Err(e) => app.status = e,
                }
            }
            TimelineDrag::Transition { transition, edge, grab_x, last_x } => {
                drag_transition_edge(app, transition, edge, last_x - grab_x, zoom);
            }
        }
    }

    // Scrubbing (click or drag on empty space — never while a clip drag runs).
    if (response.clicked() || response.dragged())
        && app.timeline_drag.is_none()
        && let Some(pos) = response.interact_pointer_pos()
    {
        if let Some(id) = transition_rects.iter().find(|(r, _)| r.contains(pos)).map(|(_, id)| *id) {
            select_transition(app, id);
        } else {
            match hit_rects.iter().find(|(r, _)| r.contains(pos)).map(|(_, id)| *id) {
                Some(id) if response.clicked() => app.selection = Some(id),
                _ => {
                    // A click on a marker head jumps to it instead of scrubbing.
                    let mut marker_click: Option<(Tick, String)> = None;
                    if response.clicked() {
                        let mut best = 7.0f32;
                        for (x, t, name) in &marker_hits {
                            let d = (pos.x - x).abs();
                            if d < best {
                                best = d;
                                marker_click = Some((*t, name.clone()));
                            }
                        }
                    }
                    match marker_click {
                        Some((t, name)) => {
                            app.playhead = t;
                            app.status = format!("Marker: {name}");
                        }
                        None => {
                            let secs = ((pos.x - alloc.left()) / app.zoom).max(0.0) as f64;
                            app.playhead = Tick((secs * TICKS_PER_SECOND as f64) as i64);
                        }
                    }
                    app.selection = None;
                }
            }
        }
    }
}

/// The clip a drag operates on.
fn drag_clip(drag: &TimelineDrag) -> Option<filmcraft_project::ClipId> {
    match drag {
        TimelineDrag::Move { clip, .. } => Some(*clip),
        TimelineDrag::Trim { clip, .. } => Some(*clip),
        TimelineDrag::Transition { .. } => None,
    }
}

/// The painted rectangle of a clip on one timeline row. The draw loop and the
/// UI tests both call this, so a scripted drag grabs exactly the rectangle a
/// person sees instead of a second, drifting copy of the geometry.
pub(crate) fn timeline_clip_rect(alloc: egui::Rect, row: usize, start: Tick, duration: Tick, zoom: f32) -> egui::Rect {
    let y = alloc.top() + RULER_H + row as f32 * ROW_H;
    let x = |t: Tick| alloc.left() + t.seconds() as f32 * zoom;
    egui::Rect::from_min_max(egui::pos2(x(start), y + 3.0), egui::pos2(x(start + duration), y + ROW_H - 3.0))
}

/// The Source monitor: a real `render_item` preview of the open media at the
/// engine's source playhead, its transport and the source In/Out marks.
pub fn source_monitor_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("source").layout(egui::Layout::top_down(egui::Align::Min)));
    ui::panel_header(&mut area, "Source");
    area.add_space(3.0);

    let Some(view) = source_inspect(app) else {
        area.add_space(4.0);
        let open = ui::empty_state(
            &mut area,
            "No source open",
            "Select a media item in the project browser, then open it here for its own transport and In/Out marks.",
            "Open Selected",
        );
        if open && let Some(selected) = app.bin_selection {
            open_source(app, selected);
        }
        return;
    };
    let item = ItemId(view["item"].as_u64().unwrap_or(0));
    let rate = source_rate(&view);
    let start = Tick(view["start"].as_i64().unwrap_or(0));
    let end = Tick(view["end"].as_i64().unwrap_or(0));
    let t = app.session.state.source_playhead.clamp(start, end);
    let mark_in = view["markIn"].as_i64();
    let mark_out = view["markOut"].as_i64();
    let playing = app.source_playing;
    let source_name = app.session.project.items.get(&item).map(|i| i.name.clone()).unwrap_or_else(|| "Source".to_string());
    let source_markers = view["markers"].as_array().cloned().unwrap_or_default();

    // The picture, through the engine's own Source-monitor renderer. The
    // transport, the range bar, the marker strip and the range caption below it
    // are fixed-height rows.
    let pic_h = (area.available_rect_before_wrap().height() - 152.0).max(60.0);
    let (pic_rect, _) = area.allocate_exact_size(egui::vec2(area.available_width(), pic_h), egui::Sense::hover());
    let needs = app.source_monitor.as_ref().is_none_or(|m| m.item != item || m.time != t);
    if needs {
        let rendered = app
            .with_sources(|project, sources| filmcraft_render::render_item(project, item, t, 0.5, sources as &dyn filmcraft_render::SourceProvider))
            .flatten();
        if let Some(image) = rendered {
            let size = (image.w as u32, image.h as u32);
            let color = pixels::to_color_image(&image);
            if let Some(m) = app.source_monitor.as_mut().filter(|m| m.item == item) {
                m.texture.set(color, egui::TextureOptions::default());
            } else {
                let texture = ui.ctx().load_texture("source-monitor", color, egui::TextureOptions::default());
                app.source_monitor = Some(crate::app::SourceMonitor { texture, item, time: t, size });
            }
            if let Some(m) = app.source_monitor.as_mut() {
                m.time = t;
                m.size = size;
            }
        }
    }
    if let Some(m) = &app.source_monitor {
        let scale = (pic_rect.width() / m.size.0.max(1) as f32).min(pic_rect.height() / m.size.1.max(1) as f32);
        let shown = egui::vec2(m.size.0 as f32 * scale, m.size.1 as f32 * scale);
        let at = pic_rect.min + egui::vec2((pic_rect.width() - shown.x) / 2.0, (pic_rect.height() - shown.y) / 2.0);
        area.painter().image(
            m.texture.id(),
            egui::Rect::from_min_size(at, shown),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    area.horizontal(|ui| {
        if ui.small_button("|<").on_hover_text("Source start").clicked() {
            source_set_playhead(app, start);
        }
        if ui.small_button("<|").on_hover_text("Previous frame").clicked() {
            source_step(app, -1);
        }
        if ui.small_button(if playing { "||" } else { ">" }).on_hover_text("Play / pause the source").clicked() {
            source_toggle_play(app);
        }
        if ui.small_button("|>").on_hover_text("Next frame").clicked() {
            source_step(app, 1);
        }
        if ui.small_button(">|").on_hover_text("Source end").clicked() {
            source_set_playhead(app, Tick((end.0 - rate.frame_duration().0).max(start.0)));
        }
        let frames = rate.frame_at(t) - rate.frame_at(start);
        let total = rate.frame_at(end) - rate.frame_at(start);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format!("{frames} / {total}  {:.2}s", (t - start).seconds())).size(9.5).monospace().color(ui::TEXT_LO));
        });
    });
    area.add_space(2.0);
    // The range bar: clicking the track seeks, dragging either handle sets that
    // mark, and dragging the band between them slides both. The engine's
    // `adjust_range` owns the clamping and the frame snapping.
    let bounds = filmcraft_time::TimeRange::from_bounds(start, end);
    let range = selected_range(start, end, mark_in.map(Tick), mark_out.map(Tick), rate.frame_duration());
    let bar_width = area.available_width();
    let (bar, _) = area.allocate_exact_size(egui::vec2(bar_width, 20.0), egui::Sense::hover());
    let painter = area.painter().with_clip_rect(bar);
    painter.rect_filled(bar, 2.0, ui::BG_INSET);
    let rects = source_range_rects(bar, bounds, range);
    let band = egui::Rect::from_min_max(egui::pos2(rects.in_handle.center().x, bar.top() + 3.0), egui::pos2(rects.out_handle.center().x, bar.bottom() - 3.0));
    painter.rect_filled(band, 2.0, ui::ACCENT_DIM);
    let span = bounds.duration.0.max(1) as f32;
    let at_x = |t: Tick| (bar.left() + ((t.0 - bounds.start.0) as f32 / span) * bar.width()).clamp(bar.left(), bar.right());
    let play_x = at_x(t);
    painter.line_segment([egui::pos2(play_x, bar.top()), egui::pos2(play_x, bar.bottom())], egui::Stroke::new(1.0, ui::ACCENT_HI));
    let subclip = !view["subclip"].is_null();
    // Source markers: a warm stem with a small tab in the bar's top band, so
    // they read over the accent band and the playhead without hiding either.
    let marker_starts: Vec<Tick> = source_markers.iter().map(|m| Tick(m["start"].as_i64().unwrap_or(0))).collect();
    let marker_rects = source_marker_rects(bar, bounds, &marker_starts);
    for (_, rect) in source_markers.iter().zip(&marker_rects) {
        let x = rect.center().x;
        painter.line_segment([egui::pos2(x, bar.top()), egui::pos2(x, bar.bottom())], egui::Stroke::new(1.0, ui::WARN));
        painter.rect_filled(egui::Rect::from_center_size(egui::pos2(x, bar.top() + 4.0), egui::vec2(SOURCE_MARKER_HANDLE_W, 5.0)), 1.0, ui::WARN);
    }
    if !subclip {
        for handle in [rects.in_handle, rects.out_handle] {
            let grip = egui::Rect::from_center_size(handle.center(), egui::vec2(3.0, bar.height() - 2.0));
            painter.rect_filled(grip, 1.0, ui::ACCENT);
        }
    }
    // The track first, then the grab targets on top of it, so a drag on a
    // handle wins over the seek click underneath.
    let seek = area.interact(bar, egui::Id::new("ff-source-range"), egui::Sense::click());
    if let Some(pos) = seek.interact_pointer_pos().filter(|_| seek.clicked()) {
        seek_source(app, bar, bounds, pos);
    }
    if !subclip {
        let in_resp = area.interact(rects.in_handle, egui::Id::new("ff-source-range-in"), egui::Sense::drag());
        if in_resp.dragged() {
            drag_source_range(app, filmcraft_engine::source_monitor::RangeEdge::In, in_resp.drag_delta().x, bar);
        }
        let out_resp = area.interact(rects.out_handle, egui::Id::new("ff-source-range-out"), egui::Sense::drag());
        if out_resp.dragged() {
            drag_source_range(app, filmcraft_engine::source_monitor::RangeEdge::Out, out_resp.drag_delta().x, bar);
        }
        let body_resp = area.interact(rects.body, egui::Id::new("ff-source-range-body"), egui::Sense::click_and_drag());
        if body_resp.dragged() {
            drag_source_range(app, filmcraft_engine::source_monitor::RangeEdge::Move, body_resp.drag_delta().x, bar);
        } else if let Some(pos) = body_resp.interact_pointer_pos().filter(|_| body_resp.clicked()) {
            seek_source(app, bar, bounds, pos);
        }
        if in_resp.hovered() || out_resp.hovered() {
            area.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if body_resp.hovered() {
            area.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }
    // Marker handles last: their top-band grab targets sit over the bar's seek
    // click and the range body, so pressing a marker is a marker drag.
    for (marker, rect) in source_markers.iter().zip(&marker_rects) {
        let Some(marker_id) = marker["id"].as_u64() else { continue };
        let resp = area.interact(*rect, egui::Id::new(("ff-source-marker-handle", marker_id)), egui::Sense::drag());
        if resp.dragged() {
            drag_source_marker(app, marker_id, resp.drag_delta().x, bar);
        }
        if resp.hovered() {
            area.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    area.horizontal(|ui| {
        ui.menu_button("Marks", |ui| {
            if ui.button("Mark In (I)").clicked() {
                source_mark(app, true);
                ui.close();
            }
            if ui.button("Mark Out (O)").clicked() {
                source_mark(app, false);
                ui.close();
            }
            if ui.button("Go In").clicked() {
                source_goto(app, false);
                ui.close();
            }
            if ui.button("Go Out").clicked() {
                source_goto(app, true);
                ui.close();
            }
            ui.separator();
            if ui.button("Clear In").clicked() {
                source_clear(app, "markers.clearIn");
                ui.close();
            }
            if ui.button("Clear Out").clicked() {
                source_clear(app, "markers.clearOut");
                ui.close();
            }
            if ui.button("Clear Both").clicked() {
                source_clear(app, "markers.clearInOut");
                ui.close();
            }
            ui.separator();
            if ui.button("Open Selected").clicked()
                && let Some(selected) = app.bin_selection
            {
                open_source(app, selected);
                ui.close();
            }
            if ui.button("Close Source").clicked() {
                close_source(app);
                ui.close();
            }
        });
        if ui.small_button("Insert").on_hover_text("Insert the source range at the sequence playhead").clicked() {
            source_edit(app, true);
        }
        if ui.small_button("Overwrite").on_hover_text("Overwrite with the source range at the sequence playhead").clicked() {
            source_edit(app, false);
        }
        if chip(ui, egui::Id::new("ff-source-marker-add"), "+ Marker").on_hover_text("Add a Source marker on the Source playhead").clicked() {
            source_add_marker(app);
        }
        let range = if mark_in.is_none() && mark_out.is_none() {
            "full source".to_string()
        } else {
            source_selected(app).map(|r| format!("{:.2}s selected", r.duration.seconds())).unwrap_or_else(|| "range".to_string())
        };
        ui.label(egui::RichText::new(range).size(9.5).color(ui::TEXT_OFF));
    });
    // The Source marker strip: one row per marker, each seekable, each with its
    // own delete. Both are the engine's Source-targeted commands, so undo is the
    // engine's own history.
    let markers = view["markers"].as_array().cloned().unwrap_or_default();
    area.add_space(2.0);
    egui::ScrollArea::vertical().id_salt("source-markers").max_height(40.0).auto_shrink([false, false]).show(&mut area, |ui| {
        if markers.is_empty() {
            ui.label(ui::hint("No source markers").size(9.0));
        }
        for (index, marker) in markers.iter().enumerate() {
            let at = Tick(marker["start"].as_i64().unwrap_or(0));
            let name = marker["name"].as_str().unwrap_or("Marker");
            let width = ui.available_width();
            let (row, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), egui::Sense::hover());
            let seek = ui.interact(row, egui::Id::new(("ff-source-marker", index)), egui::Sense::click());
            let del_rect = egui::Rect::from_min_max(egui::pos2(row.right() - 16.0, row.top()), row.max);
            let del = ui.interact(del_rect, egui::Id::new(("ff-source-marker-del", index)), egui::Sense::click());
            let fill = if seek.hovered() || del.hovered() { ui::BG_HOVER } else { ui::BG_INSET };
            ui.painter().rect_filled(row, 2.0, fill);
            ui.painter().text(
                egui::pos2(row.left() + 5.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{}  {name}", marker_timecode(rate, at)),
                egui::FontId::monospace(9.0),
                ui::TEXT_MID,
            );
            ui.painter().text(del_rect.center(), egui::Align2::CENTER_CENTER, "x", egui::FontId::proportional(9.0), ui::TEXT_LO);
            if del.clicked() {
                source_delete_marker(app, at);
            } else if seek.clicked() {
                source_set_playhead(app, at);
            }
        }
    });
    if let Some(selected) = source_selected(app) {
        area.label(egui::RichText::new(format!("{}  -  {}", source_name, duration_text(selected.duration.seconds()))).size(9.5).color(ui::TEXT_LO));
    }
    if playing {
        ui.ctx().request_repaint();
    }
}

/// The program monitor, shared by the Edit, Color and new workspaces. The
/// picture is the engine's rendered sequence frame; the transport is real.
pub fn monitor_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let Some(seq_id) = app.sequence_id() else {
        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("monitor").layout(egui::Layout::top_down(egui::Align::Min)));
        ui::panel_header(&mut area, "Program");
        area.add_space(8.0);
        ui::empty_state(
            &mut area,
            "No sequence",
            "A new project creates a sequence. Import media in the project browser and double-click it to place a clip.",
            "Back to Editing",
        );
        return;
    };
    let seq_name = app.sequence_name().unwrap_or_default().to_string();
    let rate = app.sequence().map(|s| s.settings.frame_rate).unwrap_or(SEQUENCE_RATE);
    let frame = rate.frame_at(app.playhead);
    let seq_end = app.sequence_end();

    // Render only when the shown frame, the document revision or the sequence
    // changed — through the same compositor export uses.
    let needs = app.monitor.as_ref().is_none_or(|m| m.frame != frame || m.revision != app.revision || m.seq != seq_id);
    if needs {
        let playhead = app.playhead;
        let rendered = app
            .with_sources(|project, sources| filmcraft_render::render_sequence(project, seq_id, playhead, filmcraft_render::RenderOptions::default(), sources))
            .unwrap_or_else(|| filmcraft_render::Image::new(SEQUENCE_W as usize, SEQUENCE_H as usize));
        let size = (rendered.w as u32, rendered.h as u32);
        let image = pixels::to_color_image(&rendered);
        if let Some(m) = app.monitor.as_mut().filter(|m| m.seq == seq_id) {
            m.texture.set(image, egui::TextureOptions::default());
        } else {
            let texture = ui.ctx().load_texture("program-monitor", image, egui::TextureOptions::default());
            app.monitor = Some(Monitor { texture, frame, revision: app.revision, seq: seq_id, size });
        }
        if let Some(m) = app.monitor.as_mut() {
            m.frame = frame;
            m.revision = app.revision;
        }
    }

    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("monitor").layout(egui::Layout::top_down(egui::Align::Min)));
    ui::panel_header(&mut area, &format!("Program: {seq_name}"));
    area.add_space(3.0);
    let pic_h = (area.available_rect_before_wrap().height() - 58.0).max(70.0);
    let (pic_rect, _) = area.allocate_exact_size(egui::vec2(area.available_width(), pic_h), egui::Sense::hover());
    if let Some(m) = &app.monitor {
        ui::image(&area, pic_rect, m.texture.id(), m.size);
        area.painter().text(
            pic_rect.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            format!("{} x {}", m.size.0, m.size.1),
            egui::FontId::monospace(9.0),
            ui::TEXT_LO,
        );
    } else {
        area.painter().rect_filled(pic_rect, 0.0, ui::BG_INSET);
        area.painter().text(pic_rect.center(), egui::Align2::CENTER_CENTER, "Rendering...", egui::FontId::proportional(11.0), ui::TEXT_LO);
    }

    area.horizontal(|ui| {
        if ui.small_button("|<").on_hover_text("Go to sequence start").clicked() {
            app.playhead = Tick::ZERO;
        }
        if ui.small_button("<|").on_hover_text("Previous frame").clicked() {
            app.step_playhead(-1);
        }
        if ui.small_button(if app.playing { "||" } else { ">" }).on_hover_text("Play / pause (Space)").clicked() {
            app.toggle_playback();
        }
        if ui.small_button("|>").on_hover_text("Next frame").clicked() {
            app.step_playhead(1);
        }
        if ui.small_button(">|").on_hover_text("Go to sequence end").clicked() {
            app.playhead = seq_end;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(app.timecode()).size(10.0).monospace().color(ui::ACCENT_HI));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(if app.wall_start.is_some() && app.playing { "wall clock" } else { "audio clock" }).size(9.0).color(ui::TEXT_OFF));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("frame {}", frame + 1)).size(9.0).monospace().color(ui::TEXT_LO));
        });
    });
    let mut seek = app.playhead.0 as f64;
    if area.add(egui::Slider::new(&mut seek, 0.0..=seq_end.0.max(1) as f64).show_value(false)).changed() {
        app.playhead = Tick(seek.round() as i64);
    }
}

/// The right-hand inspector: the selected clip's controls and the sequence
/// metadata. It deliberately calls into the same workspaces rather than
/// duplicating their edit surfaces.
fn inspector_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui::BG_PANEL);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui::LINE_DARK));
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("inspector"));
    let selected = match app.inspector_tab {
        InspectorTab::Controls => 0,
        InspectorTab::Metadata => 1,
    };
    if let Some(index) = ui::tabs(&mut panel, "inspector-tabs", &["Effect Controls", "Metadata"], selected) {
        app.inspector_tab = if index == 0 { InspectorTab::Controls } else { InspectorTab::Metadata };
    }
    panel.add_space(6.0);
    match app.inspector_tab {
        InspectorTab::Controls => inspector_controls(app, &mut panel),
        InspectorTab::Metadata => inspector_metadata(app, &mut panel),
    }
}

fn inspector_controls(app: &mut App, panel: &mut egui::Ui) {
    let Some(clip) = app.selection else {
        panel.add_space(6.0);
        panel.label(egui::RichText::new("No clip selected").size(13.0).strong().color(ui::TEXT_HI));
        panel.add_space(3.0);
        panel.label(ui::hint(
            "Select a clip on the timeline to see its source, duration, effects and quick links to the grading, text, masking and compositing workspaces.",
        ));
        ui::big_gap(panel);
        ui::section(panel, "SEQUENCE");
        if let Some(seq) = app.sequence() {
            ui::kv(panel, "Name", app.sequence_name().unwrap_or_default().to_string());
            ui::kv(panel, "Frame rate", format!("{:.3} fps", seq.settings.frame_rate.as_f64()));
            ui::kv(panel, "Resolution", format!("{} x {}", seq.settings.width, seq.settings.height));
            ui::kv(panel, "Markers", seq.markers.len().to_string());
        }
        ui::big_gap(panel);
        ui::section(panel, "PROJECT");
        let media = app.session.project.items.values().filter(|i| matches!(&i.kind, ItemKind::Media(_))).count();
        ui::kv(panel, "Media items", media.to_string());
        ui::kv(panel, "Compositions", app.compositions.len().to_string());
        return;
    };

    let info = app.sequence().and_then(|seq| {
        let (track, item) = seq.find_item(clip)?;
        let video = seq.video_tracks.iter().any(|t| t.id == track);
        let effects = item
            .effects
            .iter()
            .map(|e| (e.def().map(|d| d.name.to_string()).unwrap_or_else(|| e.effect.clone()), e.enabled, e.masks.len()))
            .collect::<Vec<_>>();
        Some((
            item.name.clone(),
            track_name(seq, track, video).unwrap_or_else(|| if video { "Video".to_string() } else { "Audio".to_string() }),
            item.start,
            item.duration,
            item.item,
            effects,
        ))
    });
    let Some((name, track, start, duration, item, effects)) = info else {
        panel.label(ui::hint("The selected clip is no longer in the sequence."));
        return;
    };

    panel.horizontal(|ui| {
        let (icon, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
        ui.painter().rect_filled(icon, 3.0, ui::CLIP_VIDEO);
        ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, "V", egui::FontId::proportional(10.0), ui::TEXT_HI);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(name).size(13.0).strong().color(ui::TEXT_HI));
            ui.label(ui::hint(&format!("{track}  -  {}", marker_timecode(SEQUENCE_RATE, start))));
        });
    });
    ui::kv(panel, "Duration", format!("{:.2}s", duration.seconds()));
    ui::kv(panel, "Source item", item.0.to_string());

    ui::section(panel, "QUICK ACTIONS");
    panel.horizontal(|ui| {
        if ui.small_button("Effects").clicked() {
            app.workspace = Workspace::Effects;
        }
        if ui.small_button("Color").clicked() {
            app.workspace = Workspace::Color;
        }
        if ui.small_button("Text").clicked() {
            app.workspace = Workspace::Text;
        }
    });
    panel.horizontal(|ui| {
        if ui.small_button("Mask").clicked() {
            app.workspace = Workspace::Masking;
        }
        if ui.small_button("Compositing").clicked() {
            app.workspace = Workspace::Compositing;
        }
    });

    ui::section(panel, "APPLIED EFFECTS");
    if effects.is_empty() {
        panel.label(ui::hint("No effects on this clip. The Effects workspace adds them."));
    } else {
        for (name, enabled, masks) in &effects {
            panel.horizontal(|ui| {
                ui::dot(ui, if *enabled { ui::ACCENT } else { ui::TEXT_OFF });
                ui.label(egui::RichText::new(name).size(10.5).color(if *enabled { ui::TEXT_MID } else { ui::TEXT_OFF }));
                if *masks > 0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("{masks} mask")).size(9.0).color(ui::TEXT_LO));
                    });
                }
            });
        }
    }

    if let Some(comp) = app.compositions.get(&item) {
        let layers = comp.comp_layers();
        if !layers.is_empty() {
            ui::section(panel, "COMPOSITION LAYERS");
            for (index, (name, video)) in layers.iter().enumerate() {
                panel.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{}.", index + 1)).size(9.5).color(ui::TEXT_OFF));
                    ui::dot(ui, if *video { ui::CLIP_VIDEO } else { ui::CLIP_TITLE });
                    ui.label(egui::RichText::new(name).size(10.5).color(ui::TEXT_MID));
                });
            }
        }
    }

    if let Some(seq) = app.sequence()
        && !seq.markers.is_empty()
    {
        ui::section(panel, "SEQUENCE MARKERS");
        for marker in seq.markers.iter().take(5) {
            panel.horizontal(|ui| {
                ui::dot(ui, marker_color(marker.color));
                ui.label(egui::RichText::new(if marker.name.is_empty() { "Marker" } else { &marker.name }).size(10.0).color(ui::TEXT_MID));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(marker_timecode(SEQUENCE_RATE, marker.start)).size(9.0).monospace().color(ui::TEXT_LO));
                });
            });
        }
    }
}

fn inspector_metadata(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "DOCUMENT");
    ui::kv(panel, "Project", app.session.project.name.clone());
    ui::kv(panel, "Path", app.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Not saved".to_string()));
    ui::kv(panel, "Schema", frameforge_bridge::FFPROJ_SCHEMA.to_string());
    ui::kv(panel, "Dirty", if app.dirty { "Yes" } else { "No" });
    ui::section(panel, "SEQUENCE");
    if let Some(seq) = app.sequence() {
        ui::kv(panel, "Name", app.sequence_name().unwrap_or_default().to_string());
        ui::kv(panel, "Size", format!("{} x {}", seq.settings.width, seq.settings.height));
        ui::kv(panel, "Rate", format!("{:.3} fps", seq.settings.frame_rate.as_f64()));
        ui::kv(panel, "Video tracks", seq.video_tracks.len().to_string());
        ui::kv(panel, "Audio tracks", seq.audio_tracks.len().to_string());
        ui::kv(panel, "Markers", seq.markers.len().to_string());
    }
    ui::section(panel, "PLAYHEAD");
    ui::kv(panel, "Timecode", app.timecode());
    let rate = app.sequence().map(|s| s.settings.frame_rate).unwrap_or(SEQUENCE_RATE);
    ui::kv(panel, "Frame", (rate.frame_at(app.playhead) + 1).to_string());
    ui::kv(panel, "Tick", app.playhead.0.to_string());
    ui::section(panel, "MEDIA");
    let media = app.session.project.items.values().filter(|i| matches!(&i.kind, ItemKind::Media(_))).count();
    ui::kv(panel, "Media items", media.to_string());
    ui::kv(panel, "Compositions", app.compositions.len().to_string());
    ui::footer_hint(panel, "Every edit is an engine command with one chronological undo order.");
}

#[cfg(test)]
mod tests {
    //! Real engine tests for source-patched placement (append and
    //! insert/overwrite), the track-header settings, the marker helpers, the
    //! snap maths and the Source monitor (marks, range edits, marker ripple,
    //! persistence): no UI, but the exact commands the panels use.

    use super::{
        all_bins, bin_parent, bin_trail, item_location, item_matches, move_item_to_bin, new_bin, parse_tags, placement_params, plan_placement, set_tags,
    };
    use filmcraft_engine::Session;
    use filmcraft_media::generators::GeneratorSource;
    use filmcraft_media::{Generator, MediaSource};
    use filmcraft_project::{BinEntry, ClipId, ItemId, ItemKind, Label, MediaClip, MediaRef, Project, TransitionAlign};
    use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};
    use serde_json::json;

    fn demo_session() -> Session {
        let mut s = Session::default();
        s.execute("file.openDemoProject", json!({})).unwrap();
        s
    }

    #[test]
    fn track_header_visibility_mute_solo_and_lock_are_undoable_set_track_edits() {
        let mut s = demo_session();
        let (v1, a1) = {
            let seq = s.active_sequence().unwrap();
            (seq.video_tracks[0].id, seq.audio_tracks[0].id)
        };

        // Editor-state buttons still use the targeting command and its keys.
        let (command, params, _) = super::track_action_command(super::TrackAction::Target { track: v1, on: false });
        assert_eq!(command, "timeline.setTargeting");
        assert_eq!(params["targeted"], json!(false));
        let (command, params, _) = super::track_action_command(super::TrackAction::Patch { track: a1, on: true });
        assert_eq!(command, "timeline.setTargeting");
        assert_eq!(params["sourcePatch"], json!(true));

        let cases = [
            (super::TrackAction::Setting { track: v1, key: super::TRACK_ENABLED, on: false }, super::TRACK_ENABLED, false),
            (super::TrackAction::Setting { track: a1, key: super::TRACK_MUTED, on: true }, super::TRACK_MUTED, true),
            (super::TrackAction::Setting { track: a1, key: super::TRACK_SOLO, on: true }, super::TRACK_SOLO, true),
            (super::TrackAction::Setting { track: v1, key: super::TRACK_LOCKED, on: true }, super::TRACK_LOCKED, true),
        ];
        for (action, key, on) in cases {
            let (command, params, what) = super::track_action_command(action);
            assert_eq!(command, "timeline.setTrack");
            assert!(!what.is_empty());
            assert_eq!(params[key], json!(on), "the dynamic key `{key}` is present");
            s.execute(command, params).unwrap();
        }
        let seq = s.active_sequence().unwrap();
        assert!(!seq.video_tracks[0].enabled, "V hides video output");
        assert!(seq.audio_tracks[0].muted, "M mutes the audio track");
        assert!(seq.audio_tracks[0].solo, "the solo button sets solo");
        assert!(seq.video_tracks[0].locked, "L locks the track");

        assert!(s.undo().is_some(), "track settings enter the engine history");
        let seq = s.active_sequence().unwrap();
        assert!(!seq.video_tracks[0].locked, "undo restores the lock state");
        assert!(seq.audio_tracks[0].solo, "earlier settings stay");
    }

    fn add_tone_item(s: &mut Session) -> ItemId {
        s.edit("New Tone", |project, _| {
            let source =
                GeneratorSource::new(Generator::Tone { hz: 440.0, db: -6.0 }, 1280, 720, filmcraft_time::FrameRate::FPS_24, Tick(10 * TICKS_PER_SECOND));
            let info = source.info().clone();
            let name = info.name.clone();
            Ok(project.add_item(
                name.as_str(),
                Label::Iris,
                ItemKind::Media(MediaClip {
                    media: MediaRef::Generator(Generator::Tone { hz: 440.0, db: -6.0 }),
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
        .unwrap()
    }

    /// A fresh app over a project with one tone item, at the root bin.
    fn tone_app() -> (crate::app::App, ItemId) {
        let mut s = Session::default();
        let item = add_tone_item(&mut s);
        let app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        (app, item)
    }

    #[test]
    fn tags_are_parsed_deduped_searchable_and_undoable() {
        assert_eq!(parse_tags(" b-roll, Hero, b-roll, , HERO "), vec!["b-roll".to_string(), "Hero".to_string()]);

        let (mut app, item) = tone_app();
        set_tags(&mut app, item, "Hero, B-Roll");
        assert_eq!(app.session.project.items[&item].metadata["Tags"], "Hero, B-Roll", "the list is normalised and saved");
        {
            let saved = &app.session.project.items[&item];
            assert!(item_matches(saved, "hero", None), "a tag is searchable by its text");
            assert!(item_matches(saved, "roll", None), "part of a tag matches too");
            assert!(item_matches(saved, "", Some("b-roll")), "the tag filter is case-insensitive");
            assert!(!item_matches(saved, "", Some("drone")));
            assert!(!item_matches(saved, "drone", None));
            assert!(item_matches(saved, "", None), "no search and no filter match everything");
        }
        app.undo();
        assert!(!app.session.project.items[&item].metadata.contains_key("Tags"), "undo removes the tags as one step");
    }

    #[test]
    fn search_also_covers_other_metadata_values() {
        let (mut app, item) = tone_app();
        app.edit_project("Scene", |project, _| {
            project.items.get_mut(&item).unwrap().metadata.insert("Scene".to_string(), "Rooftop".to_string());
            Ok(())
        })
        .unwrap();
        let saved = &app.session.project.items[&item];
        assert!(item_matches(saved, "rooftop", None), "the Scene field is searched");
        assert!(!item_matches(saved, "basement", None));
    }

    #[test]
    fn bins_nest_and_moving_an_item_is_one_undo_step() {
        let mut project = Project::new("Test");
        let outer = project.add_bin("Footage", None);
        let inner = project.add_bin("B-Roll", Some(outer));
        assert_eq!(all_bins(&project.root), vec![(outer, "Footage".to_string()), (inner, "Footage / B-Roll".to_string())]);
        let trail: Vec<String> = bin_trail(&project.root, inner).unwrap().into_iter().map(|(_, name)| name).collect();
        assert_eq!(trail, vec!["Footage".to_string(), "B-Roll".to_string()]);
        assert_eq!(bin_parent(&project.root, inner), Some(outer));
        assert_eq!(bin_parent(&project.root, outer), None, "the root is the parent of a top-level bin");

        let (mut app, item) = tone_app();
        assert_eq!(item_location(&app.session.project.root, item), "Project");
        app.new_bin_name = "Footage".to_string();
        new_bin(&mut app);
        let bin = app.media_bin.expect("the browser follows the bin it just made");
        assert_eq!(item_location(&app.session.project.root, item), "Project", "creating a bin moves nothing");

        move_item_to_bin(&mut app, item, Some(bin));
        assert_eq!(item_location(&app.session.project.root, item), "Project / Footage");
        assert!(
            app.session.project.root.find_bin(bin).unwrap().children.iter().any(|entry| matches!(entry, BinEntry::Item(id) if *id == item)),
            "the item is inside the bin"
        );
        app.undo();
        assert_eq!(item_location(&app.session.project.root, item), "Project", "undo returns the item to the root");
    }

    #[test]
    fn placement_uses_the_patched_video_and_audio_tracks() {
        let mut s = demo_session();
        let item = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 2.0})).unwrap()["item"].as_u64().unwrap());
        let (v2, a2) = {
            let seq = s.active_sequence().unwrap();
            (seq.video_tracks[1].id, seq.audio_tracks[1].id)
        };
        let ends = {
            let seq = s.active_sequence().unwrap();
            (super::track_end(seq, Some(v2), true), super::track_end(seq, Some(a2), false))
        };
        s.execute("timeline.setTargeting", json!({"track": v2.0, "sourcePatch": true})).unwrap();
        s.execute("timeline.setTargeting", json!({"track": a2.0, "sourcePatch": true})).unwrap();
        let tg = s.targeting();
        let plan = {
            let seq = s.active_sequence().unwrap();
            plan_placement(&s.project, seq, item, &tg).unwrap()
        };
        assert_eq!(plan.video.as_ref().map(|v| v.0), Some(v2), "the picture follows the patched V2");
        assert_eq!(plan.audio.as_ref().map(|a| a.0), Some(a2), "the sound follows the patched A2");
        assert_eq!(plan.at, ends.0.max(ends.1), "both streams start together at the later patched-track end");

        let seq_id = s.state.active_sequence.unwrap();
        let r = s.execute("timeline.place", placement_params(item, seq_id, &plan)).unwrap();
        let clips: Vec<ClipId> = r["clips"].as_array().unwrap().iter().filter_map(|c| c.as_u64().map(ClipId)).collect();
        assert_eq!(clips.len(), 2);
        let seq = s.active_sequence().unwrap();
        let mut tracks: Vec<_> = clips.iter().filter_map(|c| seq.find_item(*c).map(|(t, _)| t)).collect();
        tracks.sort_by_key(|t| t.0);
        let mut want = vec![v2, a2];
        want.sort_by_key(|t| t.0);
        assert_eq!(tracks, want);
    }

    #[test]
    fn audio_only_placement_uses_the_audio_destination_and_unpatched_video_is_an_error() {
        let mut s = demo_session();
        let tone = add_tone_item(&mut s);
        let a1 = s.active_sequence().unwrap().audio_tracks[0].id;
        let tg = s.targeting();
        let plan = {
            let seq = s.active_sequence().unwrap();
            plan_placement(&s.project, seq, tone, &tg).unwrap()
        };
        assert!(plan.video.is_none());
        assert_eq!(plan.audio.as_ref().map(|a| a.0), Some(a1));
        let params = placement_params(tone, s.state.active_sequence.unwrap(), &plan);
        assert_eq!(params["track"].as_u64(), Some(a1.0), "an audio-only clip names the sound track as `track`");
        assert!(params.get("audioTrack").is_none());

        // With no video destination, a picture cannot land anywhere.
        let v1 = s.active_sequence().unwrap().video_tracks[0].id;
        s.execute("timeline.setTargeting", json!({"track": v1.0, "sourcePatch": false})).unwrap();
        let bars = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 1.0})).unwrap()["item"].as_u64().unwrap());
        let tg = s.targeting();
        let err = {
            let seq = s.active_sequence().unwrap();
            plan_placement(&s.project, seq, bars, &tg).err().unwrap()
        };
        assert!(err.contains("No video destination"), "{err}");
    }

    #[test]
    fn marker_helpers_add_clear_and_navigate_through_the_real_engine() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Markers"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1"})).unwrap();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");

        app.playhead = Tick(3 * TICKS_PER_SECOND);
        super::add_marker(&mut app);
        let first = {
            let seq = app.session.active_sequence().unwrap();
            assert_eq!(seq.markers.len(), 1, "markers.add lands one marker");
            seq.markers[0].start
        };
        assert_eq!(app.playhead, first, "the marker lands on the frame-snapped playhead");

        app.playhead = first + Tick(TICKS_PER_SECOND);
        super::add_marker(&mut app);
        assert_eq!(app.session.active_sequence().unwrap().markers.len(), 2);

        app.playhead = Tick::ZERO;
        super::go_marker(&mut app, true);
        assert_eq!(app.playhead, first, "next marker goes to the first one");
        super::go_marker(&mut app, true);
        assert!(app.playhead > first, "the second next goes later");
        super::go_marker(&mut app, false);
        assert_eq!(app.playhead, first, "previous marker comes back");

        super::clear_marker(&mut app);
        assert_eq!(app.session.active_sequence().unwrap().markers.len(), 1, "clear removes the marker at the playhead");
        app.undo();
        assert_eq!(app.session.active_sequence().unwrap().markers.len(), 2, "undo restores the cleared marker");
    }

    #[test]
    fn snap_tick_uses_the_nearest_target_inside_the_tolerance() {
        let targets = [Tick(100), Tick(200), Tick(300)];
        assert_eq!(super::snap_tick(Tick(96), &targets, Tick(8)), Tick(100));
        assert_eq!(super::snap_tick(Tick(205), &targets, Tick(8)), Tick(200));
        assert_eq!(super::snap_tick(Tick(150), &targets, Tick(8)), Tick(150), "nothing near: unchanged");
        assert_eq!(super::snap_tick(Tick(109), &targets, Tick(8)), Tick(109), "outside the radius is left alone");
        assert_eq!(super::snap_tick(Tick(108), &targets, Tick(8)), Tick(100), "the boundary is inside the radius");
    }

    #[test]
    fn snap_targets_include_the_playhead_and_skip_the_dragged_clip() {
        let s = demo_session();
        let seq = s.active_sequence().unwrap();
        let clip = seq.video_tracks.iter().chain(&seq.audio_tracks).flat_map(|t| t.items.iter()).map(|i| i.id).next().expect("the demo has clips");
        let all = super::snap_targets(seq, None, Tick(777));
        let without = super::snap_targets(seq, Some(clip), Tick(777));
        assert!(without.contains(&Tick(777)), "the playhead is a target");
        assert_eq!(all.len() - without.len(), 4, "the dragged clip and its linked sound contribute two edges each, and nothing else");
    }

    #[test]
    fn a_snap_prefers_the_edge_that_actually_moved() {
        assert_eq!(super::choose_snap(Tick(100), Tick(110), Tick(100)), Tick(110), "a snapped start beats an unmoved end");
        assert_eq!(super::choose_snap(Tick(100), Tick(100), Tick(90)), Tick(90), "a snapped end beats an unmoved start");
        assert_eq!(super::choose_snap(Tick(100), Tick(106), Tick(96)), Tick(96), "the nearer of two snapped edges wins");
        assert_eq!(super::choose_snap(Tick(100), Tick(100), Tick(100)), Tick(100), "nothing moves when neither edge snaps");
    }

    #[test]
    fn snap_targets_include_sequence_markers() {
        let mut s = demo_session();
        s.execute("markers.add", json!({"time": 4 * TICKS_PER_SECOND, "name": "cue"})).unwrap();
        let seq = s.active_sequence().unwrap();
        let targets = super::snap_targets(seq, None, Tick(777));
        assert!(targets.contains(&Tick(4 * TICKS_PER_SECOND)), "a sequence marker is a snap target");
    }

    #[test]
    fn snap_move_does_not_snap_a_linked_pair_to_its_own_other_half() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Snap"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let a = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 10.0})).unwrap()["item"].as_u64().unwrap());
        let seq = s.state.active_sequence.unwrap();
        let tg = s.targeting();
        let plan = {
            let q = s.active_sequence().unwrap();
            let mut p = plan_placement(&s.project, q, a, &tg).unwrap();
            p.at = Tick(2 * TICKS_PER_SECOND);
            p
        };
        s.execute("timeline.place", placement_params(a, seq, &plan)).unwrap();

        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        app.playhead = Tick(8 * TICKS_PER_SECOND);
        let video = {
            let q = app.sequence().unwrap();
            let item = q.video_tracks[0].items.first().expect("the pair's picture");
            assert!(item.link.is_some(), "the fixture is a linked A/V pair");
            item.id
        };

        // The picture half would otherwise snap backwards onto its own sound at
        // 2 s (the pair has not moved yet), instead of following the pointer.
        let raw = Tick(2 * TICKS_PER_SECOND + TICKS_PER_SECOND / 10);
        assert_eq!(super::snap_move(&app, video, raw, 40.0), raw, "the pair's own other half is not a snap target");
    }

    #[test]
    fn insert_and_overwrite_at_the_playhead_use_the_real_engine_insert_flag() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "IO"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1"})).unwrap();
        let a = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 2.0})).unwrap()["item"].as_u64().unwrap());
        let b = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 1.0})).unwrap()["item"].as_u64().unwrap());
        let seq = s.state.active_sequence.unwrap();
        let tg = s.targeting();
        let plan = {
            let q = s.active_sequence().unwrap();
            let mut p = plan_placement(&s.project, q, a, &tg).unwrap();
            p.at = Tick::ZERO;
            p
        };
        s.execute("timeline.place", placement_params(a, seq, &plan)).unwrap();

        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        app.playhead = Tick::ZERO;
        let one_second = Tick(TICKS_PER_SECOND);

        super::place_at_playhead(&mut app, b, true);
        {
            let q = app.session.active_sequence().unwrap();
            let v1 = &q.video_tracks[0];
            assert!(v1.items.iter().any(|i| i.item == b && i.start == Tick::ZERO), "insert lands B at the playhead");
            let a_after = v1.items.iter().find(|i| i.item == a).map(|i| i.start);
            assert!(a_after.is_some_and(|t| t >= one_second), "insert ripples A right, got {a_after:?}");
        }

        app.undo();
        super::place_at_playhead(&mut app, b, false);
        {
            let q = app.session.active_sequence().unwrap();
            let v1 = &q.video_tracks[0];
            assert!(v1.items.iter().any(|i| i.item == b && i.start == Tick::ZERO), "overwrite lands B at the playhead");
            assert!(v1.items.iter().filter(|i| i.item == a).all(|i| i.start >= one_second), "overwrite trims A under B");
        }
    }

    /// A fresh project with one sequence and a 4 s Bars & Tone media item.
    fn source_fixture() -> (crate::app::App, ItemId) {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Source"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let src = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 4.0})).unwrap()["item"].as_u64().unwrap());
        let app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        (app, src)
    }

    #[test]
    fn source_open_previews_real_frames_and_its_marks_stay_off_the_program_playhead() {
        let (mut app, src) = source_fixture();
        app.playhead = Tick(2 * TICKS_PER_SECOND);
        super::open_source(&mut app, src);
        assert_eq!(app.session.state.source_item, Some(src));
        let preview = app
            .with_sources(|project, sources| filmcraft_render::render_item(project, src, Tick::ZERO, 0.5, sources as &dyn filmcraft_render::SourceProvider))
            .flatten();
        assert!(preview.is_some_and(|i| i.w > 0 && i.h > 0), "the Source monitor renders a real frame");

        super::source_set_playhead(&mut app, Tick(TICKS_PER_SECOND));
        assert_eq!(app.playhead, Tick(2 * TICKS_PER_SECOND), "source seeking must not move the Program playhead");
        super::source_mark(&mut app, true);
        assert_eq!(app.playhead, Tick(2 * TICKS_PER_SECOND), "source marking must not move the Program playhead");
        super::source_set_playhead(&mut app, Tick(2 * TICKS_PER_SECOND));
        super::source_mark(&mut app, false);

        let selected = super::source_selected(&mut app).expect("a selected range");
        assert_eq!(selected.start, Tick(TICKS_PER_SECOND));
        assert_eq!(selected.duration, Tick(TICKS_PER_SECOND) + FrameRate::FPS_24.frame_duration());
    }

    #[test]
    fn source_insert_places_the_marked_range_and_ripples_markers_with_undo() {
        let (mut app, src) = source_fixture();
        super::open_source(&mut app, src);
        let fd = FrameRate::FPS_24.frame_duration();
        super::source_set_playhead(&mut app, Tick(TICKS_PER_SECOND));
        super::source_mark(&mut app, true);
        super::source_set_playhead(&mut app, Tick(2 * TICKS_PER_SECOND));
        super::source_mark(&mut app, false);
        let inserted = Tick(TICKS_PER_SECOND) + fd;

        let before = Tick(TICKS_PER_SECOND / 2);
        let at = Tick(2 * TICKS_PER_SECOND);
        let after = Tick(4 * TICKS_PER_SECOND);
        app.execute("markers.add", json!({"time": before.0, "name": "before"})).unwrap();
        app.execute("markers.add", json!({"time": at.0, "name": "at"})).unwrap();
        app.execute("markers.add", json!({"time": after.0, "name": "after"})).unwrap();

        app.playhead = at;
        assert!(app.session.state.ripple_sequence_markers, "FrameForge defaults Ripple Markers on");
        super::source_edit(&mut app, true);
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.video_tracks[0].items.len(), 1, "one inserted picture clip");
            assert_eq!(q.video_tracks[0].items[0].duration, inserted, "the clip carries exactly the marked range");
            let marker_at = |name: &str| q.markers.iter().find(|m| m.name == name).unwrap().start;
            assert_eq!(marker_at("before"), before, "markers before the insert point stay");
            assert_eq!(marker_at("at"), at + inserted, "a marker exactly at the insert point moves");
            assert_eq!(marker_at("after"), after + inserted, "later markers move by the inserted range");
        }

        app.undo();
        {
            let q = app.session.active_sequence().unwrap();
            assert!(q.video_tracks[0].items.is_empty(), "undo removes the inserted clip");
            assert_eq!(q.markers.iter().find(|m| m.name == "at").unwrap().start, at, "undo restores marker positions");
        }

        app.session.state.ripple_sequence_markers = false;
        app.playhead = at;
        super::source_edit(&mut app, true);
        let q = app.session.active_sequence().unwrap();
        assert_eq!(q.markers.iter().find(|m| m.name == "after").unwrap().start, after, "ripple off leaves marker positions alone");
    }

    #[test]
    fn source_mark_edges_are_normalized_and_one_sided_marks_use_the_source_edges() {
        let (mut app, src) = source_fixture();
        super::open_source(&mut app, src);
        let view = super::source_inspect(&mut app).unwrap();
        let start = Tick(view["start"].as_i64().unwrap());
        let end = Tick(view["end"].as_i64().unwrap());

        // One-sided In: [In, end)
        super::source_set_playhead(&mut app, Tick(2 * TICKS_PER_SECOND));
        super::source_mark(&mut app, true);
        let selected = super::source_selected(&mut app).unwrap();
        assert_eq!((selected.start, selected.end()), (Tick(2 * TICKS_PER_SECOND), end));

        // Reversed marks collapse to an empty selection, never a backwards edit.
        super::source_set_playhead(&mut app, Tick(TICKS_PER_SECOND));
        super::source_mark(&mut app, false);
        let selected = super::source_selected(&mut app).unwrap();
        assert_eq!(selected.duration, Tick::ZERO, "In after Out is an empty selection");

        // Clearing both returns the full source.
        super::source_clear(&mut app, "markers.clearInOut");
        let selected = super::source_selected(&mut app).unwrap();
        assert_eq!((selected.start, selected.end()), (start, end));
    }

    #[test]
    fn source_marks_survive_project_save_and_reopen() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Marks"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1"})).unwrap();
        let src = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 2.0})).unwrap()["item"].as_u64().unwrap());
        s.state.source_item = Some(src);
        s.execute("project.setMarks", json!({"item": src.0, "in": 1000, "out": 2000})).unwrap();

        let doc = frameforge_bridge::FrameForgeProject {
            schema: frameforge_bridge::FFPROJ_SCHEMA,
            project: (*s.project).clone(),
            compositions: std::collections::HashMap::new(),
        };
        let path = std::env::temp_dir().join(format!("ff_source_marks_{}.ffproj", std::process::id()));
        doc.save(&path).unwrap();
        let loaded = frameforge_bridge::FrameForgeProject::load(&path).unwrap();
        let (_, clip, _) = loaded.project.resolve_media(src).unwrap();
        assert_eq!((clip.mark_in, clip.mark_out), (Some(Tick(1000)), Some(Tick(2000))));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_source_playhead_stops_on_the_last_frame() {
        let (start, end) = (Tick::ZERO, Tick(10));

        // Inside the range: the playhead advances by exactly the elapsed time.
        assert_eq!(super::advance_source(Tick::ZERO, Tick(5), start, end), (Tick(5), false));

        // Reaching the end lands on the last frame and reports the stop, so the
        // audio clock and the wall clock end playback identically.
        assert_eq!(super::advance_source(Tick::ZERO, Tick(10), start, end), (Tick(9), true));

        // Overshooting (a slow frame, or a finished audio queue) still clamps.
        assert_eq!(super::advance_source(Tick(8), Tick(5), start, end), (Tick(9), true));
    }

    #[test]
    fn ripple_delete_closes_the_gap_across_linked_av_and_markers_and_refuses_locks() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Ripple"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let a = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 2.0})).unwrap()["item"].as_u64().unwrap());
        let b = ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 1.0})).unwrap()["item"].as_u64().unwrap());
        let seq = s.state.active_sequence.unwrap();
        let tg = s.targeting();
        let plan = {
            let q = s.active_sequence().unwrap();
            let mut p = plan_placement(&s.project, q, a, &tg).unwrap();
            p.at = Tick::ZERO;
            p
        };
        s.execute("timeline.place", placement_params(a, seq, &plan)).unwrap();

        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        app.playhead = two_seconds;
        super::place_at_playhead(&mut app, b, false);
        app.execute("markers.add", json!({"time": TICKS_PER_SECOND / 2, "name": "inside"})).unwrap();
        app.execute("markers.add", json!({"time": two_seconds.0, "name": "edge"})).unwrap();
        app.execute("markers.add", json!({"time": 3 * TICKS_PER_SECOND, "name": "after"})).unwrap();

        let first = {
            let q = app.session.active_sequence().unwrap();
            let first = q.video_tracks[0].items.iter().find(|i| i.item == a).unwrap().id;
            assert!(q.audio_tracks[0].items.iter().any(|i| i.item == a), "the fixture is a linked A/V pair");
            first
        };
        app.selection = Some(first);
        super::ripple_delete_selected(&mut app);
        {
            let q = app.session.active_sequence().unwrap();
            let v1 = &q.video_tracks[0];
            assert!(!v1.items.iter().any(|i| i.item == a), "ripple delete removes A's picture");
            assert_eq!(v1.items.iter().find(|i| i.item == b).unwrap().start, Tick::ZERO, "B closes the gap");
            let a1 = &q.audio_tracks[0];
            assert!(!a1.items.iter().any(|i| i.item == a), "ripple delete removes A's linked sound");
            assert_eq!(a1.items.iter().find(|i| i.item == b).unwrap().start, Tick::ZERO, "linked sound stays in sync");
            assert!(q.markers.iter().all(|m| m.name != "inside"), "a marker inside the removed span is removed");
            let marker_at = |name: &str| q.markers.iter().find(|m| m.name == name).unwrap().start;
            assert_eq!(marker_at("edge"), Tick::ZERO, "a marker at the removed span's end moves to the gap start");
            assert_eq!(marker_at("after"), Tick(TICKS_PER_SECOND), "later markers move left by the gap");
        }

        app.undo();
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.video_tracks[0].items.iter().find(|i| i.item == a).unwrap().start, Tick::ZERO, "undo restores A");
            assert_eq!(q.video_tracks[0].items.iter().find(|i| i.item == b).unwrap().start, two_seconds, "undo restores B");
            assert_eq!(q.markers.iter().find(|m| m.name == "edge").unwrap().start, two_seconds, "undo restores marker positions");
            assert_eq!(
                q.markers.iter().find(|m| m.name == "inside").unwrap().start,
                Tick(TICKS_PER_SECOND / 2),
                "undo restores the marker removed inside the span"
            );
        }

        // A locked track refuses the whole edit rather than desynchronising the pair.
        let (first, video_track) = {
            let q = app.session.active_sequence().unwrap();
            (q.video_tracks[0].items.iter().find(|i| i.item == a).unwrap().id, q.video_tracks[0].id)
        };
        app.execute("timeline.setTrack", json!({"track": video_track.0, "locked": true})).unwrap();
        app.selection = Some(first);
        super::ripple_delete_selected(&mut app);
        assert!(app.status.contains("locked"), "the status explains the refusal, got {:?}", app.status);
        let q = app.session.active_sequence().unwrap();
        assert!(q.video_tracks[0].items.iter().any(|i| i.item == a), "the locked clip is untouched");
    }

    /// `count` adjacent 2 s clips cut from 4 s media (so each keeps a 2 s handle),
    /// placed through the Source monitor's real three-point edit.
    fn trimmed_clip_app(count: usize) -> (crate::app::App, Vec<ItemId>, Vec<ClipId>) {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Trim"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let items: Vec<ItemId> =
            (0..count).map(|_| ItemId(s.execute("file.newBarsAndTone", json!({"seconds": 4.0})).unwrap()["item"].as_u64().unwrap())).collect();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        let fd = FrameRate::FPS_24.frame_duration();
        for (k, item) in items.iter().enumerate() {
            app.playhead = Tick(two_seconds.0 * k as i64);
            super::open_source(&mut app, *item);
            super::source_set_playhead(&mut app, Tick::ZERO);
            super::source_mark(&mut app, true);
            super::source_set_playhead(&mut app, two_seconds - fd);
            super::source_mark(&mut app, false);
            super::source_edit(&mut app, false);
        }
        let clips = app.session.active_sequence().unwrap().video_tracks[0].items.iter().map(|i| i.id).collect();
        (app, items, clips)
    }

    #[test]
    fn roll_moves_only_the_shared_edit_point_and_undoes() {
        let (mut app, _items, clips) = trimmed_clip_app(2);
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        let half = FrameRate::FPS_24.tick_of(12);
        app.selection = Some(clips[0]);
        super::roll_selected(&mut app, 12);
        {
            let q = app.session.active_sequence().unwrap();
            let a = q.video_tracks[0].items.iter().find(|i| i.id == clips[0]).unwrap();
            let b = q.video_tracks[0].items.iter().find(|i| i.id == clips[1]).unwrap();
            assert_eq!(a.start, Tick::ZERO, "roll leaves the left clip's start fixed");
            assert_eq!(a.end(), two_seconds + half, "roll extends the left clip");
            assert_eq!(b.start, two_seconds + half, "the shared edit point moved");
            assert_eq!(b.end(), Tick(4 * TICKS_PER_SECOND), "roll leaves the right clip's end fixed");
            assert_eq!(b.source_in, half, "the right clip loses the rolled head frames");
        }
        app.undo();
        let q = app.session.active_sequence().unwrap();
        assert_eq!(q.video_tracks[0].items.iter().find(|i| i.id == clips[0]).unwrap().end(), two_seconds, "undo restores the edit point");
        assert_eq!(q.video_tracks[0].items.iter().find(|i| i.id == clips[1]).unwrap().start, two_seconds, "undo restores the right clip");
    }

    #[test]
    fn slip_and_slide_keep_the_right_things_fixed() {
        let (mut app, _items, clips) = trimmed_clip_app(3);
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        let four_seconds = Tick(4 * TICKS_PER_SECOND);
        let six_seconds = Tick(6 * TICKS_PER_SECOND);
        let half = FrameRate::FPS_24.tick_of(12);

        // Slip: only the selected clip's source range moves.
        app.selection = Some(clips[1]);
        super::slip_selected(&mut app, 12);
        {
            let q = app.session.active_sequence().unwrap();
            let a = q.video_tracks[0].items.iter().find(|i| i.id == clips[0]).unwrap();
            let b = q.video_tracks[0].items.iter().find(|i| i.id == clips[1]).unwrap();
            let c = q.video_tracks[0].items.iter().find(|i| i.id == clips[2]).unwrap();
            assert_eq!((b.start, b.end()), (two_seconds, four_seconds), "slip keeps B's timeline span");
            assert_eq!(b.source_in, half, "slip moves B's source In");
            assert_eq!((a.start, a.end()), (Tick::ZERO, two_seconds), "slip leaves the left neighbour alone");
            assert_eq!((c.start, c.end()), (four_seconds, six_seconds), "slip leaves the right neighbour alone");
        }
        app.undo();

        // Slide: B moves, and the neighbours absorb the move in their source ranges.
        app.selection = Some(clips[1]);
        super::slide_selected(&mut app, 12);
        {
            let q = app.session.active_sequence().unwrap();
            let a = q.video_tracks[0].items.iter().find(|i| i.id == clips[0]).unwrap();
            let b = q.video_tracks[0].items.iter().find(|i| i.id == clips[1]).unwrap();
            let c = q.video_tracks[0].items.iter().find(|i| i.id == clips[2]).unwrap();
            assert_eq!((b.start, b.end()), (two_seconds + half, four_seconds + half), "slide moves B by 12 frames");
            assert_eq!(a.start, Tick::ZERO, "slide keeps the sequence start");
            assert_eq!(a.end(), two_seconds + half, "slide extends the left neighbour");
            assert_eq!(c.start, four_seconds + half, "slide moves the right neighbour's start");
            assert_eq!(c.end(), six_seconds, "slide keeps the sequence end");
            assert_eq!(c.source_in, half, "slide trims the right neighbour's head");
        }
        app.undo();
        let q = app.session.active_sequence().unwrap();
        assert_eq!(q.video_tracks[0].items.iter().find(|i| i.id == clips[1]).unwrap().start, two_seconds, "undo restores B");
        assert_eq!(q.video_tracks[0].items.iter().find(|i| i.id == clips[2]).unwrap().start, four_seconds, "undo restores C");
    }

    /// Place exactly `src_in..src_out` of `item` at timeline `at` through the
    /// Source monitor, leaving the media's unused head/tail as handles.
    fn place_source_range(app: &mut crate::app::App, item: ItemId, at: Tick, src_in: Tick, src_out: Tick) {
        app.playhead = at;
        super::open_source(app, item);
        super::source_set_playhead(app, src_in);
        super::source_mark(app, true);
        super::source_set_playhead(app, src_out - FrameRate::FPS_24.frame_duration());
        super::source_mark(app, false);
        super::source_edit(app, false);
    }

    /// Two adjacent 2 s clips with handles on both sides. The color-matte
    /// version has a hard red→blue cut; the Bars & Tone version has the same
    /// picture/sound pair for the audio transition test.
    fn transition_app(video: bool) -> (crate::app::App, Vec<ClipId>) {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "Transitions"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let new = |s: &mut Session, color: Option<&str>| {
            let v = match color {
                Some(color) => s.execute("file.newColorMatte", json!({"color": color, "seconds": 4.0})).unwrap(),
                None => s.execute("file.newBarsAndTone", json!({"seconds": 4.0})).unwrap(),
            };
            ItemId(v["item"].as_u64().unwrap())
        };
        let a = new(&mut s, video.then_some("#ff0000"));
        let b = new(&mut s, video.then_some("#0000ff"));
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        place_source_range(&mut app, a, Tick::ZERO, Tick::ZERO, two_seconds);
        place_source_range(&mut app, b, two_seconds, Tick(TICKS_PER_SECOND), Tick(3 * TICKS_PER_SECOND));
        let clips = app.session.active_sequence().unwrap().video_tracks[0].items.iter().map(|i| i.id).collect();
        (app, clips)
    }

    #[test]
    fn video_cross_dissolve_changes_the_rendered_pixels_at_the_cut() {
        let (mut app, clips) = transition_app(true);
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        let six_frames = FrameRate::FPS_24.tick_of(6);
        app.selection = Some(clips[1]);
        super::apply_dissolve(&mut app, true);
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.video_tracks[0].transitions.len(), 1, "one real transition on V1");
            let tr = &q.video_tracks[0].transitions[0];
            assert_eq!((tr.start, tr.duration), (two_seconds - six_frames, Tick(six_frames.0 * 2)), "a 12-frame dissolve centred on the cut");
            assert_eq!((tr.from, tr.to), (Some(clips[0]), Some(clips[1])), "the transition names the outgoing and incoming clips");
        }

        let seq_id = app.sequence_id().unwrap();
        let at = |app: &mut crate::app::App, secs: f64| {
            let t = FrameRate::FPS_24.tick_of((secs * 24.0).round() as i64);
            app.with_sources(|p, sources| {
                filmcraft_render::render_sequence(p, seq_id, t, filmcraft_render::RenderOptions { scale: 0.25, ..Default::default() }, sources)
            })
            .unwrap()
        };
        let px = |img: &filmcraft_render::Image| img.get(img.w / 2, img.h / 2);
        let before = px(&at(&mut app, 1.5));
        let mid = px(&at(&mut app, 2.0));
        let after = px(&at(&mut app, 2.5));
        assert!(before[0] > 0.9 && before[2] < 0.1, "before the dissolve is red: {before:?}");
        assert!(after[2] > 0.9 && after[0] < 0.1, "after the dissolve is blue: {after:?}");
        assert!(mid[0] > 0.2 && mid[2] > 0.2 && mid[1] < 0.1, "the cut is a real red/blue mix: {mid:?}");

        app.undo();
        let hard_cut = px(&at(&mut app, 2.0));
        assert!(hard_cut[2] > 0.9 && hard_cut[0] < 0.1, "undo removes the dissolve and the cut is blue: {hard_cut:?}");
    }

    #[test]
    fn audio_crossfade_changes_the_rendered_samples_at_the_cut() {
        let (mut app, clips) = transition_app(false);
        // Select the incoming picture; the helper finds its linked sound and
        // applies the audio transition at that same In edge.
        app.selection = Some(clips[1]);
        super::apply_dissolve(&mut app, false);
        let q = app.session.active_sequence().unwrap().clone();
        assert_eq!(q.audio_tracks[0].transitions.len(), 1, "one real transition on A1");
        let sr = q.settings.sample_rate as i64;
        let mix_at = |app: &mut crate::app::App, secs: f64| {
            let start = (secs * sr as f64).round() as i64;
            app.with_sources(|p, sources| filmcraft_render::audio::mix_sequence(p, &q, start, 4800, sources)).unwrap()
        };
        let rms = |samples: &[f32]| (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt();
        let before = mix_at(&mut app, 1.5);
        let mid = mix_at(&mut app, 2.0);
        let after = mix_at(&mut app, 2.5);
        let (before, mid, after) = (rms(&before.channels[0]), rms(&mid.channels[0]), rms(&after.channels[0]));
        assert!(before > 0.01, "the fixture has sound: {before}");
        assert!(mid > before * 1.15, "the crossfade overlaps both sides at the cut: {mid} vs {before}");
        assert!(mid > after * 1.15, "the crossfade is real on the incoming side too: {mid} vs {after}");
    }

    #[test]
    fn transition_duration_and_alignment_are_editable_through_the_engine() {
        let (mut app, clips) = transition_app(true);
        let cut = Tick(2 * TICKS_PER_SECOND);
        let frames = |n: i64| FrameRate::FPS_24.tick_of(n);

        app.selection = Some(clips[1]);
        app.transition_frames = 24;
        super::apply_dissolve(&mut app, true);
        let id = {
            let q = app.session.active_sequence().unwrap();
            let tr = &q.video_tracks[0].transitions[0];
            assert_eq!((tr.start, tr.duration), (cut - frames(12), frames(24)), "a 24-frame dissolve is centred on the cut");
            assert_eq!(tr.align, TransitionAlign::CenterAtCut);
            tr.id
        };

        app.transition_frames = 8;
        app.transition_align = TransitionAlign::EndAtCut;
        super::update_transition(&mut app);
        {
            let q = app.session.active_sequence().unwrap();
            let tr = &q.video_tracks[0].transitions[0];
            assert_eq!(tr.id, id, "the same transition was edited");
            assert_eq!((tr.start, tr.duration), (cut - frames(8), frames(8)), "8 frames ending at the cut");
            assert_eq!(tr.align, TransitionAlign::EndAtCut);
        }

        app.undo();
        {
            let q = app.session.active_sequence().unwrap();
            let tr = &q.video_tracks[0].transitions[0];
            assert_eq!((tr.start, tr.duration), (cut - frames(12), frames(24)), "undo restores the 24-frame centred span");
            assert_eq!(tr.align, TransitionAlign::CenterAtCut);
        }

        // The outgoing clip's In edge has no transition to edit.
        app.selection = Some(clips[0]);
        super::update_transition(&mut app);
        assert!(app.status.contains("No transition"), "status says why: {}", app.status);
    }

    #[test]
    fn dragging_a_transition_edge_sets_its_length_and_one_undo_restores_it() {
        let (mut app, clips) = transition_app(true);
        let frames = |n: i64| FrameRate::FPS_24.tick_of(n);
        app.selection = Some(clips[1]);
        app.transition_frames = 24;
        super::apply_dissolve(&mut app, true);
        let id = app.session.active_sequence().unwrap().video_tracks[0].transitions[0].id.0;

        // 40 px/s for one second is 24 frames: the out edge moves a second
        // later, so the transition holds its start and lasts 48 frames.
        super::drag_transition_edge(&mut app, id, Some("out"), 40.0, 40.0);
        {
            let q = app.session.active_sequence().unwrap();
            let tr = &q.video_tracks[0].transitions[0];
            assert_eq!(tr.duration, frames(48), "the dragged edge sets the length");
            assert_eq!(tr.start, Tick(2 * TICKS_PER_SECOND) - frames(12), "the out edge moves, so the start holds");
        }
        assert_eq!(app.status, "Transition 48 frames");

        // A drag past the clips it joins is clamped by the engine's own rules,
        // not refused: from the start it holds, the two 2 s clips leave 2.5 s.
        super::drag_transition_edge(&mut app, id, Some("out"), 10_000.0, 40.0);
        {
            let q = app.session.active_sequence().unwrap();
            let tr = &q.video_tracks[0].transitions[0];
            assert_eq!((tr.start, tr.duration), (Tick(2 * TICKS_PER_SECOND) - frames(12), frames(60)), "the clamp stops at the clips");
        }

        app.undo();
        assert_eq!(app.session.active_sequence().unwrap().video_tracks[0].transitions[0].duration, frames(48), "one undo restores the previous length");
        app.undo();
        assert_eq!(app.session.active_sequence().unwrap().video_tracks[0].transitions[0].duration, frames(24), "and one more the dissolve that was applied");
    }

    #[test]
    fn sequence_in_out_marks_lift_leaves_a_gap_and_extract_closes_it_and_undoes() {
        let (mut app, _items, clips) = trimmed_clip_app(3);
        let two_seconds = Tick(2 * TICKS_PER_SECOND);
        let four_seconds = Tick(4 * TICKS_PER_SECOND);
        let fd = FrameRate::FPS_24.frame_duration();

        // Program In at 2 s, Out on the last frame of the 2 s clip (Out is inclusive).
        app.playhead = two_seconds;
        super::program_mark(&mut app, true);
        app.playhead = four_seconds - fd;
        super::program_mark(&mut app, false);
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.mark_in, Some(two_seconds), "Program In lands at the playhead");
            assert_eq!(q.mark_out, Some(four_seconds - fd), "Program Out is inclusive");
        }

        // Lift removes B from the targeted V1/A1 tracks and leaves a gap: C stays put.
        super::lift_in_out(&mut app);
        {
            let q = app.session.active_sequence().unwrap();
            assert!(!q.video_tracks[0].items.iter().any(|i| i.id == clips[1]), "B is gone");
            assert_eq!(
                q.video_tracks[0].items.iter().map(|i| i.start).collect::<Vec<_>>(),
                vec![Tick::ZERO, four_seconds],
                "lift leaves C where it was (the gap stays)"
            );
            assert_eq!(
                q.audio_tracks[0].items.iter().map(|i| i.start).collect::<Vec<_>>(),
                vec![Tick::ZERO, four_seconds],
                "the linked sound lifts with the picture"
            );
            assert_eq!(q.mark_in, Some(two_seconds), "lift keeps the marks");
        }

        app.undo();
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.video_tracks[0].items.iter().map(|i| i.start).collect::<Vec<_>>(), vec![Tick::ZERO, two_seconds, four_seconds], "undo restores B");
        }

        // Extract closes the gap, clears the marks and parks the playhead at the range start.
        super::extract_in_out(&mut app);
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(
                q.video_tracks[0].items.iter().map(|i| i.start).collect::<Vec<_>>(),
                vec![Tick::ZERO, two_seconds],
                "extract closes the gap (C moves to B's start)"
            );
            assert_eq!(q.audio_tracks[0].items.iter().map(|i| i.start).collect::<Vec<_>>(), vec![Tick::ZERO, two_seconds], "the linked sound stays in sync");
            assert_eq!((q.mark_in, q.mark_out), (None, None), "extract clears the marks");
        }
        assert_eq!(app.playhead, two_seconds, "the shell follows the engine playhead to the range start");

        app.undo();
        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(
                q.video_tracks[0].items.iter().map(|i| i.start).collect::<Vec<_>>(),
                vec![Tick::ZERO, two_seconds, four_seconds],
                "undo restores the extracted range"
            );
            assert_eq!(q.mark_in, Some(two_seconds), "undo restores the cleared marks");
        }
    }

    #[test]
    fn marker_fields_update_is_an_undoable_project_edit() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "MarkerEdit"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1"})).unwrap();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");

        app.playhead = Tick(3 * TICKS_PER_SECOND);
        super::add_marker(&mut app);
        let (marker_id, before_name) = {
            let q = app.session.active_sequence().unwrap();
            (q.markers[0].id, q.markers[0].name.clone())
        };
        assert_eq!(before_name, "", "markers.add defaults to an empty name");

        app.marker_name = "Shot 1".to_string();
        app.marker_comment = "pickup".to_string();
        app.marker_duration_frames = 24;
        app.marker_color = Label::Blue;
        super::update_marker_at_playhead(&mut app);
        {
            let q = app.session.active_sequence().unwrap();
            let m = q.markers.iter().find(|m| m.id == marker_id).unwrap();
            assert_eq!(m.name, "Shot 1");
            assert_eq!(m.comment, "pickup");
            assert_eq!(m.duration, app.session.sequence_rate().tick_of(24), "the length is 24 frames");
            assert_eq!(m.color, Label::Blue);
        }

        app.undo();
        {
            let q = app.session.active_sequence().unwrap();
            let m = q.markers.iter().find(|m| m.id == marker_id).unwrap();
            assert_eq!(m.name, before_name, "undo restores the old name");
            assert_eq!(m.comment, "", "undo restores the old comment");
            assert_eq!(m.duration, Tick::ZERO, "undo restores a point marker");
            assert_eq!(m.color, Label::Green, "undo restores the default colour");
        }

        // Away from a marker the helper reports why nothing happened.
        app.playhead = Tick(9 * TICKS_PER_SECOND);
        app.marker_name = "Elsewhere".to_string();
        super::update_marker_at_playhead(&mut app);
        assert!(app.status.contains("No marker"), "status says why: {}", app.status);
    }

    #[test]
    fn marker_csv_import_parses_quoted_text_and_is_one_undo() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "MarkerCsvImport"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");

        // The export's own header, a quoted name/comment with commas, quotes and
        // a newline, and a second row that carries only a timecode.
        let text = format!(
            "name,comment,start_ticks,duration_ticks,start_timecode,label\n\"Intro, \"\"wide\"\"\",\"line1\nline2\",{},{},00:00:01:00,Blue\n\"Cue B\",\"\",,0,00:00:04:00,Rose\n",
            TICKS_PER_SECOND,
            TICKS_PER_SECOND / 2
        );
        let count = super::import_markers_csv_text(&mut app, &text).expect("the exported format imports");
        assert_eq!(count, 2, "both rows imported");

        {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.markers.len(), 2);
            let first = &q.markers[0];
            assert_eq!(first.name, "Intro, \"wide\"", "a quoted name keeps its comma and quotes");
            assert_eq!(first.comment, "line1\nline2", "a quoted comment keeps its newline");
            assert_eq!(first.start, Tick(TICKS_PER_SECOND), "start_ticks wins when both columns are present");
            assert_eq!(first.duration, Tick(TICKS_PER_SECOND / 2), "duration_ticks is read");
            assert_eq!(first.color, Label::Blue, "the label column names the colour");
            let second = &q.markers[1];
            assert_eq!(second.start, Tick(4 * TICKS_PER_SECOND), "a blank start_ticks falls back to the timecode");
            assert_eq!(second.color, Label::Rose);
        }

        app.undo();
        assert!(app.session.active_sequence().unwrap().markers.is_empty(), "one import is one undo step");

        let bad = "name,comment,start_ticks,duration_ticks,start_timecode,label\n\"Bad\",\"\",oops,0,,Green\n";
        let err = super::import_markers_csv_text(&mut app, bad).expect_err("a bad tick count is an error");
        assert!(err.contains("row 2"), "the error names the offending row: {err}");
        assert_eq!(app.session.active_sequence().unwrap().markers.len(), 0, "a bad row imports nothing");
    }

    #[test]
    fn marker_edl_import_reads_loc_lines_and_is_one_undo() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "MarkerEdl"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");

        // A real CMX 3600 cut list with Avid locator lines: the record timecodes
        // are an hour into the tape, so the hour is the base the markers drop.
        let text = "TITLE: Cut\nFCM: NON-DROP FRAME\n\n001  AX       V     C        00:00:00:00 00:00:01:00 01:00:00:00 01:00:01:00\n* FROM CLIP NAME: shot.mov\n* LOC: 01:00:02:00 BLUE Intro\n* LOC: 01:00:05:00 Wider shot\n";
        let count = super::import_markers_edl_text(&mut app, text).unwrap();
        assert_eq!(count, 2, "both locator lines import");
        let markers: Vec<(Tick, String, Label, String)> =
            app.sequence().unwrap().markers.iter().map(|m| (m.start, m.name.clone(), m.color, m.comment.clone())).collect();
        assert_eq!(markers[0].0, Tick(2 * TICKS_PER_SECOND), "the record timecode drops the EDL's hour");
        assert_eq!(markers[0].1, "Intro");
        assert_eq!(markers[0].2, Label::Blue, "BLUE maps onto the label palette");
        assert_eq!(markers[0].3, "", "an EDL locator has no comment column");
        assert_eq!(markers[1].0, Tick(5 * TICKS_PER_SECOND));
        assert_eq!(markers[1].1, "Wider shot");
        assert_eq!(markers[1].2, Label::Green, "a locator without a colour word defaults to green");
        assert_eq!(app.status, "Imported 2 markers");

        app.undo();
        assert!(app.sequence().unwrap().markers.is_empty(), "the whole EDL import is one undo step");
    }

    #[test]
    fn marker_edl_import_refuses_a_bad_locator_and_a_file_without_any() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "MarkerEdl"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");

        let err = super::import_markers_edl_text(&mut app, "TITLE: Cut\nFCM: NON-DROP FRAME\n").expect_err("a file with no locators is an error");
        assert!(err.contains("no `* LOC:`"), "the error says what is missing: {err}");

        let err = super::import_markers_edl_text(&mut app, "TITLE: Cut\n* LOC: 01:00:xx:00 BLUE bad\n").expect_err("a bad timecode is an error");
        assert!(err.starts_with("line 2:"), "the error names the offending line: {err}");
        assert!(app.sequence().unwrap().markers.is_empty(), "a bad locator imports nothing");
    }

    #[test]
    fn marker_csv_export_escapes_text_and_writes_the_file() {
        let mut s = Session::default();
        s.execute("file.newProject", json!({"name": "MarkerCsv"})).unwrap();
        s.execute("file.newSequence", json!({"name": "S1", "fps": 24})).unwrap();
        let mut app = crate::app::App::from_parts((*s.project).clone(), std::collections::HashMap::new(), None, "test");
        app.execute(
            "markers.add",
            json!({
                "time": TICKS_PER_SECOND,
                "durationFrames": 12,
                "name": "A, \"B\"",
                "comment": "line1\nline2",
                "color": "Blue"
            }),
        )
        .unwrap();

        let csv = {
            let q = app.session.active_sequence().unwrap();
            assert_eq!(q.markers.len(), 1);
            super::markers_csv(q)
        };
        let expected = format!(
            "name,comment,start_ticks,duration_ticks,start_timecode,label\n\"A, \"\"B\"\"\",\"line1\nline2\",{},{},00:00:01:00,Blue\n",
            TICKS_PER_SECOND,
            TICKS_PER_SECOND / 2
        );
        assert_eq!(csv, expected, "RFC 4180 quoting keeps the columns intact");

        let path = std::env::temp_dir().join(format!("frameforge-markers-{}.csv", std::process::id()));
        let written = super::write_markers_csv(&mut app, &path).unwrap();
        assert_eq!(written, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected, "the file has the tested bytes");
        let _ = std::fs::remove_file(&path);
        assert!(app.status.contains("Exported 1 markers"));
    }

    #[test]
    fn source_range_rects_place_the_handles_on_the_marks() {
        let bar = egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(400.0, 18.0));
        let bounds = filmcraft_time::TimeRange::from_bounds(Tick::ZERO, Tick(10 * TICKS_PER_SECOND));
        let range = filmcraft_time::TimeRange::from_bounds(Tick(2 * TICKS_PER_SECOND), Tick(6 * TICKS_PER_SECOND));

        let rects = super::source_range_rects(bar, bounds, range);

        // 2 s of 10 s is a fifth across a 400 px bar: x = 100 + 80.
        assert!((rects.in_handle.center().x - 180.0).abs() < 0.5, "the In handle sits on its mark: {}", rects.in_handle.center().x);
        // 6 s is three fifths across: x = 100 + 240.
        assert!((rects.out_handle.center().x - 340.0).abs() < 0.5, "the Out handle sits on its mark: {}", rects.out_handle.center().x);
        assert!(rects.body.left() >= rects.in_handle.right() - 0.01, "the body starts after the In handle");
        assert!(rects.body.right() <= rects.out_handle.left() + 0.01, "the body ends before the Out handle");
        assert_eq!((rects.in_handle.top(), rects.in_handle.bottom()), (bar.top(), bar.bottom()), "the handles use the full bar height");
    }

    #[test]
    fn the_selected_range_treats_out_as_inclusive_and_clamps_to_the_source() {
        let (start, end) = (Tick::ZERO, Tick(10 * TICKS_PER_SECOND));
        let fd = Tick(TICKS_PER_SECOND / 24);

        // No marks: the whole source, exactly as the engine's `SourceView` says.
        let full = super::selected_range(start, end, None, None, fd);
        assert_eq!((full.start, full.end()), (start, end));

        // Out is inclusive: marking frame 47 selects through frame 47, so the
        // exclusive end is one frame later.
        let some = super::selected_range(start, end, Some(Tick(2 * TICKS_PER_SECOND)), Some(Tick(6 * TICKS_PER_SECOND)), fd);
        assert_eq!((some.start, some.end()), (Tick(2 * TICKS_PER_SECOND), Tick(6 * TICKS_PER_SECOND) + fd));

        // A mark past the source clamps; an inverted pair cannot produce a
        // negative range (Out before In collapses to a zero-length span at In).
        let clamped = super::selected_range(start, end, Some(Tick::ZERO), Some(Tick(20 * TICKS_PER_SECOND)), fd);
        assert_eq!((clamped.start, clamped.end()), (start, end));
        let inverted = super::selected_range(start, end, Some(Tick(8 * TICKS_PER_SECOND)), Some(Tick(TICKS_PER_SECOND)), fd);
        assert_eq!((inverted.start, inverted.end()), (Tick(8 * TICKS_PER_SECOND), Tick(8 * TICKS_PER_SECOND)));
    }
}
