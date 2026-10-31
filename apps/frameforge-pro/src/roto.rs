//! Roto AI (XMem) in the Masking workspace: point at a runtime, consent, verify,
//! then run one real propagation job on the selected clip.
//!
//! XMem is optional and external. This panel never downloads anything, never
//! guesses where a runtime lives and never claims inference it did not run: the
//! *Verify runtime* step is [`RotoRuntime::resolve`] (consent, pinned commit,
//! streamed SHA-256) and the job is [`export_frames`] +
//! [`XmemWorker`] + [`run_job`] on a worker thread, so the UI stays responsive
//! while a slow CPU pass runs and the user can cancel it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use filmcraft_render::SourceProvider;
use filmcraft_time::{FrameRate, Tick, TimeRange};
use frameforge_bridge::{
    ProjectSources, RotoConsent, RotoJobLimits, RotoJobOutcome, RotoJobSpec, RotoRuntime, RotoRuntimeRequest, XmemWorker, export_frames,
    python_candidates_from_env, run_job,
};

use crate::app::App;
use crate::ui;

/// How long a worker gets to answer `hello` before it is killed.
const HANDSHAKE: Duration = Duration::from_secs(120);
/// A working size the measured target laptop handled (480x270, ~4.4 s/pass).
const DEFAULT_WORKING: (u32, u32) = (480, 270);

/// A job running on its own thread.
pub struct RotoJob {
    cancel: Arc<AtomicBool>,
    done: Arc<Mutex<Option<Result<String, String>>>>,
    progress: Arc<Mutex<String>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// Everything the panel remembers. All of it is session state: a path is never
/// saved into the project and no weight or runtime is ever copied into it.
pub struct RotoState {
    pub python: String,
    pub xmem_source: String,
    pub weights: String,
    pub worker_script: String,
    pub expected_sha256: String,
    pub consent: bool,
    pub consent_note: String,
    pub initial_mask: String,
    pub width: u32,
    pub height: u32,
    pub forward: bool,
    /// The verified runtime, or `None` until *Verify runtime* succeeds.
    pub runtime: Option<RotoRuntime>,
    pub verify_error: Option<String>,
    pub job: Option<RotoJob>,
    pub report: Option<String>,
    pub job_error: Option<String>,
}

impl Default for RotoState {
    fn default() -> Self {
        Self {
            python: String::new(),
            xmem_source: String::new(),
            weights: String::new(),
            worker_script: String::new(),
            expected_sha256: String::new(),
            consent: false,
            consent_note: String::new(),
            initial_mask: String::new(),
            width: DEFAULT_WORKING.0,
            height: DEFAULT_WORKING.1,
            forward: true,
            runtime: None,
            verify_error: None,
            job: None,
            report: None,
            job_error: None,
        }
    }
}

impl RotoState {
    /// The request the user's fields describe. Empty fields are named, so the
    /// panel can say exactly what is missing instead of failing later.
    pub fn request(&self) -> Result<RotoRuntimeRequest, String> {
        let field = |value: &str, name: &str| -> Result<PathBuf, String> {
            let value = value.trim();
            if value.is_empty() { Err(format!("{name} is not set")) } else { Ok(PathBuf::from(value)) }
        };
        let expected = self.expected_sha256.trim().to_ascii_lowercase();
        if expected.is_empty() {
            return Err("the weights SHA-256 is not set".to_string());
        }
        Ok(RotoRuntimeRequest {
            python: field(&self.python, "the Python interpreter")?,
            xmem_source: field(&self.xmem_source, "the XMem checkout")?,
            weights: field(&self.weights, "the weights file")?,
            worker_script: field(&self.worker_script, "the worker script")?,
            expected_sha256: expected,
        })
    }

