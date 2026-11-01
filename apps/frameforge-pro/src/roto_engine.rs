#![allow(dead_code)] // The engine surface is complete; the workspace adopts pieces incrementally.

//! The rotoscoping engine's workspace controls: which backend is chosen, the
//! model manager (catalogue, install plan, download, verify, remove), and the
//! runtime report.
//!
//! This panel never runs inference and never downloads anything by itself: an
//! install is a *plan* the user starts, the downloader is the platform's own
//! `curl`, and a model only becomes "installed" after its SHA-256 has been
//! computed on disk. Every backend that is not actually wired says so.

use std::path::{Path, PathBuf};
use std::process::Child;

use frameforge_roto::engine::{FileProbe, FrameRange, RuntimeProbe};
use frameforge_roto::models::{
    self, DiskBudget, Install, InstallPlan, InstallState, ModelDescriptor, PlanError, Redistribution, RuntimeKind, Task, download_command, find, install_plan,
    installed_bytes, total_bytes,
};
use frameforge_roto::runtime::{self, RuntimeReport};
use frameforge_roto::scheduler::{JobKind, Queue};
use frameforge_roto::tracking::{BackendRegistry, Direction};

use crate::app::App;
use crate::ui;

/// A download the manager is watching.
struct DownloadRun {
    child: Child,
    destination: PathBuf,
    total: Option<u64>,
    /// The model this download belongs to.
    model: &'static str,
}

/// Everything the engine panel remembers between frames.
pub struct RotoEngineState {
    /// The tracking backend the workspace will use.
    pub tracking_model: String,
    /// The interactive selection backend.
    pub selection_model: String,
    /// Where models are installed on this machine.
    pub models_root: String,
    /// How much disk the user is willing to spend on models, in MiB.
    pub budget_mb: u64,
    /// One install record per model the user has touched.
    installs: Vec<Install>,
    /// The plan currently shown in the panel.
    plan: Option<InstallPlan>,
    /// The download in flight.
    download: Option<DownloadRun>,
    /// The runtime report for this machine.
    report: RuntimeReport,
    /// The shared job queue: enqueueing is the user's action, and it starts
    /// nothing until the workspace asks for the next job.
    pub queue: Queue,
    /// The last thing that happened, for the status line.
    pub message: Option<String>,
    /// The digest the user pinned for a model, per session.
    pub pinned: Vec<(&'static str, String)>,
}

impl Default for RotoEngineState {
    fn default() -> Self {
        Self {
            tracking_model: models::DEFAULT_TRACKING_MODEL.to_string(),
            selection_model: models::DEFAULT_SELECTION_MODEL.to_string(),
            models_root: default_models_root().display().to_string(),
            budget_mb: 20_480,
            installs: Vec::new(),
            plan: None,
            download: None,
            report: runtime::report(None),
            queue: Queue::new(1),
            message: None,
            pinned: Vec::new(),
        }
    }
}

/// Where this machine keeps rotoscoping models.
pub fn default_models_root() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("FrameForgePro").join("models");
    }
    std::env::temp_dir().join("frameforge-models")
}

impl RotoEngineState {
    pub fn models_root_path(&self) -> PathBuf {
        PathBuf::from(self.models_root.trim())
    }

    /// The probe the engine adapters consult: file presence only.
    pub fn probe(&self) -> FileProbe {
        FileProbe::new(self.models_root_path())
    }

    pub fn backend_registry(&self, worker: impl Into<PathBuf>, program: impl Into<PathBuf>) -> BackendRegistry {
        BackendRegistry::new(std::sync::Arc::new(self.probe()), program, worker)
    }

    fn install_mut(&mut self, model_id: &str) -> Option<&mut Install> {
        if !self.installs.iter().any(|install| install.model == model_id) {
            self.installs.push(Install::new(model_id)?);
        }
        self.installs.iter_mut().find(|install| install.model == model_id)
    }

    pub fn install(&self, model_id: &str) -> Option<&Install> {
        self.installs.iter().find(|install| install.model == model_id)
    }

