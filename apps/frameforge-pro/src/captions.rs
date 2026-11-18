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
//!
//! Import and export read and write a real caption file through the engine
//! (`captions.import` / `captions.export`): an import is one undoable edit, an
//! export is not an edit at all. Split cuts the selected cue at the playhead,
//! or every cue the playhead crosses when nothing is selected; Merge Up folds a
//! cue into the one before it and Merge Down into the one after, on the same
//! track.

use std::path::{Path, PathBuf};

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
    /// The track's BCP-47 language tag (`en`, `fr-CA`).
    pub language: String,
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
    /// The WebVTT voice span the caption carries, if any.
    pub speaker: Option<String>,
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
    /// Draft WebVTT voice for the selected caption.
    pub speaker: String,
    /// Draft colour for the hex row, in the engine's `#rrggbbaa` spelling.
    pub hex: String,
    /// Which of the style's three colours the hex row writes.
    pub hex_slot: ColorSlot,
    /// Draft name for the window's target track.
    pub name: String,
    /// Draft BCP-47 language tag for the window's target track.
    pub language: String,
    /// The track `name` and `language` were seeded from, so a target change
    /// re-reads them without clobbering a value the user is in the middle of
    /// typing.
    seeded_track: Option<u64>,
    /// Draft length in seconds for the next Add.
    pub seconds: f64,
    /// Draft start and length in seconds for the selected caption.
    pub start: f64,
    pub length: f64,
    /// The caption format an export writes: `srt` or `vtt`.
    pub format: String,
    /// Why the list could not be read, when it could not.
    pub error: Option<String>,
}

impl CaptionEditor {
    fn new() -> Self {
        Self {
            tracks: Vec::new(),
            captions: Vec::new(),
            track: None,
            selected: None,
            text: String::new(),
            speaker: String::new(),
            hex: String::new(),
            hex_slot: ColorSlot::Text,
            name: String::new(),
            language: String::new(),
            seeded_track: None,
            seconds: 3.0,
            start: 0.0,
            length: 3.0,
            format: "srt".to_string(),
            error: None,
        }
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
            seed_track(editor);
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

/// Seed the name and language drafts from the target track. Only a target
/// change re-reads them, so a value the user is typing survives the refreshes
/// the other commands run (an undo does not refresh the window, so the draft
/// keeps the last edit).
fn seed_track(editor: &mut CaptionEditor) {
    let Some(id) = editor.track else {
        editor.name.clear();
        editor.language.clear();
        editor.seeded_track = None;
        return;
    };
    if editor.seeded_track == Some(id) {
        return;
    }
    if let Some(row) = editor.tracks.iter().find(|t| t.id == id) {
        editor.name = row.name.clone();
        editor.language = row.language.clone();
        editor.seeded_track = Some(id);
    }
}

/// Point the draft fields at a row the document holds.
fn seed(editor: &mut CaptionEditor, row: &CaptionRow) {
    editor.text = row.text.clone();
    editor.speaker = row.speaker.clone().unwrap_or_default();
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
            language: track.get("language").and_then(Value::as_str).unwrap_or_default().to_string(),
            count: captions.len(),
        });
        for caption in captions {
            rows.push(CaptionRow {
                id: caption.get("id").and_then(Value::as_u64).unwrap_or_default(),
                track: id,
                in_tc: caption.get("in").and_then(Value::as_str).unwrap_or_default().to_string(),
                duration: caption.get("duration").and_then(Value::as_str).unwrap_or_default().to_string(),
                text: caption.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
                speaker: caption.get("speaker").and_then(Value::as_str).map(str::to_string),
                start: caption.get("start").and_then(Value::as_i64).unwrap_or_default(),
                end: caption.get("end").and_then(Value::as_i64).unwrap_or_default(),
            });
        }
    }
    rows.sort_by_key(|r| (r.start, r.id));
    let selection = value.get("selection").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
    (tracks, rows, selection)
}

/// Delete the window's target track and its cues as one undoable edit.
pub fn delete_track(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    app.execute("captions.deleteTrack", json!({"track": track}))?;
    // The target is gone: let the window fall back to the track that is left
    // rather than keep an id that no longer resolves.
    editor.track = None;
    editor.seeded_track = None;
    refresh(app, editor);
    app.status = "Deleted caption track".to_string();
    Ok(())
}

/// Add a subtitle-format caption track and make it the window's target.
pub fn add_track(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let value = app.execute("captions.newTrack", json!({"format": "Subtitle"}))?;
    let id = value.get("track").and_then(Value::as_u64);
    refresh(app, editor);
    if let Some(id) = id {
        editor.track = Some(id);
        // `refresh` seeded the old target; the new track's name is its own.
        seed_track(editor);
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
    // The engine's `speaker` is `str|null`: a blank field clears the WebVTT
    // voice span rather than writing an empty one.
    let speaker = editor.speaker.trim();
    let speaker = if speaker.is_empty() { Value::Null } else { json!(speaker) };
    app.execute("captions.setText", json!({"caption": id, "text": editor.text, "speaker": speaker}))?;
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

/// The window's target track, or an error when it has none.
fn target_track(editor: &CaptionEditor) -> Result<u64, String> {
    editor.track.ok_or_else(|| "select a caption track first".to_string())
}

/// The engine's current style for `track`.
fn style_of(app: &App, track: u64) -> Result<filmcraft_project::CaptionStyle, String> {
    let item =
        app.sequence().and_then(|sequence| sequence.caption_track(filmcraft_project::TrackId(track))).ok_or_else(|| "no such caption track".to_string())?;
    Ok(item.style.clone())
}

/// The engine's own name for an alignment.
fn align_name(align: filmcraft_project::CaptionAlign) -> &'static str {
    match align {
        filmcraft_project::CaptionAlign::Left => "left",
        filmcraft_project::CaptionAlign::Center => "center",
        filmcraft_project::CaptionAlign::Right => "right",
    }
}

/// The engine's own name for an anchor.
fn anchor_name(anchor: filmcraft_project::CaptionAnchor) -> &'static str {
    match anchor {
        filmcraft_project::CaptionAnchor::Top => "top",
        filmcraft_project::CaptionAnchor::Middle => "middle",
        filmcraft_project::CaptionAnchor::Bottom => "bottom",
    }
}