    pub fn consent(&self) -> RotoConsent {
        let note = if self.consent_note.trim().is_empty() { "consent recorded in the Ask tab".to_string() } else { self.consent_note.trim().to_string() };
        if self.consent { RotoConsent::given(note) } else { RotoConsent::refused(note) }
    }

    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Resolve and verify the runtime the fields describe.
    pub fn verify(&mut self) {
        self.verify_error = None;
        match self.request().and_then(|request| RotoRuntime::resolve(&request, &self.consent()).map_err(|error| error.to_string())) {
            Ok(runtime) => self.runtime = Some(runtime),
            Err(error) => {
                self.runtime = None;
                self.verify_error = Some(error);
            }
        }
    }

    /// Adopt the job's result once its thread has finished. Cheap: it locks one
    /// small mutex and only joins when there is a result to take. A thread that
    /// stopped without reporting anything (a panic) is reported as such, so the
    /// panel can never sit on "running" forever.
    pub fn poll(&mut self) {
        let finished = self.job.as_ref().and_then(|job| job.done.lock().ok().and_then(|slot| slot.clone()));
        let stopped_without_result = finished.is_none() && self.job.as_ref().is_some_and(|job| job.handle.as_ref().is_some_and(|handle| handle.is_finished()));
        if finished.is_none() && !stopped_without_result {
            return;
        }
        let result = finished.unwrap_or_else(|| Err("the XMem worker thread stopped without reporting a result".to_string()));
        if let Some(mut job) = self.job.take()
            && let Some(handle) = job.handle.take()
        {
            let _ = handle.join();
        }
        match result {
            Ok(report) => {
                self.report = Some(report);
                self.job_error = None;
            }
            Err(error) => {
                self.job_error = Some(error);
                self.report = None;
            }
        }
    }

    pub fn progress_line(&self) -> Option<String> {
        self.job.as_ref().and_then(|job| job.progress.lock().ok().map(|text| text.clone())).filter(|text| !text.is_empty())
    }

    /// The interpreter the probe found, offered when the field is still empty.
    pub fn python_suggestion(&self) -> Option<PathBuf> {
        if !self.python.trim().is_empty() {
            return None;
        }
        python_candidates_from_env().into_iter().next()
    }
}

/// The media frame numbers a clip range covers, computed exactly as
/// [`export_frames`] numbers its files: `frame_at(start) .. frame_at(end) - 1`.
/// Returns `None` for an empty or overflowing range.
pub fn frame_range(rate: FrameRate, range: TimeRange) -> Option<(i64, i64)> {
    let end = range.start.0.checked_add(range.duration.0)?;
    let first = rate.frame_at(range.start);
    let last = rate.frame_at(Tick(end));
    (last > first && first >= 0).then(|| (first, last - 1))
}

/// The panel, drawn into the Masking workspace's right-hand column.
pub fn show(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "ROTO AI (XMEM)");
    panel.label(ui::hint(
        "XMem is the temporal VOS engine. It is optional and external: point at your own runtime, checkout and weights, then consent. Nothing is downloaded, and no mask is claimed until a job reports one.",
    ));

    // --- runtime ---------------------------------------------------------
    runtime_fields(app, panel);
    ui::spaced_rule(panel);
    consent_fields(app, panel);
    if ui::full_button(panel, "Verify runtime").clicked() {
        app.roto.verify();
        app.status = match &app.roto.runtime {
            Some(runtime) => format!("XMem runtime verified: {}", runtime.describe()),
            None => "XMem runtime not verified".to_string(),
        };
    }
    if let Some(runtime) = app.roto.runtime.clone() {
        panel.label(egui::RichText::new("Verified").size(11.0).strong().color(ui::ACCENT_HI));
        panel.label(ui::hint(&runtime.describe()));
    }
    if let Some(error) = app.roto.verify_error.clone() {
        panel.label(egui::RichText::new(error).size(10.5).color(ui::DANGER));
    }