    /// What the panel shows for one model.
    pub fn state_of(&self, model_id: &str) -> InstallState {
        if let Some(install) = self.install(model_id) {
            return install.state.clone();
        }
        if !models::may_download(model_id) && find(model_id).is_some_and(|model| model.runtime.needs_runtime()) {
            return InstallState::Refused { reason: "the licence does not allow this app to fetch it".to_string() };
        }
        if self.probe().is_installed(model_id) {
            return InstallState::Installed { sha256: "(found on disk)".to_string() };
        }
        InstallState::NotInstalled
    }

    pub fn pinned_digest(&self, model_id: &str) -> Option<&str> {
        self.pinned.iter().find(|(id, _)| *id == model_id).map(|(_, digest)| digest.as_str())
    }

    /// Plan an install and show it. Never downloads.
    pub fn plan_install(&mut self, model_id: &str) {
        self.message = None;
        match install_plan(model_id, &self.models_root_path(), self.probe().is_installed(model_id)) {
            Ok(plan) => {
                let budget = DiskBudget { budget_mb: self.budget_mb };
                self.message = Some(format!(
                    "plan for {}: {} ({} step(s)); {}",
                    plan.model,
                    budget.describe(&plan),
                    plan.steps.len(),
                    if budget.fits(&plan) { "fits the budget" } else { "larger than the budget" }
                ));
                self.plan = Some(plan);
            }
            Err(error) => {
                self.plan = None;
                self.message = Some(refusal(error));
            }
        }
    }

