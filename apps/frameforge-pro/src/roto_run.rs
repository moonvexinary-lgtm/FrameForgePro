//! Running any engine backend over the shared protocol: export the clip's frames,
//! spawn the backend's worker, drive it under the job runner, then publish the
//! masks so the timeline can use them.
//!
//! The transport is the bridge's protocol-1 client — the same one XMem uses — so a
//! new backend is a new worker script, not a new protocol. Two rules hold here:
//!
//! * nothing starts without passing [`gate`]: the licence must permit the backend,
//!   the interpreter and worker script must exist, the model must be installed
//!   and the user must have consented. Opening the app starts nothing.
//! * a finished run is published twice: into the multi-object cache of
//!   `frameforge-roto`, and into the bridge's R1 mask cache (the layout the
//!   renderer reads), so "Use this matte on the clip" works for every backend.

#![allow(dead_code)] // The run surface is complete; the panel reports what it uses.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use filmcraft_project::ItemId;
use filmcraft_render::SourceProvider;
use filmcraft_time::{FrameRate, TimeRange};
use frameforge_bridge::{
    FrameForgeProject, MaskAdoption, MaskCache, ProjectSources, RotoCacheKey, RotoJobLimits, RotoJobOutcome, RotoJobSpec, WorkerDirection, XmemWorker,
    export_frames, run_job,
};
use frameforge_roto::cache::{self, MaskKey};
use frameforge_roto::matting::{MatteMode, Plane};

use crate::roto::{frame_range, source_identity};

/// How long a worker gets to answer `hello`.
const HANDSHAKE: Duration = Duration::from_secs(180);

/// Everything the workspace must have decided before a backend may run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunGate {
    pub model_id: String,
    /// The licence allows this build to run the backend.
    pub licence_permits: bool,
    /// The user installed the runtime and the weights themselves.
    pub runtime_installed: bool,
    /// The model's weights are on disk.
    pub weights_present: bool,
    /// The interpreter that would run the worker.
    pub interpreter: Option<PathBuf>,
    /// The worker script for this backend.
    pub worker: PathBuf,
    /// The user's explicit consent for this backend.
    pub consent: bool,
}

/// Why a run may not start. Ordered so the most fundamental problem is reported
/// first: licence, then what is missing, then consent.
pub fn gate(gate: &RunGate) -> Result<(), String> {
    if !gate.licence_permits {
        return Err(format!(
            "{} is not permitted in this build: its licence does not allow redistribution, so it can only be run as an external tool you installed yourself",
            gate.model_id
        ));
    }
    if gate.interpreter.as_ref().is_none_or(|path| !path.is_file()) {
        return Err("no Python interpreter is configured; set one in the XMem panel's runtime section".to_string());
    }
    if !gate.worker.is_file() {
        return Err(format!(
            "the worker script for {} is not shipped in this build ({}); install the backend's worker and point the panel at it",
            gate.model_id,
            gate.worker.display()
        ));
    }
    if !gate.weights_present {
        return Err(format!("{} is not installed: install its weights first", gate.model_id));
    }
    if !gate.runtime_installed {
        return Err(format!("{} has no runtime on this machine: install it and point the panel at it", gate.model_id));
    }
    if !gate.consent {
        return Err(format!("consent for {} has not been recorded", gate.model_id));
    }
    Ok(())
}

/// Everything one run needs. Built by the panel, handed to a thread.
pub struct RunRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub model_id: String,
    /// The weight file the worker should read, when the plan resolved one.
    pub weights_path: Option<PathBuf>,
    /// The weights digest the manager verified, or 64 zeros when the user has not
    /// pinned one yet. It reaches the worker (which checks its own file) and the
    /// cache key, so a changed model addresses a different cache.
    pub weights_sha256: String,
    pub object_id: u64,
    pub doc: FrameForgeProject,
    pub item: ItemId,
    pub range: TimeRange,
    pub rate: FrameRate,
    pub working: (u32, u32),
    pub initial_mask: PathBuf,
    pub direction: WorkerDirection,
    pub limits: RotoJobLimits,
    /// Where the multi-object cache keeps this project's masks.
    pub cache_root: PathBuf,
    /// Where the bridge's R1 cache keeps the matte the renderer reads.
    pub r1_root: PathBuf,
}