    // --- job -------------------------------------------------------------
    ui::spaced_rule(panel);
    ui::section(panel, "PROPAGATE THE SELECTED CLIP");
    let clip = app.selected_clip();
    let clip_label = match &clip {
        Some((_, item)) => format!("Clip media: {}", app.session.project.items.get(item).map(|it| it.name.clone()).unwrap_or_else(|| "?".to_string())),
        None => "Select a video clip on the timeline.".to_string(),
    };
    panel.label(ui::hint(&clip_label));
    path_row(panel, "Initial mask PNG", &mut app.roto.initial_mask, "PNG", false);
    panel.horizontal(|ui| {
        ui.label(ui::meta("Working size"));
        ui.add(egui::DragValue::new(&mut app.roto.width).range(16..=1920).prefix("w "));
        ui.add(egui::DragValue::new(&mut app.roto.height).range(16..=1080).prefix("h "));
    });
    panel.horizontal(|ui| {
        ui.label(ui::meta("Direction"));
        ui.selectable_value(&mut app.roto.forward, true, "Forward");
        ui.selectable_value(&mut app.roto.forward, false, "Backward");
    });

    if app.roto.is_running() {
        if let Some(line) = app.roto.progress_line() {
            panel.label(ui::live(&line));
        } else {
            panel.label(ui::hint("Starting: exporting frames…"));
        }
        if ui::full_button(panel, "Cancel job").clicked() {
            if let Some(job) = &app.roto.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
            app.status = "Cancelling the XMem job…".to_string();
        }
    } else if ui::accent_button(panel, "Export frames and propagate").clicked() {
        start_job(app);
    }

    if let Some(report) = app.roto.report.clone() {
        panel.label(egui::RichText::new(report).size(10.5).color(ui::ACCENT_HI));
    }
    if let Some(error) = app.roto.job_error.clone() {
        panel.label(egui::RichText::new(error).size(10.5).color(ui::DANGER));
    }
    ui::footer_hint(
        panel,
        "Masks are written as frame_<index>.png next to the exported frames. The R1 cache's manifest writer is still missing, so a run does not yet feed the renderer's alpha — the panel says so rather than implying it.",
    );
}

fn runtime_fields(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "RUNTIME (EXTERNAL)");
    path_row(panel, "Python", &mut app.roto.python, "python.exe", false);
    if let Some(suggestion) = app.roto.python_suggestion() {
        if ui::mini_button(panel, "Use the interpreter found on PATH", &suggestion.display().to_string()).clicked() {
            app.roto.python = suggestion.display().to_string();
        }
    }
    path_row(panel, "XMem checkout", &mut app.roto.xmem_source, "folder", true);
    path_row(panel, "Weights", &mut app.roto.weights, "pth", false);
    path_row(panel, "Worker script", &mut app.roto.worker_script, "py", false);
    panel.horizontal(|ui| {
        ui.label(ui::meta("Weights SHA-256"));
        ui.add(egui::TextEdit::singleline(&mut app.roto.expected_sha256).hint_text("64 hex digits").desired_width(210.0));
    });
}

fn consent_fields(app: &mut App, panel: &mut egui::Ui) {
    ui::section(panel, "CONSENT");
    panel.checkbox(&mut app.roto.consent, "I obtained these weights and accept the XMem licence");
    ui::kv(panel, "Consent", if app.roto.consent { "recorded" } else { "not given" });
    if app.roto.consent {
        panel.horizontal(|ui| {
            ui.label(ui::meta("Note"));
            ui.add(egui::TextEdit::singleline(&mut app.roto.consent_note).hint_text("where they came from").desired_width(200.0));
        });
    } else {
        panel.label(ui::hint("Verification reads nothing until this box is ticked."));
    }
}

/// A labelled path field with a browse button. `folder` picks a directory.
fn path_row(panel: &mut egui::Ui, label: &str, value: &mut String, filter: &str, folder: bool) {
    panel.label(ui::meta(label));
    panel.horizontal(|ui| {
        let width = (ui.available_width() - 74.0).max(60.0);
        ui.add(egui::TextEdit::singleline(value).desired_width(width));
        if ui.small_button("Browse…").clicked() {
            let picked = if folder { rfd::FileDialog::new().pick_folder() } else { rfd::FileDialog::new().add_filter(filter, &[filter]).pick_file() };
            if let Some(path) = picked {
                *value = path.display().to_string();
            }
        }
    });
}

