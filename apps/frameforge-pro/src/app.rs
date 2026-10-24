//! The application state: the editing engine's session owns the editing
//! project (every edit is a real, undoable engine command), the compositing
//! compositions live beside it, and the workspaces sit on top. Playback runs
//! on the audio clock when a sound device exists, the wall clock otherwise.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use filmcraft_engine::{EditorState, Session};
use filmcraft_project::{ClipId, ItemId, Project};
use filmcraft_time::{FrameRate, Tick};
use frameforge_bridge::{Composition, FFPROJ_SCHEMA, FrameForgeProject, ProjectSources};

use crate::audio::Player;
use crate::export::ExportState;
use crate::history::{CompositionStep, EditHistory, undo_top};

/// The workspaces. Color and Audio are not built yet: they appear in the bar
/// as clearly-marked planned entries, never as fake panels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Edit,
    Effects,
    Color,
    Audio,
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
    /// Timeline and composition edits in one chronological undo/redo order.
    pub history: EditHistory,

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
    /// The selected FilmCraft effect of the Color workspace's clip.
    pub color_effect: Option<usize>,
    /// The Color workspace's stack width (draggable gutter).
    pub color_stack_w: f32,
    /// When the last recovery autosave was written (shown in the status bar).
    pub autosaved_at: Option<Instant>,
    /// Throttles autosave writes.
    last_autosave: Instant,
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
            history: EditHistory::new(),
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
            color_effect: None,
            color_stack_w: 380.0,
            autosaved_at: None,
            last_autosave: Instant::now(),
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
        let top = undo_top(&self.session);
        let result = self.session.execute(id, params).map_err(|e| e.to_string())?;
        self.history.note_project_step(&self.session, top);
        self.mark_dirty();
        Ok(result)
    }

    /// Apply a custom, undoable project edit and keep the combined history in
    /// step. The engine still owns the whole-project snapshot.
    pub fn edit_project<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Project, &mut EditorState) -> filmcraft_engine::Result<R>,
    ) -> filmcraft_engine::Result<R> {
        let top = undo_top(&self.session);
        let result = self.session.edit(label, f)?;
        self.history.note_project_step(&self.session, top);
        self.mark_dirty();
        Ok(result)
    }

    /// Record a composition edit that has already been applied to
    /// `self.compositions`. On failure the previous state is put back, so the
    /// document and the history can never disagree.
    pub fn record_comp_change(&mut self, item: ItemId, before: Option<Composition>, after: Option<Composition>, gesture: u64) {
        let rollback = before.clone();
        match self.history.record_composition(&mut self.session, item, before, after, gesture) {
            Ok(()) => self.mark_dirty(),
            Err(e) => {
                match rollback {
                    Some(composition) => {
                        self.compositions.insert(item, composition);
                    }
                    None => {
                        self.compositions.remove(&item);
                    }
                }
                self.status = e.to_string();
                self.mark_dirty();
            }
        }
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
        if !self.confirm_discard() {
            return;
        }
        let mut app = App::from_parts(Project::new("Untitled"), HashMap::new(), None, "New project");
        if let Err(e) =
            app.execute("file.newSequence", serde_json::json!({"name": "Main", "width": SEQUENCE_W, "height": SEQUENCE_H, "fps": 24, "video": 2, "audio": 2}))
        {
            app.status = e;
        }
        *self = app;
    }

    pub fn open(&mut self, path: PathBuf) {
        if !self.confirm_discard() {
            return;
        }
        // A newer autosave belongs to this project: offer the work a crash left
        // behind instead of silently discarding it. If the project file itself
        // is unreadable, the autosave is the only good copy and is loaded
        // directly (with the error reported).
        let recovery = frameforge_bridge::autosave::newer_than(&path);
        let loaded = match FrameForgeProject::load(&path) {
            Ok(doc) => (doc, None),
            Err(e) => match recovery.as_ref().and_then(|_| frameforge_bridge::autosave::load(&path).ok()) {
                Some(recovered) => (recovered, Some(e.to_string())),
                None => {
                    self.status = format!("{e}");
                    return;
                }
            },
        };
        let (doc, project_error) = loaded;
        *self = App::from_parts(doc.project, doc.compositions, Some(path.clone()), "Project opened");
        if let Some(why) = project_error {
            self.status = format!("Project file unreadable ({why}); recovered the autosave. Save to keep it.");
            self.dirty = true;
            self.autosaved_at = Some(Instant::now());
            return;
        }
        if recovery.is_none() {
            return;
        }
        let recover = rfd::MessageDialog::new()
            .set_title("Recover unsaved changes?")
            .set_description("A newer autosave exists for this project (it was written after the last save). Recover the unsaved changes?")
            .set_buttons(rfd::MessageButtons::YesNo)
            .set_level(rfd::MessageLevel::Info)
            .show()
            == rfd::MessageDialogResult::Yes;
        if recover {
            match frameforge_bridge::autosave::load(&path) {
                Ok(recovered) => {
                    *self = App::from_parts(recovered.project, recovered.compositions, Some(path), "Recovered autosaved changes — Save to keep them");
                    self.dirty = true;
                    self.autosaved_at = Some(Instant::now());
                }
                Err(e) => self.status = format!("Project opened; the autosave could not be read: {e}"),
            }
        } else if let Err(e) = frameforge_bridge::autosave::clear(&path) {
            self.status = format!("Project opened; the selected autosave could not be removed: {e}");
        } else {
            self.status = "Project opened; the autosave was discarded".to_string();
        }
    }

    pub fn save(&mut self) {
        let Some(path) = self.doc_path_or_pick() else { return };
        match self.document().save(&path) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.dirty = false;
                let _ = frameforge_bridge::autosave::clear(&path);
                self.autosaved_at = None;
                self.last_autosave = Instant::now();
                self.status = "Project saved".to_string();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    /// Whether the window's close request must be cancelled. The confirmation
    /// is a parameter so the decision table is testable without a dialog.
    fn close_guard(&mut self, close_requested: bool, confirm: fn(&mut App) -> bool) -> bool {
        close_requested && self.dirty && !confirm(self)
    }

    /// The close-button guard: offer to save, discard, or keep editing. A
    /// clean project closes without a prompt.
    fn confirm_close(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        match rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Save changes before closing?\n\nYes saves the project, No discards it, Cancel keeps editing.")
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .set_level(rfd::MessageLevel::Warning)
            .show()
        {
            rfd::MessageDialogResult::Yes => {
                self.save();
                // The Save dialog may have been cancelled or the write may
                // have failed; only close once the document is clean.
                !self.dirty
            }
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    /// The unsaved-changes guard in front of New/Open. A clean project never
    /// prompts.
    fn confirm_discard(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("The current project has unsaved changes. Discard them?")
            .set_buttons(rfd::MessageButtons::YesNo)
            .set_level(rfd::MessageLevel::Warning)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    /// Write a recovery autosave at most every 20 s while the document is
    /// dirty. Only saved projects are autosaved (an untitled project has no
    /// stable sidecar path); the unsaved-changes guard covers that case.
    fn maybe_autosave(&mut self) {
        const INTERVAL: Duration = Duration::from_secs(20);
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.clone() else { return };
        if self.last_autosave.elapsed() < INTERVAL {
            return;
        }
        self.last_autosave = Instant::now();
        match frameforge_bridge::autosave::write(&self.document(), &path) {
            Ok(_) => self.autosaved_at = Some(Instant::now()),
            Err(e) => self.status = format!("Autosave failed: {e}"),
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
                self.path = Some(path.clone());
                self.dirty = false;
                let _ = frameforge_bridge::autosave::clear(&path);
                self.autosaved_at = None;
                self.last_autosave = Instant::now();
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

    /// Undo / redo through the combined history: project steps are restored by
    /// the engine, composition steps by the snapshot that rides with them.
    pub fn undo(&mut self) {
        match self.history.undo(&mut self.session) {
            Some(undone) => {
                if let Some(step) = undone.step.as_ref() {
                    self.apply_composition_step(step, false);
                }
                self.status = format!("Undo {}", undone.label);
                self.after_history_change();
            }
            None => self.status = "Nothing to undo".to_string(),
        }
    }

    pub fn redo(&mut self) {
        match self.history.redo(&mut self.session) {
            Some(redone) => {
                if let Some(step) = redone.step.as_ref() {
                    self.apply_composition_step(step, true);
                }
                self.status = format!("Redo {}", redone.label);
                self.after_history_change();
            }
            None => self.status = "Nothing to redo".to_string(),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    fn apply_composition_step(&mut self, step: &CompositionStep, forward: bool) {
        let state = if forward { &step.after } else { &step.before };
        match state {
            Some(composition) => {
                self.compositions.insert(step.item, composition.clone());
            }
            None => {
                self.compositions.remove(&step.item);
            }
        }
    }

    /// A history jump rebuilt the document: invalidate the preview cache and
    /// drop selections that no longer point at something.
    fn after_history_change(&mut self) {
        self.mark_dirty();
        let effect_item = self.selected_clip().map(|(_, item)| item).or(self.effects_item);
        self.selection = None;
        if let Some(item) = self.effects_item
            && !self.session.project.items.contains_key(&item)
        {
            self.effects_item = None;
        }
        let effect_count = effect_item.and_then(|item| self.compositions.get(&item)).map_or(0, |composition| composition.effects.len());
        if self.selected_effect.is_some_and(|index| index >= effect_count) {
            self.selected_effect = None;
        }
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
        // A close button or Alt+F4 must not discard a dirty document. eframe
        // delivers the request on this frame; CancelClose keeps the window
        // open, and saving first is offered in the dialog.
        let close_requested = ui.ctx().input(|i| i.viewport().close_requested());
        if self.close_guard(close_requested, App::confirm_close) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

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
        // Ctrl+K (add edit on all tracks), Ctrl+S / Ctrl+Shift+S (save). The
        // marker keys are Edit-workspace only and stand down while a text field
        // has focus (they would otherwise fire while typing in a search box).
        let in_edit = self.workspace == Workspace::Edit;
        let typing = ui.ctx().egui_wants_keyboard_input();
        let (space, undo_key, redo_key, delete, add_edit, save_key, save_as_key, add_marker_key, next_marker_key) = ui.ctx().input(|i| {
            let cmd = i.modifiers.command;
            (
                i.key_pressed(egui::Key::Space),
                cmd && i.key_pressed(egui::Key::Z) && !i.modifiers.shift,
                (cmd && i.key_pressed(egui::Key::Z) && i.modifiers.shift) || (cmd && i.key_pressed(egui::Key::Y)),
                i.key_pressed(egui::Key::Delete),
                cmd && i.key_pressed(egui::Key::K),
                cmd && i.key_pressed(egui::Key::S) && !i.modifiers.shift,
                cmd && i.key_pressed(egui::Key::S) && i.modifiers.shift,
                in_edit && !typing && !cmd && !i.modifiers.shift && i.key_pressed(egui::Key::M),
                in_edit && !typing && !cmd && i.modifiers.shift && i.key_pressed(egui::Key::M),
            )
        });
        if save_key {
            self.save();
        }
        if save_as_key {
            self.save_as();
        }
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
        if add_marker_key {
            crate::edit::add_marker(self);
        }
        if next_marker_key {
            crate::edit::go_marker(self, true);
        }

        // Recovery autosave (throttled; only for saved, dirty projects).
        self.maybe_autosave();

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
                Workspace::Color => crate::color::show(self, ui, body),
                Workspace::Audio => crate::mixer::show(self, ui, body),
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
        bar.selectable_value(&mut self.workspace, Workspace::Color, "Color");
        bar.selectable_value(&mut self.workspace, Workspace::Audio, "Audio");
        bar.selectable_value(&mut self.workspace, Workspace::Export, "Export");

        bar.separator();

        if bar.add_enabled(self.can_undo(), egui::Button::new("Undo")).clicked() {
            self.undo();
        }
        if bar.add_enabled(self.can_redo(), egui::Button::new("Redo")).clicked() {
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
        if let Some(at) = self.autosaved_at {
            bar.separator();
            bar.colored_label(egui::Color32::GRAY, format!("Autosaved · {}s ago", at.elapsed().as_secs()));
        }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use frameforge_bridge::Composition;

    use super::*;

    fn refuse_discard(_: &mut App) -> bool {
        false
    }

    fn allow_discard(_: &mut App) -> bool {
        true
    }

    #[test]
    fn close_guard_blocks_only_a_dirty_project_when_discard_is_denied() {
        let mut app = App::from_parts(Project::new("Untitled"), HashMap::new(), None, "test");
        app.dirty = false;
        assert!(!app.close_guard(true, refuse_discard), "a clean project closes without a prompt");

        app.dirty = true;
        assert!(!app.close_guard(false, refuse_discard), "no close request, no prompt");
        assert!(!app.close_guard(true, allow_discard), "discarding lets the close through");
        assert!(app.close_guard(true, refuse_discard), "keeping the project cancels the close");
    }

    #[test]
    fn ctrl_z_after_a_composition_edit_leaves_the_timeline_edit_alone() {
        let mut app = App::from_parts(Project::new("Untitled"), HashMap::new(), None, "test");
        app.edit_project("Timeline", |project, _| {
            project.name = "timeline one".to_string();
            Ok(())
        })
        .unwrap();

        let item = ItemId(1);
        let composition = Composition::default();
        app.compositions.insert(item, composition.clone());
        app.record_comp_change(item, None, Some(composition), 1);

        app.undo();
        assert!(!app.compositions.contains_key(&item), "the composition edit is undone");
        assert_eq!(app.session.project.name, "timeline one", "the timeline edit is untouched");

        app.undo();
        assert_eq!(app.session.project.name, "Untitled", "the timeline edit is undone");
        assert!(!app.can_undo());

        app.redo();
        assert_eq!(app.session.project.name, "timeline one");
        app.redo();
        assert!(app.compositions.contains_key(&item), "redo restores the composition");
        assert!(!app.can_redo());
    }
}
