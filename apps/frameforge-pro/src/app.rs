//! The application state: the editing engine's session owns the editing
//! project (every edit is a real, undoable engine command), the compositing
//! compositions live beside it, and the workspaces sit on top. Playback runs
//! on the audio clock when a sound device exists, the wall clock otherwise.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use filmcraft_engine::Session;
use filmcraft_project::{ClipId, ItemId, Project};
use filmcraft_time::{FrameRate, Tick};
use frameforge_bridge::{Composition, FFPROJ_SCHEMA, FrameForgeProject, ProjectSources};

use crate::audio::Player;
use crate::export::ExportState;

/// The workspaces. Color and Audio are not built yet: they appear in the bar
/// as clearly-marked planned entries, never as fake panels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Edit,
    Effects,
    Export,
}

pub const TOP_H: f32 = 38.0;
pub const STATUS_H: f32 = 22.0;

pub const SEQUENCE_RATE: FrameRate = FrameRate::FPS_24;
pub const SEQUENCE_W: u32 = 1280;
pub const SEQUENCE_H: u32 = 720;

/// An in-progress timeline drag, applied through an engine command on
/// release (one undo step per gesture). `last_x` tracks the pointer during
/// the drag for the ghost preview and the final delta.
pub enum TimelineDrag {
    Move { clip: ClipId, track: filmcraft_project::TrackId, grab_x: f32, origin: Tick, last_x: f32 },
    Trim { clip: ClipId, edge: &'static str, grab_x: f32, last_x: f32 },
}

pub struct App {
    /// The editing engine session: owns the editing project, the command
    /// registry, and the undo history. Every project-changing action the
    /// shell performs goes through [`Session::execute`].
    pub session: Session,
    /// The compositing compositions, keyed by media item — the bridge's half
    /// of the document (`.ffproj`).
    pub compositions: HashMap<ItemId, Composition>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Bumped on every edit; the monitor cache keys on it.
    pub revision: u64,
    sources: Option<ProjectSources>,

    pub workspace: Workspace,
    pub playhead: Tick,
    pub playing: bool,
    /// Wall-clock playback (no audio device, or playback not started yet).
    pub last: Option<Instant>,
    pub zoom: f32,
    pub selection: Option<ClipId>,
    pub seq: Option<ItemId>,
    pub status: String,
    pub export: ExportState,
    pub monitor: Option<Monitor>,
    pub effects_item: Option<ItemId>,
    pub selected_effect: Option<usize>,
    pub effect_search: String,
    pub project_w: f32,
    pub timeline_h: f32,
    pub player: Player,
    /// The wall-clock position when audio playback fell back.
    pub wall_start: Option<Tick>,
    /// An in-progress timeline drag (move or trim).
    pub timeline_drag: Option<TimelineDrag>,
}

/// The cached program-monitor texture and what it was rendered from.
pub struct Monitor {
    pub texture: egui::TextureHandle,
    pub frame: i64,
    pub revision: u64,
    pub seq: ItemId,
    pub size: (u32, u32),
}

impl App {
    pub fn new() -> App {
        App::from_parts(Project::new("Untitled"), HashMap::new(), None, "Ready")
    }

    /// Build the app around an editing project (from a `.ffproj` or fresh).
    pub fn from_parts(project: Project, compositions: HashMap<ItemId, Composition>, path: Option<PathBuf>, status: &str) -> App {
        let mut session = Session::default();
        session.project = Arc::new(project);
        let seq = session.project.sequences().next().map(|it| it.id);
        session.state.active_sequence = seq;
        App {
            session,
            compositions,
            path,
            dirty: false,
            revision: 0,
            sources: None,
            workspace: Workspace::Edit,
            playhead: Tick::ZERO,
            playing: false,
            last: None,
            zoom: 120.0,
            selection: None,
            seq,
            status: status.to_string(),
            export: ExportState { include_audio: true, ..Default::default() },
            monitor: None,
            effects_item: None,
            selected_effect: None,
            effect_search: String::new(),
            project_w: 250.0,
            timeline_h: 220.0,
            player: Player::new(),
            wall_start: None,
            timeline_drag: None,
        }
    }

