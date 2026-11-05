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
use frameforge_roto::runtime::{self, PerformanceSettings, RuntimeReport};
use frameforge_roto::scheduler::{JobKind, Queue};
use frameforge_roto::segment::{SegmentPlan, SegmentRequest, plan_segment};
use frameforge_roto::selection::{Prompt, SelectionSession};
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

/// One backend run in flight (frame export → worker → publish).
pub struct BackendRun {
    handle: crate::roto_run::RunHandle,
    pub model_id: String,
}

/// Everything the engine panel remembers between frames.
pub struct RotoEngineState {
    /// The user's consent for the selected backend: they installed the runtime and
    /// the weights themselves and accept the licence.
    pub consent: bool,
    /// The backend run in flight, if any.
    pub run: Option<BackendRun>,
    /// The last finished run's report.
    pub last_run: Option<crate::roto_run::RunReport>,
    /// The user's Cutie checkout: the Python package `tools/cutie_worker.py`
    /// imports. FrameForge never ships or vendors it.
    pub cutie_source: String,
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
    /// The speed/quality policy the workspace runs under.
    pub performance: PerformanceSettings,
    /// The last segmentation plan built from the drawn prompts. Dormant by
    /// construction: planning never starts a process.
    pub selection_plan: Option<SegmentPlan>,
    /// Why the last attempt to plan a selection was refused.
    pub selection_error: Option<String>,
    /// Runtime files found by the last explicit scan. `None` until the user
    /// asks: scanning `PATH` on every frame would be wasteful and would look
    /// like probing.
    pub detected: Option<Vec<frameforge_roto::RuntimeFile>>,
}

