//! The application state: one open document, the workspaces over it, and the
//! transport.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use filmcraft_project::{ClipId, ItemId, Project, SequenceSettings};
use filmcraft_time::{FrameRate, Tick};
use frameforge_bridge::{FrameForgeProject, ProjectSources};

use crate::export::ExportState;

/// The workspaces. Color and Audio are not built yet: they appear in the bar
/// as clearly-marked planned entries, never as fake panels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Edit,
    Effects,
    Export,
}

/// The open document and everything derived from it.
pub struct DocState {
    pub doc: FrameForgeProject,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Bumped on every edit; the monitor cache keys on it.
    pub revision: u64,
    sources: Option<ProjectSources>,
}

impl DocState {
    pub fn sources(&mut self) -> &ProjectSources {
        if self.sources.is_none() {
            self.sources = ProjectSources::build(&self.doc).ok();
        }
        self.sources.as_ref().expect("sources")
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.sources = None;
    }
}

pub struct App {
    pub doc: DocState,
    pub workspace: Workspace,
    pub playhead: Tick,
    pub playing: bool,
    pub last: Option<Instant>,
    /// Timeline zoom: pixels per second.
    pub zoom: f32,
    pub selection: Option<ClipId>,
    pub seq: Option<ItemId>,
    pub status: String,
    pub export: ExportState,
    pub monitor: Option<Monitor>,
    /// The Effects workspace's fallback media item (when no clip is selected).
    pub effects_item: Option<ItemId>,
    /// The selected effect of that item's composition.
    pub selected_effect: Option<usize>,
    /// The add-effect search text.
    pub effect_search: String,
}

/// The cached program-monitor texture and what it was rendered from.
pub struct Monitor {
    pub texture: egui::TextureHandle,
    pub frame: i64,
    pub revision: u64,
    pub seq: ItemId,
    pub size: (u32, u32),
}

pub const SEQUENCE_RATE: FrameRate = FrameRate::FPS_24;
pub const SEQUENCE_W: u32 = 1280;
pub const SEQUENCE_H: u32 = 720;

impl App {
    pub fn new() -> App {
        App::from_project(FrameForgeProject::new(Project::new("Untitled")), None)
    }

    pub fn from_project(doc: FrameForgeProject, path: Option<PathBuf>) -> App {
        App {
            workspace: Workspace::Edit,
            playhead: Tick::ZERO,
            playing: false,
            last: None,
            zoom: 120.0,
            selection: None,
            seq: doc.project.sequences().next().map(|it| it.id),
            status: "Ready".to_string(),
            export: crate::export::ExportState { include_audio: true, ..Default::default() },
            monitor: None,
            effects_item: None,
            selected_effect: None,
            effect_search: String::new(),
            doc: DocState { doc, path, dirty: false, revision: 0, sources: None },
        }
    }

    /// New project: a sequence at 720p/24 with two video and two audio tracks.
    pub fn new_project(&mut self) {
        let mut project = Project::new("Untitled");
        project.new_sequence("Main", SequenceSettings { width: SEQUENCE_W, height: SEQUENCE_H, frame_rate: SEQUENCE_RATE, ..Default::default() }, 2, 2, None);
        *self = App::from_project(FrameForgeProject::new(project), None);
        self.status = "New project".to_string();
    }