/// Start one job: export the selected clip's frames, spawn the verified worker
/// and drive it under [`RotoJobLimits`] on a worker thread.
fn start_job(app: &mut App) {
    app.roto.job_error = None;
    app.roto.report = None;

    let Some(runtime) = app.roto.runtime.clone() else {
        app.roto.job_error = Some("Verify the runtime first.".to_string());
        return;
    };
    let Some((clip, item)) = app.selected_clip() else {
        app.roto.job_error = Some("Select a video clip on the timeline first.".to_string());
        return;
    };
    let Some(seq) = app.sequence() else {
        app.roto.job_error = Some("No sequence.".to_string());
        return;
    };
    let Some((_, clip_item)) = seq.find_item(clip) else {
        app.roto.job_error = Some("The selected clip is no longer in the sequence.".to_string());
        return;
    };
    let range = TimeRange::new(clip_item.start, clip_item.duration);
    let rate = seq.settings.frame_rate;
    let Some((first, last)) = frame_range(rate, range) else {
        app.roto.job_error = Some("The selected clip has no frames to propagate.".to_string());
        return;
    };
    let initial_mask = app.roto.initial_mask.trim().to_string();
    if !Path::new(&initial_mask).is_file() {
        app.roto.job_error = Some("Choose an initial mask PNG for the first frame.".to_string());
        return;
    }

    let (width, height) = (app.roto.width.max(16), app.roto.height.max(16));
    let direction = if app.roto.forward { frameforge_bridge::WorkerDirection::Forward } else { frameforge_bridge::WorkerDirection::Backward };
    let root = std::env::temp_dir().join(format!("frameforge-roto-{}", std::process::id()));
    let frames_dir = root.join("frames");
    let output_dir = root.join("masks");
    let spec = RotoJobSpec {
        frames_dir: frames_dir.display().to_string(),
        first_frame: first,
        last_frame: last,
        initial_mask,
        output_dir: output_dir.display().to_string(),
        width,
        height,
        model: runtime.weights.display().to_string(),
        model_sha256: runtime.weights_sha256.clone(),
        direction,
    };

    let doc = app.document();
    let limits = RotoJobLimits::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let done: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
    let progress = Arc::new(Mutex::new(String::new()));

    let thread_cancel = Arc::clone(&cancel);
    let thread_done = Arc::clone(&done);
    let thread_progress = Arc::clone(&progress);
    let handle = std::thread::Builder::new().name("frameforge-roto-job".to_string()).spawn(move || {
        let run = JobRun { doc: &doc, item, range, rate, runtime: &runtime, spec: &spec, limits: &limits };
        let result = execute_job(run, &thread_cancel, &thread_progress);
        if let Ok(mut slot) = thread_done.lock() {
            *slot = Some(result);
        }
    });
    let Ok(handle) = handle else {
        app.roto.job_error = Some("Could not start the worker thread.".to_string());
        return;
    };

    app.roto.job = Some(RotoJob { cancel, done, progress, handle: Some(handle) });
    app.status = format!("XMem job started: frames {first}..{last} at {width}x{height}");
}

/// Everything one job thread needs, so the function below stays readable.
struct JobRun<'a> {
    doc: &'a frameforge_bridge::FrameForgeProject,
    item: filmcraft_project::ItemId,
    range: TimeRange,
    rate: FrameRate,
    runtime: &'a RotoRuntime,
    spec: &'a RotoJobSpec,
    limits: &'a RotoJobLimits,
}