    /// Start (or resume) the planned download with the platform downloader.
    pub fn start_download(&mut self, resume: bool) {
        let Some(plan) = self.plan.clone() else {
            self.message = Some("plan an install first".to_string());
            return;
        };
        if self.download.is_some() {
            self.message = Some("a download is already running".to_string());
            return;
        }
        let Some(step) = plan.steps.first().cloned() else {
            self.message = Some("this plan has no downloadable files".to_string());
            return;
        };
        if let Some(parent) = step.destination.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            self.message = Some(format!("could not create {}", parent.display()));
            return;
        }
        let command = download_command(&step, resume);
        match std::process::Command::new(&command.program).args(&command.args).spawn() {
            Ok(child) => {
                if let Some(install) = self.install_mut(plan.model) {
                    let _ = install.begin(&plan);
                }
                self.message = Some(format!(
                    "{} {} ({} bytes of {})",
                    if resume { "resuming" } else { "downloading" },
                    step.file_name,
                    installed_bytes(&step.destination).unwrap_or(0),
                    step.size_bytes.map_or("an unknown size".to_string(), |bytes| bytes.to_string())
                ));
                self.download = Some(DownloadRun { child, destination: step.destination.clone(), total: step.size_bytes, model: plan.model });
            }
            Err(error) => self.message = Some(format!("could not start {}: {error}", command.program)),
        }
    }

    /// Pause: stop the downloader but keep the partial file, so Resume continues.
    pub fn pause_download(&mut self) {
        if let Some(run) = self.download.take() {
            let mut run = run;
            let _ = run.child.kill();
            let _ = run.child.wait();
            let done = installed_bytes(&run.destination).unwrap_or(0);
            if let Some(install) = self.install_mut(run.model) {
                install.report(done, run.total);
                install.pause();
            }
            self.message = Some(format!("paused after {done} bytes; Resume continues the same file"));
        }
    }

    /// Cancel: stop the downloader and mark the install cancelled. The partial
    /// file stays for the user to delete or resume explicitly.
    pub fn cancel_download(&mut self) {
        if let Some(run) = self.download.take() {
            let mut run = run;
            let _ = run.child.kill();
            let _ = run.child.wait();
            let done = installed_bytes(&run.destination).unwrap_or(0);
            if let Some(install) = self.install_mut(run.model) {
                install.cancel(done);
            }
            self.message = Some(format!("cancelled after {done} bytes"));
        }
    }

    /// Verify a downloaded file and record the digest.
    pub fn verify_and_install(&mut self, model_id: &str, path: &Path) {
        let expected = self.pinned_digest(model_id).map(str::to_string);
        let Some(install) = self.install_mut(model_id) else {
            self.message = Some(format!("{model_id} is not in the catalogue"));
            return;
        };
        match install.verify(path, expected.as_deref()) {
            Ok(digest) => {
                self.message = Some(format!("{model_id} verified: {digest}"));
                if expected.is_none() {
                    // No publisher digest was known: pin what we measured so the
                    // next verification is a real check.
                    self.pinned.push((install.model, digest));
                }
            }
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    /// Remove a model's files from disk.
    pub fn remove(&mut self, model_id: &str) {
        let dir = self.models_root_path().join(model_id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                if let Some(install) = self.install_mut(model_id) {
                    install.state = InstallState::NotInstalled;
                    install.file = None;
                }
                self.message = Some(format!("removed {}", dir.display()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.message = Some(format!("{} is not installed", dir.display())),
            Err(error) => self.message = Some(format!("could not remove {}: {error}", dir.display())),
        }
    }

    /// Watch the running download: report bytes, verify on exit.
    pub fn poll(&mut self) {
        let Some(run) = self.download.as_mut() else { return };
        let done = installed_bytes(&run.destination).unwrap_or(0);
        let total = run.total;
        let model = run.model;
        if let Some(install) = self.installs.iter_mut().find(|install| install.model == model) {
            install.report(done, total);
        }
        let finished = matches!(run.child.try_wait(), Ok(Some(_)));
        if !finished {
            return;
        }
        let destination = run.destination.clone();
        self.download = None;
        self.verify_and_install(model, &destination);
    }

    /// The jobs the workspace would enqueue for a tracking run.
    pub fn plan_jobs(&mut self, object_id: u64, range: FrameRange) -> (u64, u64) {
        let tracking = self.queue.enqueue(JobKind::Tracking, self.tracking_model.clone(), Some(object_id), range, 5);
        let qc = self.queue.enqueue(JobKind::TemporalQc, "roto.temporal.qc", Some(object_id), range, 3);
        (tracking, qc)
    }
}

fn refusal(error: PlanError) -> String {
    match error {
        PlanError::LicenseRefused(id) => format!("{id}: the licence does not allow this app to download or mirror it — obtain it from the publisher yourself"),
        other => other.to_string(),
    }
}

/// The panel, drawn above the XMem section of the Masking workspace.
pub fn show(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "ROTO ENGINE");
    panel.label(ui::hint(
        "Choose which engine does the work. Models are external: FrameForge plans the run, verifies what you installed and never downloads a model without asking.",
    ));

    backend_selectors(app, panel);
    ui::spaced_rule(panel);
    model_manager(app, panel);
    ui::spaced_rule(panel);
    runtime_report(app, panel);
}

fn backend_selectors(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "BACKENDS");
    let trackers = models::by_task(Task::Tracking);
    let selected_tracker = trackers.iter().find(|model| model.id == app.roto_engine.tracking_model).copied().unwrap_or(trackers[0]);
    panel.horizontal(|ui| {
        ui.label(ui::meta("Tracking"));
        egui::ComboBox::from_id_salt("roto-tracking").selected_text(selected_tracker.name).show_ui(ui, |ui| {
            for model in &trackers {
                ui.selectable_value(&mut app.roto_engine.tracking_model, model.id.to_string(), format!("{} ({})", model.name, model.family.label()));
            }
        });
    });
    licence_line(panel, selected_tracker, app.roto_engine.probe().is_installed(selected_tracker.id));

    let selectors = models::by_task(Task::Selection);
    let selected_selection = selectors.iter().find(|model| model.id == app.roto_engine.selection_model).copied().unwrap_or(selectors[0]);
    panel.horizontal(|ui| {
        ui.label(ui::meta("Selection"));
        egui::ComboBox::from_id_salt("roto-selection").selected_text(selected_selection.name).show_ui(ui, |ui| {
            for model in &selectors {
                ui.selectable_value(&mut app.roto_engine.selection_model, model.id.to_string(), model.name);
            }
        });
    });
    licence_line(panel, selected_selection, app.roto_engine.probe().is_installed(selected_selection.id));

    // What a run would look like — planned, never started here.
    let registry = app.roto_engine.backend_registry("tools/roto_worker.py", "python.exe");
    match registry.plan(&app.roto_engine.tracking_model, &planned_request(app), false) {
        Ok(plan) => {
            let command =
                plan.command.as_ref().map(|command| format!("{} {}", command.program, command.args.join(" "))).unwrap_or_else(|| "in-process".to_string());
            panel.label(ui::hint(&format!("planned run (dormant): {command}")));
        }
        Err(error) => {
            panel.label(egui::RichText::new(format!("cannot plan a run yet: {error}")).size(10.0).color(ui::WARN));
        }
    }
    panel.label(ui::hint(
        "The protocol worker for these backends is a stub (tools/roto_worker.py): it answers the protocol and refuses to fabricate masks until the real runtime is wired.",
    ));
}

/// A minimal request used only to *describe* a run in the panel.
fn planned_request(app: &App) -> frameforge_roto::TrackRequest {
    use frameforge_roto::engine::ObjectSpec;
    use frameforge_roto::selection::Prompt;

    let object = ObjectSpec::new(1, "Subject").with_prompts(vec![Prompt::Mask { frame: 0 }]);
    let first = app.sequence().map(|_| 0).unwrap_or(0);
    frameforge_roto::TrackRequest::new(object, FrameRange::new(first, first + 23), Direction::Forward, (app.roto.width.max(16), app.roto.height.max(16)))
}

fn licence_line(panel: &mut egui::Ui, model: &'static ModelDescriptor, installed: bool) {
    let licence = model.license;
    let colour = if licence.redistribution == Redistribution::Permitted { ui::TEXT_LO } else { ui::WARN };
    panel.label(
        egui::RichText::new(format!("{} · {} · {}", licence.spdx, licence.redistribution.label(), if installed { "installed" } else { "not installed" }))
            .size(10.0)
            .color(colour),
    );
    panel.label(ui::hint(&format!("{} · {}", model.intended_use, licence.name)));
}

fn model_manager(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "MODEL MANAGER");
    panel.horizontal(|ui| {
        ui.label(ui::meta("Models folder"));
        ui.add(egui::TextEdit::singleline(&mut app.roto_engine.models_root).desired_width(160.0));
    });
    panel.horizontal(|ui| {
        ui.label(ui::meta("Disk budget"));
        ui.add(egui::DragValue::new(&mut app.roto_engine.budget_mb).range(256..=204_800).suffix(" MB"));
        let known = total_bytes(&["cutie-base", "sam2.1-tiny"]);
        if let Some(bytes) = known {
            panel.label(ui::hint(&format!("Cutie + SAM 2.1 Tiny: {:.2} GB", bytes as f64 / 1e9)));
        }
    });

    for model in models::catalogue().iter().filter(|model| model.runtime.needs_runtime()) {
        let state = app.roto_engine.state_of(model.id);
        let size = model.versions.iter().flat_map(|version| version.weights.iter()).filter_map(|weight| weight.size_bytes).max();
        let size_text = size.map_or_else(|| "size not verified".to_string(), |bytes| format!("{:.0} MB", bytes as f64 / 1e6));
        panel.horizontal(|ui| {
            ui.label(egui::RichText::new(model.name).size(11.0).color(ui::TEXT_HI));
            ui.label(ui::meta(&format!("{} · {}", size_text, model.license.spdx)));
        });
        panel.label(egui::RichText::new(state_label(&state)).size(10.0).color(state_colour(&state)));

        if model.license.redistribution.allows_download() {
            let downloading = app.roto_engine.download.is_some();
            panel.horizontal(|ui| {
                if ui::mini_button(ui, "Plan install", "Plan the download; nothing is fetched yet").clicked() {
                    app.roto_engine.plan_install(model.id);
                }
                if !downloading && ui::mini_button(ui, "Download", "Start the platform downloader").clicked() {
                    app.roto_engine.start_download(false);
                }
                if downloading && ui::mini_button(ui, "Pause", "Stop the downloader, keep the partial file").clicked() {
                    app.roto_engine.pause_download();
                }
                if !downloading && ui::mini_button(ui, "Resume", "Continue the partial file").clicked() {
                    app.roto_engine.start_download(true);
                }
                if downloading && ui::mini_button(ui, "Cancel", "Stop and mark cancelled").clicked() {
                    app.roto_engine.cancel_download();
                }
                if ui::mini_button(ui, "Verify file…", "Hash a file and record the digest").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file()
                {
                    app.roto_engine.verify_and_install(model.id, &path);
                }
                if ui::mini_button(ui, "Remove", "Delete this model's files").clicked() {
                    app.roto_engine.remove(model.id);
                }
            });
        } else {
            panel.label(ui::hint(&format!("user-supplied only — {} — never fetched by this app", model.license.note)));
        }
    }

    if let Some(plan) = app.roto_engine.plan.clone() {
        ui::section(panel, "INSTALL PLAN");
        panel.label(ui::hint(&format!("{} → {}", plan.model, plan.install_root.display())));
        for step in &plan.steps {
            let command = download_command(step, false);
            panel.label(ui::hint(&format!("{} ({} bytes): {} {}", step.file_name, step.size_bytes.unwrap_or(0), command.program, command.args.join(" "))));
        }
        if plan.unknown_sizes > 0 {
            panel.label(egui::RichText::new(format!("{} file(s) have no verified size", plan.unknown_sizes)).size(10.0).color(ui::WARN));
        }
    }
    if let Some(message) = app.roto_engine.message.clone() {
        panel.label(egui::RichText::new(message).size(10.5).color(ui::TEXT_MID));
    }
    panel.label(ui::hint("Verification uses the one SHA-256 in the tree; a model is only installed after its bytes are hashed."));
}