/// What a finished run produced.
#[derive(Clone, Debug, Default)]
pub struct RunReport {
    pub summary: String,
    /// The multi-object cache digest, when the run published one.
    pub cache_key: Option<String>,
    /// The R1 cache key, when the run published the renderer's matte.
    pub r1_key: Option<String>,
    pub item: Option<ItemId>,
    pub frames: i64,
    pub killed: bool,
}

/// A run in flight.
pub struct RunHandle {
    cancel: Arc<AtomicBool>,
    done: Arc<Mutex<Option<Result<RunReport, String>>>>,
    progress: Arc<Mutex<String>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl RunHandle {
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn progress(&self) -> Option<String> {
        self.progress.lock().ok().map(|text| text.clone()).filter(|text| !text.is_empty())
    }

    /// Take the finished report, joining the thread.
    pub fn try_finish(&mut self) -> Option<Result<RunReport, String>> {
        let finished = self.done.lock().ok().and_then(|slot| slot.clone());
        let stopped = finished.is_none() && self.handle.as_ref().is_some_and(|handle| handle.is_finished());
        if finished.is_none() && !stopped {
            return None;
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        Some(finished.unwrap_or_else(|| Err("the backend worker stopped without reporting a result".to_string())))
    }
}

/// Start a run on its own thread.
pub fn start(request: RunRequest) -> Result<RunHandle, String> {
    if request.args.is_empty() {
        return Err("the backend plan has no command line".to_string());
    }
    if !request.initial_mask.is_file() {
        return Err("the initial mask picture must exist before a run".to_string());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let done: Arc<Mutex<Option<Result<RunReport, String>>>> = Arc::new(Mutex::new(None));
    let progress = Arc::new(Mutex::new(String::new()));

    let thread_cancel = Arc::clone(&cancel);
    let thread_done = Arc::clone(&done);
    let thread_progress = Arc::clone(&progress);
    let handle = std::thread::Builder::new()
        .name("frameforge-roto-backend".to_string())
        .spawn(move || {
            let result = execute(&request, &thread_cancel, &thread_progress);
            if let Ok(mut slot) = thread_done.lock() {
                *slot = Some(result);
            }
        })
        .map_err(|error| format!("could not start the worker thread: {error}"))?;

    Ok(RunHandle { cancel, done, progress, handle: Some(handle) })
}

/// The thread's whole body: export, spawn, drive, publish.
fn execute(request: &RunRequest, cancel: &AtomicBool, progress: &Arc<Mutex<String>>) -> Result<RunReport, String> {
    let announce = |text: String| {
        if let Ok(mut slot) = progress.lock() {
            *slot = text;
        }
    };

    let Some((first, last)) = frame_range(request.rate, request.range) else {
        return Err("the selected clip has no frames to propagate".to_string());
    };
    let (width, height) = (request.working.0.max(16), request.working.1.max(16));
    let run_root = std::env::temp_dir().join(format!("frameforge-roto-run-{}", std::process::id()));
    let frames_dir = run_root.join("frames");
    let masks_dir = run_root.join("masks");

    announce("Exporting frames…".to_string());
    let sources = ProjectSources::build(&request.doc).map_err(|error| error.to_string())?;
    let source = sources.source(request.item).ok_or_else(|| "the clip's media has no readable source".to_string())?;
    let exported = export_frames(source.as_ref(), request.range, request.rate, &frames_dir, (width, height)).map_err(|error| error.to_string())?;
    if exported.first_frame != first || exported.last_frame != last {
        return Err(format!("frame numbering drifted: exported {}..{} but the run asked for {first}..{last}", exported.first_frame, exported.last_frame));
    }

    let spec = RotoJobSpec {
        frames_dir: frames_dir.display().to_string(),
        first_frame: first,
        last_frame: last,
        initial_mask: request.initial_mask.display().to_string(),
        output_dir: masks_dir.display().to_string(),
        width,
        height,
        model: request.weights_path.as_ref().map_or_else(|| request.model_id.clone(), |path| path.display().to_string()),
        model_sha256: request.weights_sha256.clone(),
        direction: request.direction,
    };

    announce(format!("Starting the {} worker…", request.model_id));
    let args: Vec<&str> = request.args.iter().map(String::as_str).collect();
    let mut worker = XmemWorker::spawn(&request.program, &args).map_err(|error| error.to_string())?;
    let info = worker.handshake_timeout(HANDSHAKE).map_err(|error| format!("worker handshake: {error}"))?;
    announce(format!("Worker {} ready (protocol {}); propagating…", info.backend, info.protocol));

    let outcome = run_job(&mut worker, &spec, &request.limits, cancel, |frame, total, fps, rss_mb| {
        let done = (frame - first + 1).max(0);
        let memory = match (rss_mb, request.limits.memory_ceiling_mb) {
            (Some(rss), Some(ceiling)) => format!(", RSS {rss:.0} of {ceiling:.0} MB"),
            _ => String::new(),
        };
        announce(format!("Propagating {done}/{total} frames (frame {frame}, {fps:.2} fps{memory})"));
    })
    .map_err(|error| error.to_string())?;
    let _ = worker.request_shutdown();

    let (frames, killed, note) = match outcome {
        RotoJobOutcome::Done(report) => (report.frames, false, format!("{:.1}s", report.elapsed)),
        RotoJobOutcome::Cancelled(report) if report.killed => {
            return Err(format!("Cancelled: the worker did not acknowledge, so it was killed after {} progress events", report.progress_events));
        }
        RotoJobOutcome::Cancelled(report) => {
            return Err(format!("Cancelled by the worker after {} progress events", report.progress_events));
        }
    };

    announce("Publishing the masks…".to_string());
    let count = last.saturating_sub(first).saturating_add(1);
    let identity = match request.doc.project.resolve_media(request.item) {
        Some((_, clip, _)) => source_identity(request.item, clip),
        None => format!("item:{}", request.item.0),
    };

    // 1) the multi-object cache of frameforge-roto, adopting the worker's own
    //    bytes rather than re-encoding them
    let key = MaskKey {
        model_id: request.model_id.clone(),
        weights_sha256: request.weights_sha256.clone(),
        source_identity: identity.clone(),
        width,
        height,
        frame_rate_num: request.rate.num as i32,
        frame_rate_den: request.rate.den as i32,
        object_ids: vec![request.object_id],
        correction_frames: Vec::new(),
        pipeline: 1,
    };
    let cache_note = match cache::publish_from_dir(&request.cache_root, &key, request.object_id, first, &masks_dir, count, MatteMode::Alpha) {
        Ok(_) => format!("multi-object cache {}", key.digest()),
        Err(error) => format!("multi-object cache not written ({error})"),
    };

    // 2) the R1 cache the renderer reads, from the worker's own PNGs
    let r1_key = RotoCacheKey {
        source_identity: identity,
        model_sha256: format!("model:{}", request.model_id),
        width,
        height,
        frame_rate: request.rate,
        correction_ids: Vec::new(),
        xmem_commit: request.model_id.clone(),
    };
    let adoption = MaskAdoption { output_dir: Path::new(&masks_dir), first_frame: first, frame_count: count, frame_rate: request.rate, width, height };
    let r1 = match MaskCache::adopt(&request.r1_root, &r1_key, &adoption) {
        Ok(frames) => Ok(format!("{frames} masks in the R1 cache {}", r1_key.cache_key())),
        Err(error) => Err(error.to_string()),
    };

    let summary = match r1 {
        Ok(text) => format!("{} frames in {note}; {text}; {cache_note}", frames),
        Err(error) => format!("{} frames in {note}, but the renderer's matte was not published: {error}", frames),
    };
    Ok(RunReport { summary, cache_key: Some(key.digest()), r1_key: Some(r1_key.cache_key()), item: Some(request.item), frames, killed })
}

/// Whether the panel can offer the attach action for a finished run: the run
/// published the renderer's matte, and it knows which clip it belongs to.
pub fn attach_available(report: &RunReport) -> bool {
    report.r1_key.is_some() && report.item.is_some()
}

/// Read one published frame back, for the panel's preview of a finished run.
pub fn read_frame(root: &Path, key: &MaskKey, object_id: u64, frame: i64) -> Option<Plane> {
    cache::load(root, key, object_id, frame).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_with(changes: impl FnOnce(&mut RunGate)) -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("frameforge-roto-gate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let interpreter = dir.join("python.exe");
        let worker = dir.join("cutie_worker.py");
        std::fs::write(&interpreter, b"").expect("interpreter placeholder");
        std::fs::write(&worker, b"").expect("worker placeholder");
        let mut gate = RunGate {
            model_id: "cutie-base".to_string(),
            licence_permits: true,
            runtime_installed: true,
            weights_present: true,
            interpreter: Some(interpreter),
            worker,
            consent: true,
        };
        changes(&mut gate);
        super::gate(&gate)
    }

    #[test]
    fn a_fully_prepared_backend_passes_the_gate() {
        assert_eq!(gate_with(|_| {}), Ok(()));
    }

    #[test]
    fn the_gate_names_the_first_thing_that_is_missing() {
        assert!(gate_with(|gate| gate.licence_permits = false).expect_err("licence").contains("not permitted in this build"));
        assert!(gate_with(|gate| gate.interpreter = None).expect_err("interpreter").contains("Python interpreter"));
        assert!(gate_with(|gate| gate.worker = PathBuf::from("C:\\nowhere\\worker.py")).expect_err("worker").contains("not shipped"));
        assert!(gate_with(|gate| gate.weights_present = false).expect_err("weights").contains("not installed"));
        assert!(gate_with(|gate| gate.runtime_installed = false).expect_err("runtime").contains("no runtime"));
        assert!(gate_with(|gate| gate.consent = false).expect_err("consent").contains("consent"));
    }

    #[test]
    fn a_restricted_licence_is_refused_before_anything_else() {
        // Even with everything installed, GPL/non-commercial backends are refused.
        let error = gate_with(|gate| {
            gate.model_id = "matanyone".to_string();
            gate.licence_permits = false;
            gate.consent = false;
        })
        .expect_err("restricted");
        assert!(error.contains("external tool you installed yourself"), "{error}");
    }

    #[test]
    fn a_run_without_a_command_line_or_a_mask_never_starts() {
        let request = RunRequest {
            program: PathBuf::from("python.exe"),
            args: Vec::new(),
            model_id: "cutie-base".to_string(),
            weights_path: None,
            weights_sha256: "0".repeat(64),
            object_id: 1,
            doc: FrameForgeProject::new(filmcraft_project::Project::new("t")),
            item: ItemId(1),
            range: TimeRange::new(filmcraft_time::Tick::ZERO, filmcraft_time::Tick(10)),
            rate: filmcraft_time::FrameRate::FPS_24,
            working: (64, 36),
            initial_mask: PathBuf::from("C:\\nowhere\\mask.png"),
            direction: WorkerDirection::Forward,
            limits: RotoJobLimits::default(),
            cache_root: PathBuf::from("C:\\nowhere\\cache"),
            r1_root: PathBuf::from("C:\\nowhere\\r1"),
        };
        assert!(start(request).is_err(), "no command line means no run");
    }

    #[test]
    fn a_report_without_an_r1_key_is_not_attachable() {
        let report = RunReport { r1_key: None, ..RunReport::default() };
        assert!(!attach_available(&report));
    }
}