/// Step the target track's text alignment through the engine's three values.
pub fn cycle_align(app: &mut App, editor: &CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let current = style_of(app, track)?.align;
    let all = [filmcraft_project::CaptionAlign::Left, filmcraft_project::CaptionAlign::Center, filmcraft_project::CaptionAlign::Right];
    let next = all.iter().position(|align| *align == current).map_or(all[0], |index| all[(index + 1) % all.len()]);
    app.execute("captions.setStyle", json!({"track": track, "align": align_name(next)}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Step the target track's anchor through top, middle and bottom.
pub fn cycle_anchor(app: &mut App, editor: &CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let current = style_of(app, track)?.anchor;
    let all = [filmcraft_project::CaptionAnchor::Top, filmcraft_project::CaptionAnchor::Middle, filmcraft_project::CaptionAnchor::Bottom];
    let next = all.iter().position(|anchor| *anchor == current).map_or(all[0], |index| all[(index + 1) % all.len()]);
    app.execute("captions.setStyle", json!({"track": track, "anchor": anchor_name(next)}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Move the target track's text `delta` of the frame height from its edge.
pub fn step_margin(app: &mut App, editor: &CaptionEditor, delta: f32) -> Result<(), String> {
    let track = target_track(editor)?;
    let margin = style_of(app, track)?.margin;
    app.execute("captions.setStyle", json!({"track": track, "margin": (margin + delta) as f64}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Thicken or thin the target track's text outline by `delta` pixels.
pub fn step_outline(app: &mut App, editor: &CaptionEditor, delta: f32) -> Result<(), String> {
    let track = target_track(editor)?;
    let outline = style_of(app, track)?.outline;
    app.execute("captions.setStyle", json!({"track": track, "outline": (outline + delta) as f64}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Toggle the background box behind the target track's text.
pub fn toggle_background(app: &mut App, editor: &CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let background = style_of(app, track)?.background;
    app.execute("captions.setStyle", json!({"track": track, "background": !background}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Step the target track's font size by `delta` pixels (the engine clamps it).
pub fn step_size(app: &mut App, editor: &CaptionEditor, delta: f32) -> Result<(), String> {
    let track = target_track(editor)?;
    let size = style_of(app, track)?.size;
    app.execute("captions.setStyle", json!({"track": track, "size": (size + delta) as f64}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Return the target track to the engine's default caption style.
pub fn reset_style(app: &mut App, editor: &CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    app.execute("captions.setStyle", json!({"track": track, "reset": true}))?;
    app.status = "Reset caption style".to_string();
    Ok(())
}

/// The app's palettes for the caption text, its background box and its
/// outline. Each one starts on the engine's own default, so a fresh track
/// begins where the engine would put it and the first click moves off it.
const TEXT_COLORS: [[u8; 4]; 4] = [[255, 255, 255, 255], [255, 255, 0, 255], [0, 255, 255, 255], [16, 16, 16, 255]];
const BOX_COLORS: [[u8; 4]; 4] = [[0, 0, 0, 191], [0, 0, 0, 255], [32, 32, 32, 255], [255, 255, 255, 255]];
const OUTLINE_COLORS: [[u8; 4]; 4] = [[0, 0, 0, 255], [255, 255, 255, 255], [255, 255, 0, 255], [255, 0, 0, 255]];

/// Which of the style's three colours a control cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSlot {
    Text,
    Box,
    Outline,
}

impl ColorSlot {
    /// The next slot the hex row writes, in the chip's cycle order.
    fn next(self) -> Self {
        match self {
            Self::Text => Self::Box,
            Self::Box => Self::Outline,
            Self::Outline => Self::Text,
        }
    }

    /// The chip's label.
    fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Box => "Box",
            Self::Outline => "Outline",
        }
    }

    /// The `captions.setStyle` key this slot writes.
    fn key(self) -> &'static str {
        match self {
            Self::Text => "color",
            Self::Box => "backgroundColor",
            Self::Outline => "outlineColor",
        }
    }

    /// The slot's name in the status message.
    fn noun(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Box => "box",
            Self::Outline => "outline",
        }
    }
}

/// The engine's spelling of a colour: `#rrggbbaa`.
fn hex_color(color: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", color[0], color[1], color[2], color[3])
}

/// Step one of the target track's colours to the next palette entry.
pub fn cycle_color(app: &mut App, editor: &CaptionEditor, slot: ColorSlot) -> Result<(), String> {
    let track = target_track(editor)?;
    let style = style_of(app, track)?;
    let (palette, current, key) = match slot {
        ColorSlot::Text => (&TEXT_COLORS, style.color, "color"),
        ColorSlot::Box => (&BOX_COLORS, style.background_color, "backgroundColor"),
        ColorSlot::Outline => (&OUTLINE_COLORS, style.outline_color, "outlineColor"),
    };
    // A colour the palette does not know came from an import: start the cycle
    // over rather than jump to an arbitrary neighbour.
    let next = palette.iter().position(|color| *color == current).map_or(palette[0], |index| palette[(index + 1) % palette.len()]);
    let mut params = json!({"track": track});
    params[key] = json!(hex_color(next));
    app.execute("captions.setStyle", params)?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Parse the engine's colour spelling: `#rrggbb` or `#rrggbbaa`, with or
/// without the leading `#` and around whitespace. Alpha defaults to opaque.
fn parse_hex_color(text: &str) -> Result<[u8; 4], String> {
    let trimmed = text.trim();
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let invalid = || format!("not a colour: {trimmed} (use #rrggbb or #rrggbbaa)");
    if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| invalid());
    let alpha = if digits.len() == 8 { byte(6)? } else { 255 };
    Ok([byte(0)?, byte(2)?, byte(4)?, alpha])
}

/// Set one of the target track's three colours from the hex row's text, so a
/// colour does not have to come from a fixed palette.
pub fn set_hex_color(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let slot = editor.hex_slot;
    let color = parse_hex_color(&editor.hex)?;
    let mut params = json!({"track": track});
    params[slot.key()] = json!(hex_color(color));
    app.execute("captions.setStyle", params)?;
    // Show the applied value the way the engine spells it.
    editor.hex = hex_color(color);
    editor.error = None;
    app.status = format!("Caption {} colour set", slot.noun());
    Ok(())
}

/// The families the engine bundles, in cycle order. Every one of them really
/// renders, unlike a system name that may only fall back to Inter.
const FONT_FAMILIES: [&str; 3] = ["Inter", "JetBrains Mono", "Noto Serif"];

/// Step the target track's family through the engine's bundled faces.
pub fn cycle_font(app: &mut App, editor: &CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let font = style_of(app, track)?.font;
    // A family from an import that the cycle does not carry starts the cycle
    // over rather than jumping to an arbitrary neighbour.
    let next = FONT_FAMILIES.iter().position(|family| *family == font).map_or(FONT_FAMILIES[0], |index| FONT_FAMILIES[(index + 1) % FONT_FAMILIES.len()]);
    app.execute("captions.setStyle", json!({"track": track, "font": next}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// Step the target track's line spacing by `delta` multiples of the font size
/// (the engine clamps it to 0.8..4.0).
pub fn step_line_spacing(app: &mut App, editor: &CaptionEditor, delta: f32) -> Result<(), String> {
    let track = target_track(editor)?;
    let spacing = style_of(app, track)?.line_spacing;
    app.execute("captions.setStyle", json!({"track": track, "lineSpacing": (spacing + delta) as f64}))?;
    app.status = "Caption style updated".to_string();
    Ok(())
}

/// The engine's current format label for `track`.
fn format_of(app: &App, track: u64) -> Result<String, String> {
    let item =
        app.sequence().and_then(|sequence| sequence.caption_track(filmcraft_project::TrackId(track))).ok_or_else(|| "no such caption track".to_string())?;
    Ok(item.format.label().to_string())
}

/// The engine's current sync lock for `track`.
fn sync_lock_of(app: &App, track: u64) -> Result<bool, String> {
    let item =
        app.sequence().and_then(|sequence| sequence.caption_track(filmcraft_project::TrackId(track))).ok_or_else(|| "no such caption track".to_string())?;
    Ok(item.sync_lock)
}

/// Rename the window's target track through `captions.setTrack`.
pub fn rename(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let name = editor.name.trim().to_string();
    if name.is_empty() {
        return Err("a caption track needs a name".to_string());
    }
    app.execute("captions.setTrack", json!({"track": track, "name": name}))?;
    editor.name = name;
    refresh(app, editor);
    app.status = "Renamed caption track".to_string();
    Ok(())
}

/// Step the target track to the next engine caption format. The format is read
/// from the sequence document, so a cycle right after an undo is not stale.
pub fn cycle_format(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let current = format_of(app, track)?;
    let formats = filmcraft_project::CaptionFormat::ALL;
    let next = formats.iter().position(|format| format.label() == current.as_str()).map_or(formats[0], |index| formats[(index + 1) % formats.len()]);
    app.execute("captions.setTrack", json!({"track": track, "format": next.label()}))?;
    refresh(app, editor);
    app.status = format!("Caption format: {}", next.label());
    Ok(())
}

/// Set the window's target track's BCP-47 language tag through
/// `captions.setTrack`. The engine stores the tag verbatim, so the window
/// trims it and refuses an empty one rather than write an invalid tag.
pub fn set_language(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let language = editor.language.trim().to_string();
    if language.is_empty() {
        return Err("a caption track needs a language tag".to_string());
    }
    app.execute("captions.setTrack", json!({"track": track, "language": language}))?;
    editor.language = language;
    refresh(app, editor);
    app.status = "Caption language set".to_string();
    Ok(())
}

/// Flip the target track's sync lock. The lock is read from the sequence
/// document, so a toggle right after an undo is not stale.
pub fn toggle_sync(app: &mut App, editor: &mut CaptionEditor) -> Result<(), String> {
    let track = target_track(editor)?;
    let lock = !sync_lock_of(app, track)?;
    app.execute("captions.setTrack", json!({"track": track, "syncLock": lock}))?;
    refresh(app, editor);
    app.status = if lock { "Caption sync lock on".to_string() } else { "Caption sync lock off".to_string() };
    Ok(())
}

/// Turn every caption track's output on or off through the engine's bulk edit.
pub fn show_all(app: &mut App, editor: &mut CaptionEditor, on: bool) -> Result<(), String> {
    let command = if on { "captions.showAll" } else { "captions.hideAll" };
    app.execute(command, json!({}))?;
    refresh(app, editor);
    app.status = if on { "All caption tracks shown".to_string() } else { "All caption tracks hidden".to_string() };
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

/// Import a caption file into the active sequence as a new track. The engine
/// reads the file, detects its format and writes the track as one edit.
pub fn import_file(app: &mut App, editor: &mut CaptionEditor, path: &Path) -> Result<usize, String> {
    let value = app.execute("captions.import", json!({"path": path.to_string_lossy()}))?;
    let count = value.get("captions").and_then(Value::as_u64).unwrap_or_default() as usize;
    refresh(app, editor);
    // `refresh` keeps a still-valid old target; the import is the new target.
    if let Some(track) = value.get("track").and_then(Value::as_u64) {
        editor.track = Some(track);
        seed_track(editor);
    }
    let plural = if count == 1 { "" } else { "s" };
    app.status = format!("Imported {count} caption{plural}");
    Ok(count)
}

/// Export the window's track to `path` in the window's chosen format. An
/// export writes a file but does not edit the project.
pub fn export_file(app: &mut App, editor: &mut CaptionEditor, path: PathBuf) -> Result<usize, String> {
    let track = editor.track.ok_or_else(|| "add a caption track before exporting".to_string())?;
    // An export is not an edit: run it on the session directly so the write
    // adds no undo step and never dirties the project.
    let value =
        app.session.execute("captions.export", json!({"path": path.to_string_lossy(), "track": track, "format": editor.format})).map_err(|e| e.to_string())?;
    let count = value.get("captions").and_then(Value::as_u64).unwrap_or_default() as usize;
    let plural = if count == 1 { "" } else { "s" };
    app.status = format!("Exported {count} caption{plural}");
    Ok(count)
}

/// Split at the playhead and select the new right half. With a caption
/// selected that is the selected cue; with nothing selected the engine cuts
/// every unlocked track's cue the playhead crosses, which is the behaviour a
/// bare Split should have.
pub fn split_at_playhead(app: &mut App, editor: &mut CaptionEditor) -> Result<u64, String> {
    let t = app.playhead.0;
    let split_all = editor.selected.is_none();
    let params = match editor.selected {
        Some(id) => json!({"caption": id, "time": t}),
        None => {
            if !editor.captions.iter().any(|row| row.start < t && t < row.end) {
                return Err("no caption crosses the playhead".to_string());
            }
            json!({"time": t})
        }
    };
    let value = app.execute("captions.split", params)?;
    let right = value
        .get("captions")
        .and_then(Value::as_array)
        .and_then(|captions| captions.first())
        .and_then(Value::as_u64)
        .ok_or_else(|| "the engine did not report the new caption".to_string())?;
    refresh(app, editor);
    editor.selected = Some(right);
    if let Some(row) = editor.captions.iter().find(|r| r.id == right).cloned() {
        seed(editor, &row);
    }
    app.status = if split_all { "Split captions at the playhead".to_string() } else { "Split caption at the playhead".to_string() };
    Ok(right)
}

/// Merge the selected caption into the one before it on the same track.
pub fn merge_with_previous(app: &mut App, editor: &mut CaptionEditor) -> Result<u64, String> {
    let id = editor.selected.ok_or_else(|| "select a caption first".to_string())?;
    let previous = previous_caption(editor, id).ok_or_else(|| "the first caption on its track has nothing to merge up into".to_string())?;
    let value = app.execute("captions.merge", json!({"captions": [previous, id]}))?;
    let kept = value.get("caption").and_then(Value::as_u64).unwrap_or(previous);
    refresh(app, editor);
    editor.selected = Some(kept);
    if let Some(row) = editor.captions.iter().find(|r| r.id == kept).cloned() {
        seed(editor, &row);
    }
    app.status = "Merged caption up".to_string();
    Ok(kept)
}

/// The caption on the same track immediately before `id`, in timeline order.
fn previous_caption(editor: &CaptionEditor, id: u64) -> Option<u64> {
    let index = editor.captions.iter().position(|r| r.id == id)?;
    let track = editor.captions[index].track;
    editor.captions[..index].iter().rev().find(|r| r.track == track).map(|r| r.id)
}

/// The caption on the same track immediately after `id`, in timeline order.
fn next_caption(editor: &CaptionEditor, id: u64) -> Option<u64> {
    let index = editor.captions.iter().position(|r| r.id == id)?;
    let track = editor.captions[index].track;
    editor.captions[index + 1..].iter().find(|r| r.track == track).map(|r| r.id)
}

/// Merge the selected caption with the one after it on the same track, keeping
/// the selected cue (the merge result is the earlier of the two).
pub fn merge_with_next(app: &mut App, editor: &mut CaptionEditor) -> Result<u64, String> {
    let id = editor.selected.ok_or_else(|| "select a caption first".to_string())?;
    let next = next_caption(editor, id).ok_or_else(|| "the last caption on its track has nothing to merge down into".to_string())?;
    let value = app.execute("captions.merge", json!({"captions": [id, next]}))?;
    let kept = value.get("caption").and_then(Value::as_u64).unwrap_or(id);
    refresh(app, editor);
    editor.selected = Some(kept);
    if let Some(row) = editor.captions.iter().find(|r| r.id == kept).cloned() {
        seed(editor, &row);
    }
    app.status = "Merged caption down".to_string();
    Ok(kept)
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

/// The same button as `action`, but greyed out (and unpickable) when the
/// selected caption cannot take the edit yet.
fn action_enabled(ui: &mut egui::Ui, id: &str, label: &str, accent: bool, enabled: bool) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| action(ui, id, label, accent)).inner
}

/// Draw the editor when one is open. Called every frame from `App::ui_frame`,
/// so it floats over the timeline whose document it edits.
pub fn show(app: &mut App, ui: &egui::Ui) {
    let Some(mut editor) = app.captions.take() else { return };
    let mut close = false;
    let mut command: Option<Result<(), String>> = None;
    // The place row's labels read the document, like the style commands do.
    let style = editor.track.and_then(|id| style_of(app, id).ok());
    // The sync control shows the document's live lock, not a cached row.
    let sync = editor.track.and_then(|id| sync_lock_of(app, id).ok());
    egui::Window::new("Captions")
        .id(egui::Id::new("ff-caption-window"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 84.0))
        // A fixed size, not an auto-sized anchored window: a contents-filling
        // scroll area inside an auto-sized window hands its own size back and
        // the window resizes every other frame, so a click would land where a
        // control was read rather than where it is.
        .fixed_size(egui::vec2(460.0, 720.0))
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
                if action(ui, "ff-caption-delete-track", "Delete Track", false).on_hover_text("Delete the target track and its cues (undoable)").clicked() {
                    command = Some(delete_track(app, &mut editor));
                }
                if action(ui, "ff-caption-show-all", "Show All", false).on_hover_text("Turn every caption track's output on").clicked() {
                    command = Some(show_all(app, &mut editor, true));
                }
                if action(ui, "ff-caption-hide-all", "Hide All", false).on_hover_text("Turn every caption track's output off").clicked() {
                    command = Some(show_all(app, &mut editor, false));
                }
            });
            ui.horizontal(|ui| {
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
            if editor.track.is_some() {
                let format = editor.tracks.iter().find(|track| Some(track.id) == editor.track).map(|track| track.format.clone()).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(ui::hint("Track"));
                    ui.add(egui::TextEdit::singleline(&mut editor.name).id(egui::Id::new("ff-caption-name")).desired_width(150.0).hint_text("Track name"));
                    if action(ui, "ff-caption-rename", "Rename", false).clicked() {
                        command = Some(rename(app, &mut editor));
                        // A commit ends the edit, so hand focus back: otherwise
                        // the field's own Ctrl+Z would swallow the app's undo.
                        ui.ctx().memory_mut(|memory| memory.surrender_focus(egui::Id::new("ff-caption-name")));
                    }
                    if action(ui, "ff-caption-format-cycle", &format!("Format: {format}"), false)
                        .on_hover_text("Cycle Subtitle, CEA-608, CEA-708, Teletext")
                        .clicked()
                    {
                        command = Some(cycle_format(app, &mut editor));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(ui::hint("Lang"));
                    ui.add(
                        egui::TextEdit::singleline(&mut editor.language).id(egui::Id::new("ff-caption-language")).desired_width(70.0).hint_text("BCP-47 tag"),
                    );
                    if action(ui, "ff-caption-language-set", "Set", false).on_hover_text("Set the track's BCP-47 language tag").clicked() {
                        command = Some(set_language(app, &mut editor));
                        // A commit ends the edit, so hand focus back: otherwise
                        // the field's own Ctrl+Z would swallow the app's undo.
                        ui.ctx().memory_mut(|memory| memory.surrender_focus(egui::Id::new("ff-caption-language")));
                    }
                    if let Some(sync) = sync
                        && action(ui, "ff-caption-sync", if sync { "Sync on" } else { "Sync off" }, false)
                            .on_hover_text("Follow inserts and extracts on other tracks")
                            .clicked()
                    {
                        command = Some(toggle_sync(app, &mut editor));
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label(ui::hint("Style"));
                if action(ui, "ff-caption-bg", "BG", false).on_hover_text("Toggle the background box").clicked() {
                    command = Some(toggle_background(app, &editor));
                }
                if action(ui, "ff-caption-size-down", "A-", false).on_hover_text("Smaller caption text").clicked() {
                    command = Some(step_size(app, &editor, -4.0));
                }
                if action(ui, "ff-caption-size-up", "A+", false).on_hover_text("Larger caption text").clicked() {
                    command = Some(step_size(app, &editor, 4.0));
                }
                if action(ui, "ff-caption-style-reset", "Reset", false).on_hover_text("Back to the engine's default style").clicked() {
                    command = Some(reset_style(app, &editor));
                }
            });
            ui.horizontal(|ui| {
                ui.label(ui::hint("Colour"));
                if style.is_some() {
                    if action(ui, "ff-caption-text-color", "Text", false).on_hover_text("Cycle the caption text colour").clicked() {
                        command = Some(cycle_color(app, &editor, ColorSlot::Text));
                    }
                    if action(ui, "ff-caption-box-color", "Box", false).on_hover_text("Cycle the background box colour").clicked() {
                        command = Some(cycle_color(app, &editor, ColorSlot::Box));
                    }
                    if action(ui, "ff-caption-outline-color", "Outline", false).on_hover_text("Cycle the outline colour").clicked() {
                        command = Some(cycle_color(app, &editor, ColorSlot::Outline));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(ui::hint("Hex"));
                if style.is_some() {
                    if action(ui, "ff-caption-hex-slot", editor.hex_slot.label(), false)
                        .on_hover_text("Which of the three colours the hex field sets")
                        .clicked()
                    {
                        editor.hex_slot = editor.hex_slot.next();
                    }
                    ui.add(egui::TextEdit::singleline(&mut editor.hex).id(egui::Id::new("ff-caption-hex")).desired_width(84.0).hint_text("#rrggbbaa"));
                    if action(ui, "ff-caption-hex-set", "Set", false).on_hover_text("Set this colour from its hex value").clicked() {
                        command = Some(set_hex_color(app, &mut editor));
                        // A commit ends the edit, so hand focus back: otherwise
                        // the field's own Ctrl+Z would swallow the app's undo.
                        ui.ctx().memory_mut(|memory| memory.surrender_focus(egui::Id::new("ff-caption-hex")));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(ui::hint("Type"));
                if let Some(style) = &style {
                    if action(ui, "ff-caption-font", &format!("Font: {}", style.font), false)
                        .on_hover_text("Inter, JetBrains Mono, Noto Serif - the bundled faces")
                        .clicked()
                    {
                        command = Some(cycle_font(app, &editor));
                    }
                    if action(ui, "ff-caption-spacing-down", "L-", false).on_hover_text("Tighter line spacing").clicked() {
                        command = Some(step_line_spacing(app, &editor, -0.05));
                    }
                    if action(ui, "ff-caption-spacing-up", "L+", false).on_hover_text("Looser line spacing").clicked() {
                        command = Some(step_line_spacing(app, &editor, 0.05));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(ui::hint("Place"));
                if let Some(style) = &style {
                    if action(ui, "ff-caption-align", &format!("Align: {}", align_name(style.align)), false).on_hover_text("Left, centre, right").clicked() {
                        command = Some(cycle_align(app, &editor));
                    }
                    if action(ui, "ff-caption-anchor", &format!("Anchor: {}", anchor_name(style.anchor)), false).on_hover_text("Top, middle, bottom").clicked()
                    {
                        command = Some(cycle_anchor(app, &editor));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(ui::hint("Edge"));
                if style.is_some() {
                    if action(ui, "ff-caption-margin-down", "M-", false).on_hover_text("Move the text closer to the anchored edge").clicked() {
                        command = Some(step_margin(app, &editor, -0.02));
                    }
                    if action(ui, "ff-caption-margin-up", "M+", false).on_hover_text("Move the text further from the anchored edge").clicked() {
                        command = Some(step_margin(app, &editor, 0.02));
                    }
                    if action(ui, "ff-caption-outline-down", "O-", false).on_hover_text("Thinner text outline").clicked() {
                        command = Some(step_outline(app, &editor, -2.0));
                    }
                    if action(ui, "ff-caption-outline-up", "O+", false).on_hover_text("Thicker text outline").clicked() {
                        command = Some(step_outline(app, &editor, 2.0));
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(ui::hint("Speaker"));
                ui.add(
                    egui::TextEdit::singleline(&mut editor.speaker)
                        .id(egui::Id::new("ff-caption-speaker"))
                        .desired_width(220.0)
                        .hint_text("WebVTT voice (optional)"),
                );
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut editor.text).id(egui::Id::new("ff-caption-text")).desired_width(220.0).hint_text("Caption text"));
                ui.add(egui::DragValue::new(&mut editor.seconds).speed(0.1).range(0.1..=60.0).suffix(" s"));
                if action(ui, "ff-caption-add", "Add", true).clicked() {
                    command = Some(add_caption(app, &mut editor));
                }
            });
            ui.horizontal(|ui| {
                if action(ui, "ff-caption-split", "Split at Playhead", false).clicked() {
                    command = Some(split_at_playhead(app, &mut editor).map(|_| ()));
                }
                if action(ui, "ff-caption-merge", "Merge Up", false).clicked() {
                    command = Some(merge_with_previous(app, &mut editor).map(|_| ()));
                }
                if action(ui, "ff-caption-merge-down", "Merge Down", false).clicked() {
                    command = Some(merge_with_next(app, &mut editor).map(|_| ()));
                }
            });
            ui.horizontal(|ui| {
                let enabled = editor.selected.is_some();
                ui.label(ui::hint("Start"));
                ui.add_enabled(enabled, egui::DragValue::new(&mut editor.start).speed(0.1).range(0.0..=3600.0).suffix(" s"));
                ui.label(ui::hint("Length"));
                ui.add_enabled(enabled, egui::DragValue::new(&mut editor.length).speed(0.1).range(0.1..=60.0).suffix(" s"));
                if action_enabled(ui, "ff-caption-apply", "Apply", false, enabled).clicked() {
                    // Only write the times when they changed: an unchanged timing
                    // edit still pushes its own undo step, so Apply would take two
                    // Ctrl+Z to undo instead of one.
                    let times = editor
                        .selected
                        .and_then(|id| editor.captions.iter().find(|row| row.id == id))
                        .map(|row| (row.start as f64 / TICKS_PER_SECOND as f64, (row.end - row.start) as f64 / TICKS_PER_SECOND as f64));
                    let changed = times.is_none_or(|(start, length)| (editor.start - start).abs() > 1e-9 || (editor.length - length).abs() > 1e-9);
                    command = Some(apply_text(app, &mut editor).and_then(|()| if changed { apply_times(app, &mut editor) } else { Ok(()) }));
                    // A commit ends the edit, so hand both fields' focus back:
                    // otherwise the field's own Ctrl+Z would swallow the app's undo.
                    ui.ctx().memory_mut(|memory| {
                        memory.surrender_focus(egui::Id::new("ff-caption-text"));
                        memory.surrender_focus(egui::Id::new("ff-caption-speaker"));
                    });
                }
                if action_enabled(ui, "ff-caption-delete", "Delete", false, enabled).clicked() {
                    command = Some(delete(app, &mut editor));
                }
            });
            ui.horizontal(|ui| {
                ui.label(ui::hint("Format"));
                let label = if editor.format == "vtt" { "WebVTT" } else { "SubRip (SRT)" };
                egui::ComboBox::from_id_salt("ff-caption-format").selected_text(label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor.format, "srt".to_string(), "SubRip (SRT)");
                    ui.selectable_value(&mut editor.format, "vtt".to_string(), "WebVTT");
                });
                if action(ui, "ff-caption-import", "Import...", false).clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("Captions", &["srt", "vtt", "scc", "mcc", "stl", "ttml", "dfxp"]).pick_file()
                {
                    command = Some(import_file(app, &mut editor, &path).map(|_| ()));
                }
                if action(ui, "ff-caption-export", "Export...", false).clicked() {
                    let extension = editor.format.clone();
                    if let Some(path) = rfd::FileDialog::new().add_filter(extension.to_uppercase(), &[extension.as_str()]).save_file() {
                        command = Some(export_file(app, &mut editor, path).map(|_| ()));
                    }
                }
            });
            if let Some(error) = editor.error.clone() {
                ui.label(egui::RichText::new(error).size(10.0).color(ui::TEXT_LO));
            }
            ui.separator();
            if editor.captions.is_empty() {
                ui.label(ui::hint("No captions yet."));
            } else {
                // The row handler runs after the scroll area: a click needs
                // `&mut editor`, which the row list is borrowing to draw.
                let mut clicked = None;
                egui::ScrollArea::vertical().id_salt("caption-list").max_height(120.0).auto_shrink([false, false]).show(ui, |ui| {
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

    /// A uniquely named file in the system temp directory, with `text` in it.
    fn temp_file(name: &str, text: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        let path = std::env::temp_dir().join(format!("ff-caption-{stamp}-{name}"));
        std::fs::write(&path, text).expect("the fixture file is written");
        path
    }

    #[test]
    fn the_editor_lists_the_engines_own_tracks_and_captions_in_time_order() {
        let mut app = caption_app();
        let created = app.session.execute("captions.newTrack", json!({"name": "Dialogue"})).unwrap();
        let first = created["track"].as_u64().unwrap();
        let created = app.session.execute("captions.newTrack", json!({"name": "Notes"})).unwrap();
        let second = created["track"].as_u64().unwrap();
        let mut added = Vec::new();
        for (track, text, seconds) in [(first, "later", 4.0), (first, "earlier", 1.0), (second, "on the other track", 2.0)] {
            let value = app.session.execute("captions.add", json!({"track": track, "text": text, "seconds": seconds, "durationSeconds": 1.5})).unwrap();
            added.push(value["caption"].as_u64().unwrap());
        }
        let list = app.session.execute("captions.list", json!({})).unwrap();
        let (tracks, rows, selection) = rows_from_list(&list);
        assert_eq!(
            tracks.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
            ["C1", "C2"],
            "the engine's slot labels, top first (a new track is inserted at the top)"
        );
        assert_eq!(tracks.iter().map(|t| t.count).collect::<Vec<_>>(), [1, 2], "each track counts its own captions");
        assert_eq!(tracks.iter().map(|t| t.language.as_str()).collect::<Vec<_>>(), ["en", "en"], "a new track carries the engine's language default");
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["earlier", "on the other track", "later"], "captions read in timeline order");
        assert_eq!(rows[0].in_tc, "00:00:01:00", "the engine's own timecode at 24 fps");
        assert_eq!(rows[0].duration, "00:00:01:12", "a 1.5 s caption is 36 frames");
        assert_eq!(selection, vec![added[2]], "each `captions.add` selects the caption it wrote, so the last one is selected");
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

    #[test]
    fn importing_an_srt_adds_a_track_named_for_the_file_and_one_undo_removes_it() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        let path = temp_file("import.srt", "1\n00:00:01,000 --> 00:00:02,500\nFirst line\n\n2\n00:00:03,000 --> 00:00:04,000\nSecond line\n");
        let undo_before = app.session.history.undo.len();

        let imported = import_file(&mut app, &mut editor, &path).expect("the caption file imports");
        assert_eq!(imported, 2, "the reply counts the cues that landed");
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        assert_eq!(track_count(&app), 1, "the import adds one caption track");
        assert_eq!(editor.tracks.len(), 1, "the editor lists it");
        assert_eq!(editor.tracks[0].name, stem, "the track is named for the file");
        assert_eq!(
            caption_list(&app),
            vec![(frames(24), frames(60), "First line".to_string()), (frames(72), frames(96), "Second line".to_string()),],
            "the cues land on the frame grid with their own times and text"
        );
        assert_eq!(app.session.history.undo.len(), undo_before + 1, "the whole import is one undo step");
        assert!(app.session.state.caption_selection.is_empty(), "the engine clears the caption selection on import");
        assert_eq!(editor.selected, None, "so the window shows nothing selected");
        assert_eq!(editor.track, Some(editor.tracks[0].id), "the imported track is the window's target");

        app.undo();
        assert_eq!(track_count(&app), 0, "one undo removes the imported track and its cues");
    }

    #[test]
    fn exporting_webvtt_writes_the_cues_without_dirtying_the_project_and_reimports() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        add_caption(&mut app, &mut editor).expect("the caption is added");
        editor.format = "vtt".to_string();
        // The caption edit dirtied the project; the export must not set it
        // again (and must not add a step either).
        app.dirty = false;
        let undo_before = app.session.history.undo.len();
        let path = temp_file("export.vtt", "");
        let exported = export_file(&mut app, &mut editor, path.clone()).expect("the track exports");

        assert_eq!(exported, 1, "the reply counts the cues written");
        let text = std::fs::read_to_string(&path).expect("the export wrote a file");
        assert!(text.starts_with("WEBVTT\n\n"), "a WebVTT file begins with its signature: {text:?}");
        assert!(text.contains("00:00:00.000 --> 00:00:03.000"), "the cue's own times, in WebVTT's clock: {text:?}");
        assert!(text.contains("New caption"), "the cue's text: {text:?}");
        assert_eq!(app.session.history.undo.len(), undo_before, "an export is not an edit");
        assert!(!app.dirty, "an export does not dirty the project");

        // The file the engine wrote is a caption file the engine reads back.
        let mut other = caption_app();
        open(&mut other);
        let mut other_editor = other.captions.clone().expect("the editor opens");
        let reimported = import_file(&mut other, &mut other_editor, &path).expect("the exported file imports");
        assert_eq!(reimported, 1, "one cue round-trips");
        assert_eq!(caption_list(&other), vec![(0, frames(72), "New caption".to_string())], "the same cue, back on the frame grid");
    }

    #[test]
    fn a_file_that_is_not_a_caption_errors_with_its_name_and_leaves_the_sequence_alone() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        let path = temp_file("notes.txt", "this is not a caption file\n");
        let undo_before = app.session.history.undo.len();

        let error = import_file(&mut app, &mut editor, &path).expect_err("a text file is not a caption file");
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        assert!(error.contains(&name), "the error names the file: {error}");
        assert_eq!(track_count(&app), 0, "a bad file adds no track");
        assert_eq!(app.session.history.undo.len(), undo_before, "a bad file adds no undo step");
    }

    #[test]
    fn splitting_the_selected_caption_at_the_playhead_is_one_undo() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        add_caption(&mut app, &mut editor).expect("the caption is added");
        let id = editor.selected.expect("the new caption is selected");
        app.playhead = Tick(TICKS_PER_SECOND);

        let right = split_at_playhead(&mut app, &mut editor).expect("the caption splits at the playhead");
        assert_ne!(right, id, "the reply names the new right-hand cue");
        assert_eq!(
            caption_list(&app),
            vec![(0, frames(24), "New caption".to_string()), (frames(24), frames(72), "New caption".to_string())],
            "the 3 s cue is cut at the 1 s playhead, both halves on the frame grid"
        );
        assert_eq!(editor.selected, Some(right), "the window follows the engine and selects the right half");
        assert_eq!(
            app.session.state.caption_selection.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![right],
            "the engine's caption selection names the right half"
        );

        app.undo();
        assert_eq!(caption_list(&app), vec![(0, frames(72), "New caption".to_string())], "one undo restores the whole cue");
    }

    #[test]
    fn merging_the_selected_caption_up_folds_it_into_the_earlier_cue_as_one_undo() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        let track = editor.track.expect("the new track is the target");
        app.execute("captions.add", json!({"track": track, "text": "one", "time": 0, "durationSeconds": 1.5})).expect("the first cue is added");
        app.execute("captions.add", json!({"track": track, "text": "two", "time": frames(36), "durationSeconds": 1.5})).expect("the second cue is added");
        refresh(&mut app, &mut editor);
        let first = editor.captions[0].id;
        let second = editor.captions[1].id;
        select(&mut app, &mut editor, second).expect("the second cue is selected");

        let kept = merge_with_previous(&mut app, &mut editor).expect("the cue merges into the one before it");
        assert_eq!(kept, first, "the earlier cue keeps the merged result");
        assert_eq!(caption_list(&app), vec![(0, frames(72), "one\ntwo".to_string())], "the merged cue spans both cues and joins the texts with a line break");
        assert_eq!(
            app.session.state.caption_selection.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![kept],
            "the engine's caption selection names the merged cue"
        );

        app.undo();
        assert_eq!(caption_list(&app), vec![(0, frames(36), "one".to_string()), (frames(36), frames(72), "two".to_string())], "one undo restores both cues");
    }

    #[test]
    fn splitting_at_the_playhead_with_no_cue_selected_cuts_every_cue_it_crosses() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        add_caption(&mut app, &mut editor).expect("the caption is added");
        // Drop the selection: a bare Split is about the playhead, not one cue.
        editor.selected = None;
        app.playhead = Tick(TICKS_PER_SECOND);

        let right = split_at_playhead(&mut app, &mut editor).expect("the playhead split works without a selection");
        assert_eq!(
            caption_list(&app),
            vec![(0, frames(24), "New caption".to_string()), (frames(24), frames(72), "New caption".to_string())],
            "the cue the playhead crosses is cut in two"
        );
        assert_eq!(app.status, "Split captions at the playhead", "the plural says no single cue was meant");
        assert_eq!(editor.selected, Some(right), "the window selects the new right half");

        app.undo();
        assert_eq!(caption_list(&app), vec![(0, frames(72), "New caption".to_string())], "one undo restores the whole cue");
    }

    #[test]
    fn merging_the_selected_caption_down_folds_the_next_cue_into_it() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        let track = editor.track.expect("the new track is the target");
        app.execute("captions.add", json!({"track": track, "text": "one", "time": 0, "durationSeconds": 1.5})).expect("the first cue is added");
        app.execute("captions.add", json!({"track": track, "text": "two", "time": frames(36), "durationSeconds": 1.5})).expect("the second cue is added");
        refresh(&mut app, &mut editor);
        let first = editor.captions[0].id;
        select(&mut app, &mut editor, first).expect("the first cue is selected");

        let kept = merge_with_next(&mut app, &mut editor).expect("the cue merges into the one after it");
        assert_eq!(kept, first, "the selected cue keeps the merged result");
        assert_eq!(caption_list(&app), vec![(0, frames(72), "one\ntwo".to_string())], "the merged cue spans both cues and joins the texts with a line break");
        assert_eq!(app.status, "Merged caption down");
        assert_eq!(
            app.session.state.caption_selection.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![kept],
            "the engine's caption selection names the merged cue"
        );

        app.undo();
        assert_eq!(caption_list(&app), vec![(0, frames(36), "one".to_string()), (frames(36), frames(72), "two".to_string())], "one undo restores both cues");
    }

    #[test]
    fn setting_a_caption_colour_from_hex_writes_the_style() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        let track = editor.track.expect("the new track is the target");
        let before = style_of(&app, track).expect("the style reads");
        assert_eq!(before.color, TEXT_COLORS[0], "a new track starts on the palette's first text colour");
        let undo = app.session.history.undo.len();

        editor.hex_slot = ColorSlot::Text;
        editor.hex = "  #3366ff  ".to_string();
        set_hex_color(&mut app, &mut editor).expect("six hex digits set the colour");
        assert_eq!(style_of(&app, track).expect("the style reads").color, [0x33, 0x66, 0xff, 0xff], "a six-digit colour is opaque");
        assert_eq!(app.status, "Caption text colour set");
        assert_eq!(app.session.history.undo.len(), undo + 1, "one undo step");

        // Eight digits keep their alpha, and each slot writes only its own key.
        editor.hex_slot = ColorSlot::Box;
        editor.hex = "#11223344".to_string();
        set_hex_color(&mut app, &mut editor).expect("eight hex digits set the colour");
        let style = style_of(&app, track).expect("the style reads");
        assert_eq!(style.background_color, [0x11, 0x22, 0x33, 0x44], "the alpha is kept");
        assert_eq!(style.color, [0x33, 0x66, 0xff, 0xff], "the box slot leaves the text colour alone");
        assert_eq!(app.status, "Caption box colour set");

        editor.hex_slot = ColorSlot::Outline;
        editor.hex = "#00ff00".to_string();
        set_hex_color(&mut app, &mut editor).expect("the outline slot sets its own colour");
        assert_eq!(style_of(&app, track).expect("the style reads").outline_color, [0, 255, 0, 255], "the outline colour is written");
        assert_eq!(app.status, "Caption outline colour set");

        for _ in 0..3 {
            app.undo();
        }
        let restored = style_of(&app, track).expect("the style reads");
        assert_eq!(
            (restored.color, restored.background_color, restored.outline_color),
            (before.color, before.background_color, before.outline_color),
            "three Ctrl+Z restore all three colours"
        );
    }

    #[test]
    fn a_malformed_hex_colour_is_refused_without_an_edit() {
        let mut app = caption_app();
        open(&mut app);
        let mut editor = app.captions.clone().expect("the editor opens");
        add_track(&mut app, &mut editor).expect("the track is added");
        let track = editor.track.expect("the new track is the target");
        let before = style_of(&app, track).expect("the style reads");
        let undo = app.session.history.undo.len();

        editor.hex = "blue".to_string();
        let error = set_hex_color(&mut app, &mut editor).expect_err("a word is not a colour");
        assert!(error.starts_with("not a colour"), "the message says what is wrong (got {error:?})");
        editor.hex = "#12345".to_string();
        let error = set_hex_color(&mut app, &mut editor).expect_err("five digits are not a colour");
        assert!(error.starts_with("not a colour"), "the message says what is wrong (got {error:?})");

        assert_eq!(style_of(&app, track).expect("the style reads"), before, "a refused colour changes nothing");
        assert_eq!(app.session.history.undo.len(), undo, "a refused colour adds no undo step");
    }
}