    /// The whole document (editing project + compositions), for save, export
    /// and the bridge builders.
    pub fn document(&self) -> FrameForgeProject {
        FrameForgeProject { schema: FFPROJ_SCHEMA, project: (*self.session.project).clone(), compositions: self.compositions.clone() }
    }

    /// Run an engine command; on success the document changed (the monitor
    /// and sources rebuild).
    pub fn execute(&mut self, id: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let result = self.session.execute(id, params).map_err(|e| e.to_string())?;
        self.mark_dirty();
        Ok(result)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.sources = None;
    }

    /// The project and the built sources together: the render path needs
    /// both, and this keeps the borrow in one place.
    pub fn with_sources<R>(&mut self, f: impl FnOnce(&Project, &ProjectSources) -> R) -> Option<R> {
        if self.sources.is_none() {
            self.sources = ProjectSources::build_from(&self.session.project, &self.compositions).ok();
        }
        let Some(sources) = &self.sources else { return None };
        Some(f(&self.session.project, sources))
    }

    /// A clone of the built sources, for the audio worker.
    pub fn built_sources(&mut self) -> Option<ProjectSources> {
        if self.sources.is_none() {
            self.sources = ProjectSources::build_from(&self.session.project, &self.compositions).ok();
        }
        self.sources.clone()
    }

    pub fn new_project(&mut self) {
        let mut app = App::from_parts(Project::new("Untitled"), HashMap::new(), None, "New project");
        if let Err(e) =
            app.execute("file.newSequence", serde_json::json!({"name": "Main", "width": SEQUENCE_W, "height": SEQUENCE_H, "fps": 24, "video": 2, "audio": 2}))
        {
            app.status = e;
        }
        *self = app;
    }