    pub fn open(&mut self, path: PathBuf) {
        match FrameForgeProject::load(&path) {
            Ok(doc) => {
                *self = App::from_project(doc, Some(path));
                self.status = "Project opened".to_string();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    pub fn save(&mut self) {
        let Some(path) = self.doc.path.clone().or_else(|| self.pick_save_path()) else {
            return;
        };
        match self.doc.doc.save(&path) {
            Ok(()) => {
                self.doc.path = Some(path);
                self.doc.dirty = false;
                self.status = "Project saved".to_string();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    fn pick_save_path(&mut self) -> Option<PathBuf> {
        let path = rfd::FileDialog::new().add_filter("FrameForge Pro project", &["ffproj"]).set_file_name("project.ffproj").save_file()?;
        Some(path)
    }

    pub fn save_as(&mut self) {
        let Some(path) = self.pick_save_path() else { return };
        match self.doc.doc.save(&path) {
            Ok(()) => {
                self.doc.path = Some(path);
                self.doc.dirty = false;
                self.status = "Project saved".to_string();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    /// The active sequence, if the project has one.
    pub fn sequence_id(&self) -> Option<ItemId> {
        self.seq.or_else(|| self.doc.doc.project.sequences().next().map(|it| it.id))
    }

    /// The active sequence, read-only.
    pub fn sequence(&self) -> Option<&filmcraft_project::Sequence> {
        let id = self.sequence_id()?;
        self.doc.doc.project.items.get(&id).and_then(|it| it.as_sequence())
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
    pub fn delete_selected_clip(&mut self) {
        let Some((clip, _)) = self.selected_clip() else { return };
        let seq = self.sequence_id();
        if let Some(seq) = seq
            && let Some(s) = self.doc.doc.project.sequence_mut(seq)
        {
            for track in s.video_tracks.iter_mut().chain(s.audio_tracks.iter_mut()) {
                track.items.retain(|it| it.id != clip);
                track.sort();
            }
        }
        self.selection = None;
        self.doc.mark_dirty();
        self.status = "Clip removed".to_string();
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Playback advances the playhead by wall-clock time, quantized to the
        // sequence's frames. Silent (video preview only) until the engine
        // session brings the audio clock (M6) — the monitor says so.
        if self.playing {
            let now = Instant::now();
            if let Some(last) = self.last.replace(now) {
                let nanos = now.duration_since(last).as_nanos();
                let nanos = i64::try_from(nanos).unwrap_or(i64::MAX / 4);
                self.playhead = self.playhead + Tick::from_units(nanos.min(i64::MAX / 8), 1_000_000_000);
            }
            let end = self.sequence_end();
            if self.playhead >= end {
                self.playhead = end;
                self.playing = false;
                self.last = None;
            }
            ctx.request_repaint();
        }

        // The transport keys work everywhere.
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) && self.sequence().is_some() {
            self.playing = !self.playing;
            self.last = None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.delete_selected_clip();
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{}{}", if self.doc.dirty { "• " } else { "" }, self.doc.doc.project.name));
                ui.separator();
                ui.colored_label(egui::Color32::GRAY, &self.status);
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.workspace {
            Workspace::Edit => crate::edit::show(self, ui),
            Workspace::Effects => crate::effects::show(self, ui),
            Workspace::Export => crate::export::show(self, ui),
        });
    }
}

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("FrameForge Pro").strong());
            ui.separator();

            if ui.button("New").clicked() {
                self.new_project();
            }
            if ui.button("Open…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("FrameForge Pro project", &["ffproj"]).pick_file() {
                    self.open(path);
                }
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            if ui.button("Save As…").clicked() {
                self.save_as();
            }

            ui.separator();

            ui.selectable_value(&mut self.workspace, Workspace::Edit, "Edit");
            ui.selectable_value(&mut self.workspace, Workspace::Effects, "Effects");
            ui.selectable_value(&mut self.workspace, Workspace::Export, "Export");
            ui.add_enabled(false, egui::Button::new("Color"))
                .on_disabled_hover_text("The Color workspace is planned but not built yet (see docs/ARCHITECTURE.md, M6)");
            ui.add_enabled(false, egui::Button::new("Audio"))
                .on_disabled_hover_text("The Audio workspace is planned but not built yet (see docs/ARCHITECTURE.md, M6)");

            ui.separator();

            let playing = self.playing;
            if ui.button(if playing { "⏸ Pause" } else { "▶ Play" }).clicked() && self.sequence().is_some() {
                self.playing = !self.playing;
                self.last = None;
            }
            if ui.button("⏮ Start").clicked() {
                self.playhead = Tick::ZERO;
            }
            ui.monospace(self.timecode());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(path) = &self.doc.path {
                    ui.label(egui::RichText::new(path.display().to_string()).weak());
                }
            });
        });
    }
}