fn runtime_report(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "RUNTIME");
    let report: &RuntimeReport = &app.roto_engine.report;
    panel.label(ui::hint(&report.summary()));
    for provider in &report.providers {
        let colour = if provider.usable_for_inference { ui::ACCENT_HI } else { ui::TEXT_LO };
        panel.label(egui::RichText::new(format!("{} — {}", provider.label, provider.state.label())).size(10.0).color(colour));
    }
    panel.label(ui::hint(report.guidance()));
    panel.label(ui::hint("GPU memory and RAM are not probed by this build; the manager works to the disk budget you set instead."));
}

fn state_colour(state: &InstallState) -> egui::Color32 {
    match state {
        InstallState::Installed { .. } => ui::ACCENT_HI,
        InstallState::Failed { .. } | InstallState::Refused { .. } => ui::DANGER,
        InstallState::Downloading { .. } | InstallState::Verifying => ui::WARN,
        _ => ui::TEXT_LO,
    }
}

/// A short human label for one install state.
fn state_label(state: &InstallState) -> String {
    match state {
        InstallState::NotInstalled => "not installed".to_string(),
        InstallState::Downloading { done_bytes, total_bytes } => match total_bytes {
            Some(total) => format!("downloading {:.0}% ({done_bytes} of {total} bytes)", *done_bytes as f64 / *total as f64 * 100.0),
            None => format!("downloading {done_bytes} bytes"),
        },
        InstallState::Paused { done_bytes, .. } => format!("paused at {done_bytes} bytes"),
        InstallState::Verifying => "verifying…".to_string(),
        InstallState::Installed { sha256 } => format!("installed · {sha256}"),
        InstallState::Failed { reason } => format!("failed: {reason}"),
        InstallState::Cancelled { done_bytes } => format!("cancelled at {done_bytes} bytes"),
        InstallState::Refused { reason } => format!("not offered: {reason}"),
    }
}

/// The interpreter paths the manager offers for a runtime.
pub fn interpreter_candidates() -> Vec<PathBuf> {
    frameforge_bridge::python_candidates_from_env()
}

/// Whether the runtime the panel would use is present at all.
pub fn runtime_available() -> bool {
    find(models::DEFAULT_TRACKING_MODEL).is_some() && !interpreter_candidates().is_empty()
}

/// A convenience for the workspace: the model that is selected and installed.
pub fn ready_backend(state: &RotoEngineState) -> Option<&'static ModelDescriptor> {
    let model = find(&state.tracking_model)?;
    state.probe().is_installed(model.id).then_some(model)
}

/// The engine's own description of what it can do, for the panel.
pub fn capabilities(state: &RotoEngineState) -> Option<frameforge_roto::EngineCapabilities> {
    let model = find(&state.tracking_model)?;
    Some(frameforge_roto::EngineCapabilities::for_model(model))
}

/// The runtime kind the selected backend needs.
pub fn selected_runtime(state: &RotoEngineState) -> RuntimeKind {
    find(&state.tracking_model).map(|model| model.runtime).unwrap_or(RuntimeKind::BuiltInRust)
}