    pub fn open(&mut self, path: PathBuf) {
        match FrameForgeProject::load(&path) {
            Ok(doc) => {
                let project = doc.project;
                let compositions = doc.compositions;
                *self = App::from_parts(project, compositions, Some(path), "Project opened");
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    pub fn save(&mut self) {
        let Some(path) = self.doc_path_or_pick() else { return };
        match self.document().save(&path) {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
                self.status = "Project saved".to_string();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    fn doc_path_or_pick(&mut self) -> Option<PathBuf> {
        if let Some(path) = self.path.clone() {
            return Some(path);
        }
        self.pick_save_path()
    }

    fn pick_save_path(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().add_filter("FrameForge Pro project", &["ffproj"]).set_file_name("project.ffproj").save_file()
    }

    pub fn save_as(&mut self) {
        let Some(path) = self.pick_save_path() else { return };
        match self.document().save(&path) {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
                self.status = "Project saved".to_string();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    /// The active sequence, if the project has one.
    pub fn sequence_id(&self) -> Option<ItemId> {
        self.seq.or_else(|| self.session.project.sequences().next().map(|it| it.id))
    }

    /// The active sequence, read-only.
    pub fn sequence(&self) -> Option<&filmcraft_project::Sequence> {
        let id = self.sequence_id()?;
        self.session.project.items.get(&id).and_then(|it| it.as_sequence())
    }

    /// The sequence's duration (its last clip's end), or zero.
    pub fn sequence_end(&self) -> Tick {
        self.sequence()
            .map(|s| {
                let mut end = Tick::ZERO;
                for track in s.video_tracks.iter().chain(&s.audio_tracks) {
                    for item in &track.items {
                        end = end.max(item.start + item.duration);
                    }
                }
                end
            })
            .unwrap_or_default()
    }

    /// SMPTE-ish timecode of the playhead at the sequence's rate.
    pub fn timecode(&self) -> String {
        let rate = self.sequence().map(|s| s.settings.frame_rate).unwrap_or(SEQUENCE_RATE);
        let total = rate.frame_at(self.playhead);
        let fps = rate.as_f64().round().max(1.0) as i64;
        format!("{}:{:02}:{:02}:{:02}", total / (fps * 3600), (total / (fps * 60)) % 60, (total / fps) % 60, total % fps)
    }

    /// The selected clip, if the selection still names one.
    pub fn selected_clip(&self) -> Option<(ClipId, ItemId)> {
        let selected = self.selection?;
        let s = self.sequence()?;
        for track in s.video_tracks.iter().chain(&s.audio_tracks) {
            for item in &track.items {
                if item.id == selected {
                    return Some((item.id, item.item));
                }
            }
        }
        None
    }

    /// Delete the selected clip through the engine (undoable), naming the
    /// clip explicitly so no selection state is needed.
    pub fn delete_selected_clip(&mut self) {
        let Some((clip, _)) = self.selected_clip() else { return };
        match self.execute("edit.clear", serde_json::json!({"clips": [clip.0]})) {
            Ok(_) => self.status = "Clip removed".to_string(),
            Err(e) => self.status = e,
        }
        self.selection = None;
    }

    /// Undo / redo through the engine's history.
    pub fn undo(&mut self) {
        match self.execute("edit.undo", serde_json::json!({})) {
            Ok(_) => self.status = "Undo".to_string(),
            Err(e) => self.status = e,
        }
        self.selection = None;
    }

    pub fn redo(&mut self) {
        match self.execute("edit.redo", serde_json::json!({})) {
            Ok(_) => self.status = "Redo".to_string(),
            Err(e) => self.status = e,
        }
        self.selection = None;
    }

    /// Razor every track at the playhead (Add Edit, All Tracks).
    pub fn add_edit_all_tracks(&mut self) {
        match self.execute("sequence.addEditAllTracks", serde_json::json!({"time": self.playhead.0})) {
            Ok(_) => self.status = "Add Edit (all tracks)".to_string(),
            Err(e) => self.status = e,
        }
    }

    fn start_playback(&mut self) {
        let Some(seq) = self.sequence_id() else { return };
        let Some(sources) = self.built_sources() else {
            self.status = "Cannot build media sources".to_string();
            return;
        };
        let project = Arc::clone(&self.session.project);
        let start = self.playhead;
        let end = self.sequence_end();
        if end <= start {
            self.playhead = Tick::ZERO;
            return;
        }
        match self.player.play(project, sources, seq, start, end) {
            Ok(()) => {
                self.wall_start = None;
                self.status = "Playing".to_string();
            }
            Err(why) => {
                // No usable device: wall-clock playback, silently.
                self.wall_start = Some(self.playhead);
                self.status = format!("Playing (silent — {why})");
            }
        }
    }

    fn stop_playback(&mut self) {
        self.player.stop();
        self.playing = false;
        self.last = None;
        self.wall_start = None;
        self.status = "Stopped".to_string();
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Playback: the audio clock drives the playhead when a stream runs;
        // otherwise the wall clock does.
        if self.playing {
            if self.wall_start.is_some() {
                let now = Instant::now();
                if let Some(last) = self.last.replace(now) {
                    let nanos = i64::try_from(now.duration_since(last).as_nanos()).unwrap_or(i64::MAX / 4);
                    self.playhead += Tick::from_units(nanos.min(i64::MAX / 8), 1_000_000_000);
                }
                let end = self.sequence_end();
                if self.playhead >= end {
                    self.playhead = end;
                    self.stop_playback();
                }
            } else {
                self.playhead = self.player.position();
                if self.playhead >= self.sequence_end() || self.player.is_finished() {
                    self.playhead = self.sequence_end();
                    self.stop_playback();
                }
            }
            ui.ctx().request_repaint();
        }

        // Transport keys everywhere: Space, Ctrl+Z / Ctrl+Shift+Z, Delete,
        // Ctrl+K (add edit on all tracks).
        let (space, undo_key, redo_key, delete, add_edit) = ui.ctx().input(|i| {
            let cmd = i.modifiers.command;
            (
                i.key_pressed(egui::Key::Space),
                cmd && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                (cmd && i.key_pressed(egui::Key::Z) && i.modifiers.shift) || (cmd && i.key_pressed(egui::Key::Y)),
                i.key_pressed(egui::Key::Delete),
                cmd && i.key_pressed(egui::Key::K),
            )
        });
        if space && self.sequence().is_some() {
            if self.playing {
                self.stop_playback();
            } else {
                self.playing = true;
                self.last = None;
                self.start_playback();
            }
        }
        if undo_key {
            self.undo();
        }
        if redo_key {
            self.redo();
        }
        if delete {
            self.delete_selected_clip();
        }
        if add_edit {
            self.add_edit_all_tracks();
        }

        let full = ui.max_rect();
        let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), TOP_H));
        let status = egui::Rect::from_min_max(egui::pos2(full.min.x, full.max.y - STATUS_H), full.max);
        let body = egui::Rect::from_min_max(egui::pos2(full.min.x, top.max.y), egui::pos2(full.max.x, status.min.y));

        // The panic hook (upstream's rule): a fault in one panel must never
        // close the app or lose the document — catch it, keep the session,
        // say what happened. It is a safety net, not a substitute for the
        // engine crates' never-crash rules.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.top_bar(ui, top);
            match self.workspace {
                Workspace::Edit => crate::edit::show(self, ui, body),
                Workspace::Effects => crate::effects::show(self, ui, body),
                Workspace::Export => crate::export::show(self, ui, body),
            }
            self.status_bar(ui, status);
        }));
        if result.is_err() {
            self.status = "A UI error occurred. The project is intact — save it now.".to_string();
            self.playing = false;
        }
    }
}

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
        let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        bar.add_space(10.0);
        bar.label(egui::RichText::new("FrameForge Pro").strong());
        bar.separator();