/// The worker thread's whole body: export, spawn, handshake, run.
fn execute_job(run: JobRun<'_>, cancel: &AtomicBool, progress: &Arc<Mutex<String>>) -> Result<String, String> {
    let JobRun { doc, item, range, rate, runtime, spec, limits } = run;
    let set_progress = |text: String| {
        if let Ok(mut slot) = progress.lock() {
            *slot = text;
        }
    };

    set_progress("Exporting frames…".to_string());
    let sources = ProjectSources::build(doc).map_err(|error| error.to_string())?;
    let source = sources.source(item).ok_or_else(|| "The clip's media has no readable source.".to_string())?;
    let exported = export_frames(source.as_ref(), range, rate, Path::new(&spec.frames_dir), (spec.width, spec.height)).map_err(|error| error.to_string())?;
    if exported.first_frame != spec.first_frame || exported.last_frame != spec.last_frame {
        return Err(format!(
            "frame numbering drifted: exported {}..{} but the job asked for {}..{}",
            exported.first_frame, exported.last_frame, spec.first_frame, spec.last_frame
        ));
    }

    set_progress(format!("Frames ready ({} at {}x{}); starting the worker…", exported.count, exported.width, exported.height));
    let (program, args) = runtime.worker_command();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut worker = XmemWorker::spawn(&program, &args).map_err(|error| error.to_string())?;
    let info = worker.handshake_timeout(HANDSHAKE).map_err(|error| format!("worker handshake: {error}"))?;
    set_progress(format!("Worker {} ready (protocol {}); propagating…", info.backend, info.protocol));

    let outcome = run_job(&mut worker, spec, limits, cancel, |frame, total, fps| {
        let done = (frame - spec.first_frame + 1).max(0);
        set_progress(format!("Propagating {done}/{total} frames (frame {frame}, {fps:.2} fps)"));
    })
    .map_err(|error| error.to_string())?;

    let _ = worker.request_shutdown();
    match outcome {
        RotoJobOutcome::Done(report) => {
            Ok(format!("Done: {} frames in {:.1}s ({} progress events) → {}", report.frames, report.elapsed, report.progress_events, spec.output_dir))
        }
        RotoJobOutcome::Cancelled(report) => {
            if report.killed {
                Err(format!("Cancelled: the worker did not acknowledge, so it was killed after {} progress events", report.progress_events))
            } else {
                Ok(format!("Cancelled by the worker after {} progress events", report.progress_events))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RotoState, frame_range};
    use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick, TimeRange};

    const R: FrameRate = FrameRate::FPS_24;

    #[test]
    fn a_request_names_the_first_missing_field() {
        let mut state = RotoState::default();
        assert_eq!(state.request().expect_err("nothing is set"), "the weights SHA-256 is not set");

        state.expected_sha256 = "AB".repeat(32);
        assert_eq!(state.request().expect_err("no paths"), "the Python interpreter is not set");

        state.python = " C:\\py\\python.exe ".to_string();
        state.xmem_source = "C:\\XMem".to_string();
        state.weights = "C:\\XMem.pth".to_string();
        state.worker_script = "C:\\tools\\xmem_worker.py".to_string();
        let request = state.request().expect("all fields are set");
        assert_eq!(request.python.to_string_lossy(), "C:\\py\\python.exe", "paths are trimmed");
        assert_eq!(request.expected_sha256, "ab".repeat(32), "the hash is lowercased for comparison");
    }

    #[test]
    fn consent_is_refused_until_the_box_is_ticked() {
        let mut state = RotoState { consent_note: "bought a licence".to_string(), ..RotoState::default() };
        assert!(!state.consent().accepted);
        state.consent = true;
        assert!(state.consent().accepted);
        assert_eq!(state.consent().note, "bought a licence");
    }

    #[test]
    fn frame_range_matches_the_exporter_numbering() {
        let range = TimeRange::new(Tick::ZERO, R.tick_of(3));
        assert_eq!(frame_range(R, range), Some((0, 2)), "three frames are 0..2");
        let offset = TimeRange::new(Tick(TICKS_PER_SECOND), R.tick_of(1));
        assert_eq!(frame_range(R, offset), Some((24, 24)));
        assert_eq!(frame_range(R, TimeRange::new(Tick::ZERO, Tick::ZERO)), None, "an empty range has no frames");
    }
}
