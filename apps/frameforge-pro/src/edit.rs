//! The Edit workspace: the project panel (media items), the program monitor
//! (real frames through the engine) and the timeline (real clips, real
//! playhead, real engine commands). Laid out by rect — this egui generation
//! has no panel helpers.

use filmcraft_engine::Targeting;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{Generator, MediaSource};
use filmcraft_project::{ClipId, ItemId, ItemKind, Label, MediaClip, MediaRef, Project, Sequence, TrackId};
use filmcraft_time::TICKS_PER_SECOND;
use filmcraft_time::Tick;

use crate::app::{App, Monitor, SEQUENCE_H, SEQUENCE_RATE, SEQUENCE_W, TimelineDrag};
use crate::pixels;

const ROW_H: f32 = 34.0;
const RULER_H: f32 = 18.0;
const TIMELINE_HEADER_H: f32 = 54.0;
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

/// Everything a move or trim can snap to: zero, the playhead and every clip
/// edge except the one being dragged.
fn snap_targets(seq: &Sequence, exclude: Option<filmcraft_project::ClipId>, playhead: Tick) -> Vec<Tick> {
    let mut targets = vec![Tick::ZERO, playhead.max(Tick::ZERO)];
    for track in seq.video_tracks.iter().chain(&seq.audio_tracks) {
        for item in &track.items {
            if Some(item.id) == exclude {
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
    if (by_start.0 - raw.0).abs() <= (by_end.0 - raw.0).abs() { by_start } else { by_end }
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
    let project_w = app.project_w.clamp(180.0, body.width() - 260.0);
    let timeline_h = app.timeline_h.clamp(120.0, body.height() - 150.0);

    let project = egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + project_w, body.max.y));
    let rest = egui::Rect::from_min_max(egui::pos2(body.min.x + project_w, body.min.y), body.max);
    let timeline = egui::Rect::from_min_max(egui::pos2(rest.min.x, rest.max.y - timeline_h), rest.max);
    let monitor = egui::Rect::from_min_max(rest.min, egui::pos2(rest.max.x, timeline.min.y));

    let source_w = app.source_w.clamp(220.0, (monitor.width() - 220.0).max(220.0));
    let program = egui::Rect::from_min_max(monitor.min, egui::pos2(monitor.max.x - source_w, monitor.max.y));
    let source = egui::Rect::from_min_max(egui::pos2(program.max.x, monitor.min.y), monitor.max);

    project_panel(app, ui, project);
    crate::app::gutter_v(ui, body.min.x + project_w, body.min.y, body.max.y, &mut app.project_w);
    monitor_panel(app, ui, program);
    crate::app::gutter_v(ui, program.max.x, monitor.min.y, monitor.max.y, &mut app.source_w);
    source_monitor_panel(app, ui, source);
    crate::app::gutter_h(ui, monitor.max.y, rest.min.x, rest.max.x, &mut app.timeline_h);
    timeline_panel(app, ui, timeline);
}

/// Generators the engine has commands for (undoable, with the registry's
/// validation and defaults).
fn add_generator(app: &mut App, command: &str) {
    match app.execute(command, serde_json::json!({"seconds": GENERATOR_SECONDS})) {
        Ok(_) => app.status = "Media added".to_string(),
        Err(e) => app.status = e,
    }
}

/// Tone has no engine command yet: a real, undoable `Session::edit` closure —
/// the engine's own pattern for custom edits.
fn add_tone(app: &mut App) {
    let hz = 440.0_f32;
    let db = -6.0_f32;
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
            None,
        );
        state.project_selection = vec![id];
        Ok(id)
    });
    match result {
        Ok(_) => {
            app.status = "Media added".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

fn add_file(app: &mut App, path: std::path::PathBuf) {
    match app.execute("file.import", serde_json::json!({"paths": [path.display().to_string()]})) {
        Ok(_) => app.status = "Imported".to_string(),
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

/// The effective selected range with the marks applied (Out is inclusive).
fn source_selected(app: &mut App) -> Option<filmcraft_time::TimeRange> {
    let view = source_inspect(app)?;
    let rate = source_rate(&view);
    let start = Tick(view["start"].as_i64()?);
    let end = Tick(view["end"].as_i64()?);
    let a = view["markIn"].as_i64().map(Tick).unwrap_or(start).clamp(start, end);
    let fd = rate.frame_duration();
    let b = view["markOut"].as_i64().map(|o| Tick(o) + fd).unwrap_or(end).clamp(a, end);
    Some(filmcraft_time::TimeRange::from_bounds(a, b.max(a)))
}

/// Open `item` in the Source monitor. Editor state: not undoable, not dirtying.
pub(crate) fn open_source(app: &mut App, item: ItemId) {
    match app.session.execute("source.open", serde_json::json!({"item": item.0})) {
        Ok(_) => {
            app.source_monitor = None;
            app.source_playing = false;
            app.bin_selection = Some(item);
            app.status = "Opened in the Source monitor".to_string();
        }
        Err(e) => app.status = e.to_string(),
    }
}

pub(crate) fn close_source(app: &mut App) {
    let _ = app.session.execute("source.closeAll", serde_json::json!({}));
    app.source_monitor = None;
    app.source_playing = false;
    app.status = "Source monitor closed".to_string();
}

pub(crate) fn source_set_playhead(app: &mut App, t: Tick) {
    let _ = app.session.execute("source.setPlayhead", serde_json::json!({"time": t.0}));
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

/// Play / pause the Source monitor. A video-only wall clock: the audio clock is
/// bound to the Program monitor's sequence, so source playback is silent.
pub(crate) fn source_toggle_play(app: &mut App) {
    if app.session.state.source_item.is_none() {
        app.status = "Open a clip in the Source monitor first".to_string();
        return;
    }
    if app.source_playing {
        app.source_playing = false;
        app.source_last = None;
        return;
    }
    if let Some(range) = source_selected(app)
        && app.session.state.source_playhead >= range.end()
    {
        app.session.state.source_playhead = range.start;
    }
    app.source_playing = true;
    app.source_last = None;
    app.status = "Source playing (silent)".to_string();
}

/// Advance source playback (called once per frame from `App::ui`).
pub(crate) fn source_tick(app: &mut App) {
    if !app.source_playing {
        return;
    }
    let Some(range) = source_selected(app) else {
        app.source_playing = false;
        return;
    };
    let now = std::time::Instant::now();
    let elapsed = app.source_last.map_or(std::time::Duration::ZERO, |last| now.duration_since(last));
    app.source_last = Some(now);
    let ticks = Tick::from_units(i64::try_from(elapsed.as_nanos()).unwrap_or(i64::MAX / 4).min(i64::MAX / 8), 1_000_000_000);
    let next = app.session.state.source_playhead + ticks;
    if next >= range.end() {
        app.session.state.source_playhead = Tick((range.end().0 - 1).max(range.start.0));
        app.source_playing = false;
    } else {
        app.session.state.source_playhead = next;
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
    match app.execute(command, serde_json::json!({"clip": target.0, "edge": "in", "frames": 12})) {
        Ok(value) => {
            let id = value["transition"].as_u64().unwrap_or(0);
            app.status = format!("{label} applied at the selected edit (12 frames, transition {id})");
        }
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

fn project_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("project"));
    panel.add_space(8.0);
    panel.heading("Project");
    panel.horizontal(|ui| {
        if ui.button("+ Bars & Tone").clicked() {
            add_generator(app, "file.newBarsAndTone");
        }
        if ui.button("+ Color Matte").clicked() {
            add_generator(app, "file.newColorMatte");
        }
    });
    panel.horizontal(|ui| {
        if ui.button("+ Tone").clicked() {
            add_tone(app);
        }
        if ui.button("+ File…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Media", &["mp4", "mov", "mkv", "webm", "wav", "mp3", "flac", "png", "jpg", "jpeg", "tiff", "bmp", "tga"])
                .pick_file()
        {
            add_file(app, path);
        }
    });
    panel.horizontal(|ui| {
        if ui.add_enabled(app.bin_selection.is_some(), egui::Button::new("Open Source")).on_hover_text("Open the selected item in the Source monitor").clicked()
            && let Some(id) = app.bin_selection
        {
            open_source(app, id);
        }
        if ui.button("Close Source").clicked() {
            close_source(app);
        }
    });
    panel.separator();

    let mut ids = Vec::new();
    app.session.project.root.all_items(&mut ids);
    let mut clicks = Vec::new();
    let mut chosen = None;
    egui::ScrollArea::vertical().show(&mut panel, |ui| {
        for id in ids {
            let Some(item) = app.session.project.items.get(&id) else { continue };
            let ItemKind::Media(clip) = &item.kind else { continue };
            let name = item.name.clone();
            let kind = match &clip.media {
                MediaRef::Generator(_) => "generator",
                MediaRef::File { .. } => "file",
            };
            let has_comp = app.compositions.contains_key(&id);
            let selected = app.bin_selection == Some(id);
            let label = format!("{name} [{kind}]{}", if has_comp { " ●" } else { "" });
            let response = ui.selectable_label(selected, label).on_hover_text(if has_comp {
                "Has a composition. Click: select for Insert/Overwrite at the playhead. Double-click: append to the timeline. Right-click: remove."
            } else {
                "Click: select for Insert/Overwrite at the playhead. Double-click: append to the timeline. Right-click: remove."
            });
            if response.double_clicked() {
                clicks.push((id, 0));
            } else if response.secondary_clicked() {
                clicks.push((id, 1));
            } else if response.clicked() {
                chosen = Some(id);
            }
        }
    });
    if let Some(id) = chosen {
        app.bin_selection = Some(id);
        app.status = "Selected for Insert/Overwrite".to_string();
    }
    for (id, which) in clicks {
        if which == 0 {
            place_media(app, id);
        } else {
            remove_media(app, id);
        }
    }
    panel.separator();
    panel.colored_label(egui::Color32::GRAY, "Click a media item: Open Source previews it with In/Out marks; Insert/Overwrite in the timeline header places the whole item; double-click appends (all undoable). Select a clip, then the Effects workspace.");
}

fn timeline_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    let mut header = ui
        .new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), TIMELINE_HEADER_H))).id_salt("timeline-head"));
    header.add_space(4.0);
    header.horizontal(|ui| {
        ui.label("Timeline");
        ui.separator();
        if ui.button("Razor (all tracks)").clicked() {
            app.add_edit_all_tracks();
        }
        ui.separator();
        let has_bin_item = app.bin_selection.is_some();
        if ui.add_enabled(has_bin_item, egui::Button::new("Insert")).on_hover_text("Insert the selected bin item at the playhead (ripples existing clips right)").clicked()
            && let Some(item) = app.bin_selection
        {
            place_at_playhead(app, item, true);
        }
        if ui.add_enabled(has_bin_item, egui::Button::new("Overwrite")).on_hover_text("Overwrite the selected bin item at the playhead").clicked()
            && let Some(item) = app.bin_selection
        {
            place_at_playhead(app, item, false);
        }
        ui.separator();
        if ui.button("Add Marker (M)").on_hover_text("Add a marker at the playhead (undoable)").clicked() {
            add_marker(app);
        }
        if ui.button("Prev Marker").clicked() {
            go_marker(app, false);
        }
        if ui.button("Next Marker").clicked() {
            go_marker(app, true);
        }
        if ui.button("Clear Marker").on_hover_text("Clear the marker at the playhead").clicked() {
            clear_marker(app);
        }
        ui.separator();
        if ui.add_enabled(app.selection.is_some(), egui::Button::new("Ripple Delete")).on_hover_text("Remove the selected clip and close the gap (Shift+Delete)").clicked() {
            ripple_delete_selected(app);
        }
        ui.separator();
        let mut snapping = app.session.state.snapping;
        if ui.checkbox(&mut snapping, "Snap").on_hover_text("Snap moves and trims to clip edges, the playhead and zero").changed() {
            match app.session.execute("sequence.snap", serde_json::json!({"on": snapping})) {
                Ok(_) => app.status = if snapping { "Snapping on".to_string() } else { "Snapping off".to_string() },
                Err(e) => app.status = e.to_string(),
            }
        }
        ui.separator();
        let mut ripple = app.session.state.ripple_sequence_markers;
        if ui.checkbox(&mut ripple, "Ripple Markers").on_hover_text("Insert moves sequence markers at or after the insert point").changed() {
            match app.session.execute("markers.rippleSequenceMarkers", serde_json::json!({"on": ripple})) {
                Ok(_) => app.status = if ripple { "Ripple markers on".to_string() } else { "Ripple markers off".to_string() },
                Err(e) => app.status = e.to_string(),
            }
        }
        ui.separator();
        ui.add(egui::Slider::new(&mut app.zoom, 20.0..=400.0).text("px / s"));
        ui.separator();
        ui.colored_label(
            egui::Color32::GRAY,
            "Track T/S: target/patch · V: visible · M: mute · ◉: solo · L: lock · Click: playhead · Drag clip: move · Drag edge: trim · Del: remove · Ctrl+Z: undo · Ctrl+K: add edit",
        );
    });
    let has_clip = app.selection.is_some();
    let has_pair = roll_pair(app).is_some();
    header.horizontal(|ui| {
        ui.label("Trim");
        if ui.add_enabled(has_pair, egui::Button::new("Roll -1")).on_hover_text("Roll the selected edit point one frame left (Alt+Ctrl+Left)").clicked() {
            roll_selected(app, -1);
        }
        if ui.add_enabled(has_pair, egui::Button::new("Roll +1")).on_hover_text("Roll the selected edit point one frame right (Alt+Ctrl+Right)").clicked() {
            roll_selected(app, 1);
        }
        ui.separator();
        if ui.add_enabled(has_clip, egui::Button::new("Slip -1")).on_hover_text("Slip the selected clip one frame back in its source (Alt+Left)").clicked() {
            slip_selected(app, -1);
        }
        if ui.add_enabled(has_clip, egui::Button::new("Slip +1")).on_hover_text("Slip the selected clip one frame forward in its source (Alt+Right)").clicked()
        {
            slip_selected(app, 1);
        }
        ui.separator();
        if ui.add_enabled(has_clip, egui::Button::new("Slide -1")).on_hover_text("Slide the selected clip one frame left (Alt+Shift+Left)").clicked() {
            slide_selected(app, -1);
        }
        if ui.add_enabled(has_clip, egui::Button::new("Slide +1")).on_hover_text("Slide the selected clip one frame right (Alt+Shift+Right)").clicked() {
            slide_selected(app, 1);
        }
        ui.separator();
        if ui
            .add_enabled(has_clip, egui::Button::new("Dissolve"))
            .on_hover_text("Apply a 12-frame video cross-dissolve at the selected clip's In edge (Ctrl/Cmd+D)")
            .clicked()
        {
            apply_dissolve(app, true);
        }
        if ui
            .add_enabled(has_clip, egui::Button::new("Crossfade"))
            .on_hover_text("Apply a 12-frame audio crossfade at the selected edit (Ctrl/Cmd+Shift+D)")
            .clicked()
        {
            apply_dissolve(app, false);
        }
        ui.separator();
        ui.colored_label(egui::Color32::GRAY, "Alt+Left/Right: slip · Alt+Shift: slide · Alt+Ctrl: roll");
    });

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
        body_ui.allocate_exact_size(desired, egui::Sense::click_and_drag())
    };
    let painter = ui.painter_at(alloc);
    let header_painter = ui.painter_at(strip);
    let zoom = app.zoom;
    let to_x = |t: Tick| alloc.left() + t.seconds() as f32 * zoom;

    painter.rect_filled(alloc, 0.0, ui.visuals().extreme_bg_color);
    let seconds = (alloc.width() / app.zoom).ceil() as i64;
    for tick in 0..=seconds {
        let x = alloc.left() + tick as f32 * app.zoom;
        painter.line_segment(
            [egui::pos2(x, alloc.top()), egui::pos2(x, alloc.top() + RULER_H)],
            egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color),
        );
        if tick % 5 == 0 {
            painter.text(
                egui::pos2(x + 3.0, alloc.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{tick}s"),
                egui::FontId::monospace(10.0),
                ui.visuals().text_color(),
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

    let mut hit_rects: Vec<(egui::Rect, filmcraft_project::ClipId)> = Vec::new();
    let mut actions: Vec<TrackAction> = Vec::new();
    let tg = app.session.targeting();
    // Corner above the headers, then one header per track. The target square
    // (T) selects the engine's targeted tracks (navigation/paste, mute/solo);
    // the source square (S) patches where new media lands for that stream.
    header_painter.rect_filled(egui::Rect::from_min_max(strip.min, egui::pos2(head.right(), strip.top() + RULER_H)), 0.0, ui.visuals().extreme_bg_color);
    header_painter.text(
        egui::pos2(head.left() + 6.0, strip.top() + RULER_H / 2.0),
        egui::Align2::LEFT_CENTER,
        "Tracks",
        egui::FontId::proportional(10.0),
        ui.visuals().weak_text_color(),
    );
    for (row, track) in s.video_tracks.iter().chain(&s.audio_tracks).enumerate() {
        let y = alloc.top() + RULER_H + row as f32 * ROW_H;
        let is_video = row < s.video_tracks.len();
        let row_rect = egui::Rect::from_min_max(egui::pos2(alloc.left(), y), egui::pos2(alloc.right(), y + ROW_H));
        painter.rect_filled(row_rect, 2.0, ui.visuals().faint_bg_color);
        let name = if is_video { format!("V{}", row + 1) } else { format!("A{}", row + 1 - s.video_tracks.len()) };
        let head_rect = egui::Rect::from_min_size(egui::pos2(head.left(), y), egui::vec2(head.width(), ROW_H));
        header_painter.rect_filled(head_rect, 0.0, ui.visuals().faint_bg_color);
        header_painter.line_segment(
            [egui::pos2(head.right(), y), egui::pos2(head.right(), y + ROW_H)],
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        let targeted = tg.targeted.contains(&track.id);
        let patched = if is_video { tg.video_dest == Some(track.id) } else { tg.audio_dest == Some(track.id) };
        let button_rect = |x: f32| egui::Rect::from_min_size(egui::pos2(head_rect.left() + x, y + ROW_H / 2.0 - 7.0), egui::vec2(15.0, 15.0));
        let target_rect = button_rect(6.0);
        let patch_rect = button_rect(25.0);
        let on = ui.visuals().selection.bg_fill;
        let off = ui.visuals().widgets.inactive.bg_fill;
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
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
        for item in &track.items {
            let clip_rect = egui::Rect::from_min_max(egui::pos2(to_x(item.start), y + 3.0), egui::pos2(to_x(item.start + item.duration), y + ROW_H - 3.0));
            let selected = app.selection == Some(item.id);
            let fill = if selected {
                ui.visuals().selection.bg_fill
            } else if is_video {
                egui::Color32::from_rgb(70, 105, 145)
            } else {
                egui::Color32::from_rgb(88, 120, 88)
            };
            painter.rect_filled(clip_rect, 3.0, fill);
            painter.rect_stroke(clip_rect, 3.0, egui::Stroke::new(1.0, ui.visuals().widgets.active.bg_stroke.color), egui::StrokeKind::Inside);
            painter.text(
                clip_rect.left_top() + egui::vec2(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                item.name.clone(),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
            hit_rects.push((clip_rect, item.id));
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
    painter.line_segment([egui::pos2(px, alloc.top()), egui::pos2(px, alloc.bottom())], egui::Stroke::new(2.0, egui::Color32::LIGHT_RED));

    // Interaction: drag a clip to move it, its edges to trim (engine
    // commands on release — one undo step); click empty space to scrub.
    const EDGE: f32 = 8.0;
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
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

    if response.dragged()
        && app.timeline_drag.is_some()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match &mut app.timeline_drag {
            Some(TimelineDrag::Move { last_x, .. }) => *last_x = pos.x,
            Some(TimelineDrag::Trim { last_x, .. }) => *last_x = pos.x,
            None => {}
        }
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
        };
        painter.rect_filled(ghost, 3.0, egui::Color32::from_rgba_unmultiplied(120, 170, 230, 90));
        painter.rect_stroke(ghost, 3.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(120, 170, 230)), egui::StrokeKind::Inside);
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
        }
    }

    // Scrubbing (click or drag on empty space — never while a clip drag runs).
    if (response.clicked() || response.dragged())
        && app.timeline_drag.is_none()
        && let Some(pos) = response.interact_pointer_pos()
    {
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

/// The clip a drag operates on.
fn drag_clip(drag: &TimelineDrag) -> Option<filmcraft_project::ClipId> {
    match drag {
        TimelineDrag::Move { clip, .. } => Some(*clip),
        TimelineDrag::Trim { clip, .. } => Some(*clip),
    }
}

/// The Source monitor: a real `render_item` preview of the open media at the
/// engine's source playhead, its transport and the source In/Out marks.
pub fn source_monitor_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("source").layout(egui::Layout::top_down(egui::Align::Min)));
    area.add_space(4.0);

    let Some(view) = source_inspect(app) else {
        area.horizontal(|ui| {
            ui.strong("Source");
            ui.colored_label(egui::Color32::GRAY, "no clip open");
        });
        area.label("Click a media item, then Open Source (or the Source panel button).");
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

    area.horizontal(|ui| {
        ui.strong("Source");
        if ui.button("Open Source").on_hover_text("Open the selected bin item here").clicked()
            && let Some(selected) = app.bin_selection
        {
            open_source(app, selected);
        }
        if ui.button("Close").clicked() {
            close_source(app);
        }
    });

    // The picture, through the engine's own Source-monitor renderer.
    let pic_h = (area.available_rect_before_wrap().height() - 120.0).max(70.0);
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
        if ui.button("|<").on_hover_text("Source start").clicked() {
            source_set_playhead(app, start);
        }
        if ui.button("<|").on_hover_text("Previous frame").clicked() {
            source_step(app, -1);
        }
        if ui.button(if playing { "Pause" } else { "Play" }).clicked() {
            source_toggle_play(app);
        }
        if ui.button("|>").on_hover_text("Next frame").clicked() {
            source_step(app, 1);
        }
        if ui.button(">|").on_hover_text("Source end").clicked() {
            source_set_playhead(app, Tick((end.0 - rate.frame_duration().0).max(start.0)));
        }
    });
    let frames = rate.frame_at(t) - rate.frame_at(start);
    let total = rate.frame_at(end) - rate.frame_at(start);
    area.label(egui::RichText::new(format!("frame {frames} / {total}  ({:.2}s)", (t - start).seconds())).monospace());
    let mut seek = t.0 as f64;
    if area.add(egui::Slider::new(&mut seek, start.0 as f64..=end.0 as f64).show_value(false)).changed() {
        source_set_playhead(app, Tick(seek.round() as i64));
    }

    area.horizontal(|ui| {
        if ui.button("Mark In (I)").clicked() {
            source_mark(app, true);
        }
        if ui.button("Mark Out (O)").clicked() {
            source_mark(app, false);
        }
        if ui.button("Go In").clicked() {
            source_goto(app, false);
        }
        if ui.button("Go Out").clicked() {
            source_goto(app, true);
        }
    });
    area.horizontal(|ui| {
        if ui.button("Clear In").clicked() {
            source_clear(app, "markers.clearIn");
        }
        if ui.button("Clear Out").clicked() {
            source_clear(app, "markers.clearOut");
        }
        if ui.button("Clear Both").clicked() {
            source_clear(app, "markers.clearInOut");
        }
    });
    if let Some(selected) = source_selected(app) {
        let text = if mark_in.is_none() && mark_out.is_none() {
            format!("No In/Out marks - using full source ({:.2}s)", selected.duration.seconds())
        } else {
            format!("Selected range {:.2}s", selected.duration.seconds())
        };
        area.colored_label(egui::Color32::GRAY, text);
    }
    area.horizontal(|ui| {
        if ui.button("Insert (,)").on_hover_text("Insert the source range at the sequence playhead").clicked() {
            source_edit(app, true);
        }
        if ui.button("Overwrite (.)").on_hover_text("Overwrite with the source range at the sequence playhead").clicked() {
            source_edit(app, false);
        }
    });
    if playing {
        ui.ctx().request_repaint();
    }
}

/// The program monitor — shared by the Edit and Color workspaces.
pub fn monitor_panel(app: &mut App, ui: &mut egui::Ui, rect: egui::Rect) {
    let Some(seq_id) = app.sequence_id() else {
        let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("monitor"));
        area.vertical_centered(|ui| ui.label("No sequence. New project creates one; import media on the left and double-click it to place a clip."));
        return;
    };
    let rate = app.sequence().map(|s| s.settings.frame_rate).unwrap_or(SEQUENCE_RATE);
    let frame = rate.frame_at(app.playhead);

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

    let mut area = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("monitor"));
    if let Some(m) = &app.monitor {
        let avail = area.available_rect_before_wrap();
        let scale = (avail.width() / m.size.0 as f32).min((avail.height() - 24.0) / m.size.1 as f32);
        let shown = egui::vec2(m.size.0 as f32 * scale, m.size.1 as f32 * scale);
        let at = egui::pos2(avail.left() + (avail.width() - shown.x) / 2.0, avail.top() + (avail.height() - shown.y) / 2.0);
        let dest = egui::Rect::from_min_size(at, shown);
        ui.painter().image(m.texture.id(), dest, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
    area.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.colored_label(egui::Color32::GRAY, if app.wall_start.is_some() && app.playing { "wall clock (no audio device)" } else { "audio clock" });
        ui.label(format!(
            "{} × {} · frame {}",
            app.monitor.as_ref().map(|m| m.size.0).unwrap_or(0),
            app.monitor.as_ref().map(|m| m.size.1).unwrap_or(0),
            frame + 1
        ));
    });
}

#[cfg(test)]
mod tests {
    //! Real engine tests for source-patched placement (append and
    //! insert/overwrite), the track-header settings, the marker helpers, the
    //! snap maths and the Source monitor (marks, range edits, marker ripple,
    //! persistence): no UI, but the exact commands the panels use.

    use super::{placement_params, plan_placement};
    use filmcraft_engine::Session;
    use filmcraft_media::generators::GeneratorSource;
    use filmcraft_media::{Generator, MediaSource};
    use filmcraft_project::{ClipId, ItemId, ItemKind, Label, MediaClip, MediaRef};
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
        assert_eq!(all.len() - without.len(), 2, "the dragged clip contributes its start and end, and nothing else");
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
        // Select the picture; the helper finds its linked sound and applies the
        // audio transition there.
        app.selection = Some(clips[0]);
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
}