        if bar.button("New").clicked() {
            self.new_project();
        }
        if bar.button("Open…").clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("FrameForge Pro project", &["ffproj"]).pick_file()
        {
            self.open(path);
        }
        if bar.button("Save").clicked() {
            self.save();
        }
        if bar.button("Save As…").clicked() {
            self.save_as();
        }

        bar.separator();

        bar.selectable_value(&mut self.workspace, Workspace::Edit, "Edit");
        bar.selectable_value(&mut self.workspace, Workspace::Effects, "Effects");
        bar.selectable_value(&mut self.workspace, Workspace::Export, "Export");
        bar.add_enabled(false, egui::Button::new("Color")).on_disabled_hover_text("The Color workspace is planned but not built yet (M7)");
        bar.add_enabled(false, egui::Button::new("Audio")).on_disabled_hover_text("The Audio workspace is planned but not built yet (M7)");

        bar.separator();

        if bar.add_enabled(self.session.history.can_undo(), egui::Button::new("Undo")).clicked() {
            self.undo();
        }
        if bar.add_enabled(self.session.history.can_redo(), egui::Button::new("Redo")).clicked() {
            self.redo();
        }

        bar.separator();

        if bar.button(if self.playing { "Pause" } else { "Play" }).clicked() && self.sequence().is_some() {
            if self.playing {
                self.stop_playback();
            } else {
                self.playing = true;
                self.last = None;
                self.start_playback();
            }
        }
        if bar.button("|<").clicked() {
            self.playhead = Tick::ZERO;
        }
        bar.label(egui::RichText::new(self.timecode()).monospace());

        if let Some(path) = &self.path {
            bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(path.display().to_string()).weak());
            });
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
        let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        bar.add_space(8.0);
        bar.label(format!("{}{}", if self.dirty { "• " } else { "" }, self.session.project.name));
        bar.separator();
        bar.colored_label(egui::Color32::GRAY, &self.status);
    }
}

/// A draggable vertical gutter between two regions.
pub fn gutter_v(ui: &mut egui::Ui, x: f32, y0: f32, y1: f32, value: &mut f32) {
    let grab = 6.0;
    let rect = egui::Rect::from_min_max(egui::pos2(x - grab / 2.0, y0), egui::pos2(x + grab / 2.0, y1));
    let response = ui.interact(rect, egui::Id::new("gutter-v"), egui::Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *value = pos.x;
    }
}

/// A draggable horizontal gutter between two regions.
pub fn gutter_h(ui: &mut egui::Ui, y: f32, x0: f32, x1: f32, value: &mut f32) {
    let grab = 6.0;
    let rect = egui::Rect::from_min_max(egui::pos2(x0, y - grab / 2.0), egui::pos2(x1, y + grab / 2.0));
    let response = ui.interact(rect, egui::Id::new("gutter-h"), egui::Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *value = pos.y;
    }
}