impl Default for RotoEngineState {
    fn default() -> Self {
        Self {
            consent: false,
            run: None,
            last_run: None,
            cutie_source: String::new(),
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
            performance: PerformanceSettings::default(),
            selection_plan: None,
            selection_error: None,
            detected: None,
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

    /// Keep the working size, the mask canvas and the speed policy describing
    /// the same image. The policy's area ceiling applies, so a huge value is
    /// reduced instead of becoming a huge tensor.
    pub fn set_working_size(&mut self, width: u32, height: u32) -> (u32, u32) {
        self.performance.max_width = width.max(16);
        self.performance.max_height = height.max(16);
        self.performance = self.performance.sanitised();
        (self.performance.max_width, self.performance.max_height)
    }

    /// Turn the prompts drawn in the Masking workspace into a segmentation
    /// plan. This is a *plan*: it names the model, the worker, the request file
    /// and the arguments, and starts nothing. Every refusal names what is
    /// missing, so the panel never shows a runnable-looking command for a
    /// backend that is not installed.
    pub fn plan_selection(
        &self,
        prompts: &SelectionSession,
        width: u32,
        height: u32,
        mask: Option<&Path>,
        python: &str,
        frame: i64,
    ) -> Result<SegmentPlan, String> {
        let model = find(&self.selection_model).ok_or_else(|| format!("{} is not in the model catalogue", self.selection_model))?;
        if prompts.for_frame(frame).is_empty() {
            return Err("draw at least one prompt for this frame first".to_string());
        }
        let python = python.trim();
        if python.is_empty() {
            return Err("set the Python interpreter in the runtime section first".to_string());
        }
        let (width, height) = (width.max(16), height.max(16));
        let max_side = self.performance.max_width.max(self.performance.max_height).max(width).max(height);
        let mut request = SegmentRequest::new(frame, width, height, prompts.for_frame(frame), max_side).map_err(|error| error.to_string())?;
        if request.prompts.iter().any(|prompt| matches!(prompt, Prompt::Mask { .. })) {
            let mask = mask.ok_or_else(|| "a mask prompt needs a saved mask: press \"Save as PNG…\" first".to_string())?;
            request = request.with_mask(mask);
        }
        let weights = self.probe().found_weights(&self.selection_model);
        if weights.is_empty() {
            return Err(format!("{} is not installed: install its checkpoint in the model manager first", self.selection_model));
        }
        let worker = PathBuf::from(worker_script_for(&self.selection_model));
        plan_segment(&request, &worker, Path::new(python), model, &weights, &self.models_root_path().join("selection"), false)
            .map_err(|error| error.to_string())
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
        let Some(model) = find(model_id) else {
            self.message = Some(format!("{model_id} is not in the catalogue"));
            return;
        };
        let expected = self.pinned_digest(model_id).map(str::to_string);
        // Hash first, so the borrow of the install record cannot overlap the
        // message write (and a long hash never holds a lock).
        match models::verify_file(path, expected.as_deref()) {
            Ok(digest) => {
                if let Some(install) = self.install_mut(model.id) {
                    install.state = InstallState::Installed { sha256: digest.clone() };
                    install.file = Some(path.to_path_buf());
                    install.expected_sha256 = expected.clone();
                }
                self.message = Some(format!("{} verified: {digest}", model.id));
                if expected.is_none() {
                    // No publisher digest was known: pin what we measured so the
                    // next verification is a real check.
                    self.pinned.push((model.id, digest));
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

    /// The interpreter a run would use: the one configured in the XMem runtime
    /// section, or the first one found on PATH.
    fn interpreter(&self, inputs: &RunInputs) -> Option<PathBuf> {
        let configured = inputs.python.trim();
        if configured.is_empty() { inputs.python_suggestion.clone() } else { Some(PathBuf::from(configured)) }
    }

    /// The backend's own checkout, when it needs one. FrameForge ships neither.
    fn checkout_dir(&self, inputs: &RunInputs, model_id: &str) -> Option<PathBuf> {
        let path = if model_id.starts_with("cutie") {
            self.cutie_source.trim()
        } else if model_id.starts_with("xmem") {
            inputs.xmem_source.trim()
        } else {
            ""
        };
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    /// The gate the runner checks before anything starts.
    pub fn run_gate(&self, inputs: &RunInputs, model_id: &str) -> crate::roto_run::RunGate {
        let model = find(model_id);
        crate::roto_run::RunGate {
            model_id: model_id.to_string(),
            // Prohibited and unreviewed licences may only be run as external tools
            // the user installed; this build refuses to drive them at all.
            licence_permits: model.is_some_and(|model| model.license.redistribution != Redistribution::Prohibited),
            // A checkout is the "runtime": for backends that need one, it must be a
            // directory; backends without one rely on their worker to say so.
            runtime_installed: self.checkout_dir(inputs, model_id).is_none_or(|dir| dir.is_dir()),
            weights_present: self.probe().is_installed(model_id),
            interpreter: self.interpreter(inputs),
            worker: PathBuf::from(worker_script_for(model_id)),
            consent: self.consent,
        }
    }

    /// Start a run of the selected backend on the selected clip.
    ///
    /// The interpreter, worker script and weights come from the engine's own
    /// plan; the panel only adds the backend's checkout path, which the plan
    /// cannot know. The caller snapshots the workspace into [`RunInputs`] first,
    /// so this never needs two simultaneous borrows of the app.
    pub fn start_run(&mut self, inputs: &RunInputs) -> Result<(), String> {
        if let Some(run) = &self.run {
            return Err(format!("{} is already running", run.model_id));
        }
        let model_id = self.tracking_model.clone();
        crate::roto_run::gate(&self.run_gate(inputs, &model_id))?;

        let interpreter = self.interpreter(inputs).ok_or_else(|| "no Python interpreter is configured; set one in the runtime section below".to_string())?;
        let registry = self.backend_registry(worker_script_for(&model_id), interpreter);
        let planned = registry.plan(&model_id, &inputs.track_request(), true).map_err(|error| format!("cannot plan a {model_id} run: {error}"))?;
        let command = planned.command.ok_or_else(|| format!("{model_id} is not an external backend"))?;
        let weights = planned.weights.first().cloned();
        let digest = self.pinned_digest(&model_id).unwrap_or("").to_string();
        if model_id.starts_with("xmem") && digest.is_empty() {
            return Err("pin the XMem weights' SHA-256 in the runtime section before running".to_string());
        }

        let mut args = command.args;
        args.extend(backend_args(inputs, &model_id, &digest, self.performance.frame_stride)?);

        let request = crate::roto_run::RunRequest {
            program: PathBuf::from(command.program),
            args,
            model_id: model_id.clone(),
            licence: find(&model_id).map(|model| model.license.spdx.to_string()).unwrap_or_default(),
            weights_path: weights,
            weights_sha256: if digest.is_empty() { "0".repeat(64) } else { digest },
            object_id: 1,
            doc: inputs.doc.clone(),
            item: inputs.item,
            range: inputs.range,
            rate: inputs.rate,
            working: inputs.working,
            initial_mask: inputs.initial_mask.clone(),
            direction: inputs.direction,
            limits: frameforge_bridge::RotoJobLimits::default(),
            cache_root: self.models_root_path().join("cache"),
            r1_root: frameforge_bridge::default_cache_root(),
        };
        let handle = crate::roto_run::start(request)?;
        self.run = Some(BackendRun { handle, model_id });
        self.message = None;
        Ok(())
    }

    /// Watch the running backend job; adopt its report when it finishes.
    pub fn poll_run(&mut self) {
        let Some(result) = self.run.as_mut().and_then(|run| run.handle.try_finish()) else { return };
        let Some(run) = self.run.take() else { return };
        let model = run.model_id;
        match result {
            Ok(report) => {
                self.message = Some(format!("{model}: {}", report.summary));
                self.last_run = Some(report);
            }
            Err(error) => {
                self.message = Some(format!("{model}: {error}"));
                self.last_run = None;
            }
        }
    }

    pub fn run_progress(&self) -> Option<String> {
        self.run.as_ref().and_then(|run| run.handle.progress())
    }

    pub fn cancel_run(&mut self) {
        if let Some(run) = &self.run {
            run.handle.request_cancel();
        }
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

    performance_section(app, panel);
    selection_section(app, panel);
    run_controls(app, panel);
}

/// The real speed levers, with what each one costs.
fn performance_section(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "SPEED AND QUALITY");
    panel.label(ui::hint(
        "Resolution and stride are the levers that matter on integrated graphics. The mask cache still holds every frame; a stride repeats the last mask in between, and the QC review flags the stutter that causes.",
    ));
    let mut perf = app.roto_engine.performance;
    panel.horizontal(|ui| {
        ui.label(ui::meta("Working size"));
        ui.add(egui::DragValue::new(&mut perf.max_width).range(64..=3_840).prefix("w "));
        ui.add(egui::DragValue::new(&mut perf.max_height).range(64..=3_840).prefix("h "));
    });
    panel.add(egui::Slider::new(&mut perf.frame_stride, 1..=8).text("propagate every Nth frame"));
    panel.add(egui::Slider::new(&mut perf.memory_budget_mb, 512..=16_384).text("memory budget (MB)"));
    panel.checkbox(&mut perf.background, "keep inference in the background");
    let clean = perf.sanitised();
    app.roto_engine.performance = clean;
    if app.roto.width != clean.max_width || app.roto.height != clean.max_height {
        // The canvas is the mask at this size, so it cannot survive a resize.
        app.roto.width = clean.max_width;
        app.roto.height = clean.max_height;
        app.roto.clear_canvas();
        app.status = "Working size changed; the initial-mask canvas was cleared".to_string();
    }
    panel.label(ui::hint(&clean.summary((clean.max_width, clean.max_height))));
    panel.label(ui::hint(
        "The per-frame estimate is arithmetic on the working size, not a measurement. FrameForge does not print benchmark numbers it did not measure.",
    ));
}

/// Prompts drawn in the Masking workspace, the compatibility route for the
/// chosen selector, and the dormant plan they produce.
fn selection_section(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "SELECTION PROMPTS");
    panel.label(ui::hint(
        "Draw clicks, a box or a mask in the Masking workspace, then plan the selection here. Planning writes nothing and starts nothing: it names the worker and the request.",
    ));
    if app.roto.prompts.is_empty() {
        panel.label(ui::hint("No prompts yet. The Masking workspace has Add point / Remove point / Box tools."));
    } else {
        panel.label(ui::hint(&app.roto.prompts.summary()));
    }

    if let Some(row) = frameforge_roto::compat::find(&app.roto_engine.selection_model) {
        panel.label(ui::hint(&format!(
            "{} · ONNX export {} · Windows ML {} · providers: {}",
            row.path.label(),
            row.onnx_export.label(),
            row.windows_ml.label(),
            row.candidate_providers.join(", ")
        )));
        panel.label(ui::hint(row.note));
    }
    if ui::mini_button(
        panel,
        "Scan for runtime files",
        "Look beside the app and on PATH for DirectML, Windows ML and torch DLLs. Presence is not compatibility and nothing is loaded.",
    )
    .clicked()
    {
        let dirs = frameforge_roto::compat::standard_dirs();
        let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
        app.roto_engine.detected = Some(frameforge_roto::compat::detect_files(&refs));
    }
    if let Some(files) = &app.roto_engine.detected {
        panel.label(ui::hint(&frameforge_roto::compat::detection_note(files)));
    }

    let mask = (!app.roto.initial_mask.trim().is_empty()).then(|| PathBuf::from(app.roto.initial_mask.trim()));
    let python = app.roto.python.clone();
    if ui::accent_button(panel, "Plan selection (no inference)").clicked() {
        let planned = app.roto_engine.plan_selection(&app.roto.prompts, app.roto.width.max(16), app.roto.height.max(16), mask.as_deref(), &python, 0);
        match planned {
            Ok(plan) => {
                app.roto_engine.selection_error = None;
                app.roto_engine.selection_plan = Some(plan);
            }
            Err(error) => {
                app.roto_engine.selection_plan = None;
                app.roto_engine.selection_error = Some(error);
            }
        }
    }
    if let Some(plan) = &app.roto_engine.selection_plan {
        panel.label(ui::hint(&format!("{} · {} · dormant: nothing will start", plan.protocol, plan.model)));
        panel.label(egui::RichText::new(plan.command_line()).size(9.5).monospace());
        let worker = plan.command.args.get(1).map(PathBuf::from);
        match worker {
            Some(worker) if worker.is_file() => {
                panel.label(ui::hint("The worker script is present. The selection runner is the next increment; no frame is read until it exists."));
            }
            _ => {
                panel.label(
                    egui::RichText::new("The segmentation worker is not shipped in this build, so this plan cannot run yet.").size(10.0).color(ui::WARN),
                );
            }
        }
    }
    if let Some(error) = &app.roto_engine.selection_error {
        panel.label(egui::RichText::new(error).size(10.0).color(ui::DANGER));
    }
}

/// The workspace as it was when the user pressed Run: a snapshot, so the engine
/// state can be borrowed mutably on its own while the plan is built.
#[derive(Clone)]
pub struct RunInputs {
    pub doc: frameforge_bridge::FrameForgeProject,
    pub item: filmcraft_project::ItemId,
    pub range: filmcraft_time::TimeRange,
    pub rate: filmcraft_time::FrameRate,
    pub working: (u32, u32),
    pub initial_mask: PathBuf,
    pub direction: frameforge_bridge::WorkerDirection,
    pub python: String,
    pub python_suggestion: Option<PathBuf>,
    pub cutie_source: String,
    pub xmem_source: String,
}

impl RunInputs {
    pub fn from_app(app: &App) -> Result<Self, String> {
        let Some((clip, item)) = app.selected_clip() else {
            return Err("select a clip on the timeline first".to_string());
        };
        let range = app.clip_range(clip).ok_or_else(|| "the selected clip is not in the sequence".to_string())?;
        Ok(Self {
            doc: app.document(),
            item,
            range,
            rate: app.sequence_rate(),
            working: (app.roto.width.max(16), app.roto.height.max(16)),
            initial_mask: PathBuf::from(app.roto.initial_mask.trim()),
            direction: if app.roto.forward { frameforge_bridge::WorkerDirection::Forward } else { frameforge_bridge::WorkerDirection::Backward },
            python: app.roto.python.clone(),
            python_suggestion: app.roto.python_suggestion(),
            cutie_source: app.roto_engine.cutie_source.clone(),
            xmem_source: app.roto.xmem_source.clone(),
        })
    }

    /// A minimal request used only to *describe* (and validate) the run: one
    /// object, the clip's own range, the working size.
    fn track_request(&self) -> frameforge_roto::TrackRequest {
        use frameforge_roto::engine::ObjectSpec;
        use frameforge_roto::selection::Prompt;

        let object = ObjectSpec::new(1, "Subject").with_prompts(vec![Prompt::Mask { frame: 0 }]);
        let (first, last) = crate::roto::frame_range(self.rate, self.range).unwrap_or((0, 23));
        frameforge_roto::TrackRequest::new(object, FrameRange::new(first, last), Direction::Forward, self.working)
    }
}

/// Consent, the backend's checkout, the run button, live progress, cancel and the
/// attach action.
fn run_controls(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "RUN THE SELECTED BACKEND");
    let model_id = app.roto_engine.tracking_model.clone();
    let model = find(&model_id);
    let installed = app.roto_engine.probe().is_installed(&model_id);
    let licence_line = match model {
        Some(model) => format!("{} · {} · {}", model.name, model.license.spdx, if installed { "installed" } else { "not installed" }),
        None => "no backend selected".to_string(),
    };
    panel.label(ui::hint(&licence_line));

    if model_id.starts_with("cutie") {
        crate::roto::path_row(panel, "Cutie checkout", &mut app.roto_engine.cutie_source, "cutie", true);
    }
    panel.checkbox(&mut app.roto_engine.consent, "I installed this backend's runtime and weights myself and accept its licence");
    ui::kv(panel, "Initial mask", if app.roto.initial_mask.trim().is_empty() { "none yet".to_string() } else { app.roto.initial_mask.clone() });

    let running = app.roto_engine.run.is_some();
    if running {
        if let Some(line) = app.roto_engine.run_progress() {
            panel.label(ui::live(&line));
        } else {
            panel.label(ui::hint("Starting the backend…"));
        }
        if ui::full_button(panel, "Cancel run").clicked() {
            app.roto_engine.cancel_run();
            app.status = "Cancelling the backend run…".to_string();
        }
    } else if ui::accent_button(panel, "Run on the selected clip").clicked() {
        match RunInputs::from_app(app) {
            Ok(inputs) => match app.roto_engine.start_run(&inputs) {
                Ok(()) => app.status = format!("{model_id} run started"),
                Err(error) => app.roto_engine.message = Some(error),
            },
            Err(error) => app.roto_engine.message = Some(error),
        }
    }
    if let Some(message) = app.roto_engine.message.clone() {
        panel.label(egui::RichText::new(message).size(10.5).color(ui::TEXT_MID));
    }

    // Attaching is the same undoable document edit the XMem panel uses.
    let attachable: Option<(String, filmcraft_project::ItemId)> = match app.roto_engine.last_run.as_ref() {
        Some(report) if crate::roto_run::attach_available(report) => report.r1_key.clone().zip(report.item),
        _ => None,
    };
    if let Some((key, item)) = attachable {
        let already = app.compositions.get(&item).and_then(|comp| comp.roto.as_ref()).is_some_and(|roto| roto.key == key);
        if already {
            panel.label(ui::hint("This clip already uses the matte from this run."));
        } else if ui::accent_button(panel, "Use this matte on the clip").clicked() {
            crate::roto::attach_matte(app, item, &key);
        }
    }

    review_controls(app, panel);
    ui::footer_hint(
        panel,
        "The run exports the clip's frames, drives the backend's worker over protocol 1 and publishes the masks into both caches; the renderer reads the R1 one.",
    );
}

/// The temporal QC verdict over the finished run: flagged frames are one click
/// away, and each kind says what to do about it. All arithmetic on the masks —
/// nothing here ran a model.
fn review_controls(app: &mut App, panel: &mut egui::Ui) {
    let Some(report) = app.roto_engine.last_run.clone() else { return };
    ui::section(panel, "REVIEW THE RUN");
    panel.label(ui::hint(&report.provenance));
    let Some(qc) = report.qc else {
        panel.label(ui::hint("The masks were published, but they could not be read back for review."));
        return;
    };
    if qc.is_clean() {
        panel.label(ui::hint(&qc.summary()));
        return;
    }
    panel.label(egui::RichText::new(qc.summary()).size(10.5).color(ui::WARN));
    for range in &qc.flagged {
        let kinds: Vec<&str> = {
            let mut kinds: Vec<&str> = range.kinds.iter().map(|kind| kind.label()).collect();
            kinds.dedup();
            kinds
        };
        panel.label(ui::hint(&format!("frames {}-{} · {} · severity {:.2}", range.first, range.last, kinds.join(", "), range.worst_severity)));
    }
    // The worst flag decides what the workspace suggests; the list is ascending so
    // skipping the first frame of each range walks the run in order.
    if let Some(worst) = qc.signals.iter().max_by(|a, b| a.severity.total_cmp(&b.severity)) {
        panel.label(ui::hint(&format!("Suggested fix: {}", worst.kind.suggested_action().label())));
    }
    panel.horizontal_wrapped(|ui| {
        for frame in qc.review_frames() {
            if ui.small_button(format!("frame {frame}")).clicked() {
                app.goto_frame(frame);
            }
        }
    });
}

/// The worker script for a backend: the new backends have real adapters, XMem
/// keeps the one its panel already drives, and anything else uses the protocol
/// stub, which answers honestly and refuses to fabricate masks.
fn worker_script_for(model_id: &str) -> &'static str {
    if model_id.starts_with("cutie") {
        "tools/cutie_worker.py"
    } else if model_id.starts_with("xmem") {
        "tools/xmem_worker.py"
    } else if model_id.starts_with("sam2") || model_id.starts_with("edgetam") {
        "tools/segment_worker.py"
    } else {
        "tools/roto_worker.py"
    }
}

/// The flags only the backend itself understands: its checkout, the pinned
/// digest it must verify before loading, and a propagation stride for the
/// tracker that implements one.
fn backend_args(inputs: &RunInputs, model_id: &str, digest: &str, stride: u32) -> Result<Vec<String>, String> {
    if model_id.starts_with("cutie") {
        let source = inputs.cutie_source.trim();
        if source.is_empty() {
            return Err("set the Cutie checkout path first: the worker imports Cutie from your own copy".to_string());
        }
        let mut args = vec!["--cutie-source".to_string(), source.to_string(), "--device".to_string(), "cpu".to_string()];
        if !digest.is_empty() {
            args.push("--weights-sha256".to_string());
            args.push(digest.to_string());
        }
        if stride > 1 {
            args.push("--stride".to_string());
            args.push(stride.to_string());
        }
        Ok(args)
    } else if model_id.starts_with("xmem") {
        Ok(vec!["--xmem-source".to_string(), inputs.xmem_source.trim().to_string()])
    } else {
        Ok(Vec::new())
    }
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
        if let Some(bytes) = total_bytes(&["cutie-base", "sam2.1-tiny"]) {
            ui.label(ui::hint(&format!("Cutie + SAM 2.1 Tiny: {:.2} GB", bytes as f64 / 1e9)));
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
