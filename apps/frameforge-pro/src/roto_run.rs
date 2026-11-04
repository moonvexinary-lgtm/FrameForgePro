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
use frameforge_roto::temporal::{MaskSummary, QcLimits, QualityReport, analyze};

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
    /// The backend's SPDX licence identifier, carried into the run's provenance
    /// line so the report says what the user consented to run.
    pub licence: String,
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
    /// The temporal QC verdict over the published masks, when they could be read
    /// back. This is arithmetic on the masks — no model is involved.
    pub qc: Option<QualityReport>,
    /// Exactly what produced the run: model, licence and the weights digest the
    /// worker was told to expect.
    pub provenance: String,
}

impl RunReport {
    /// The frames the review list should offer, worst first in the list order the
    /// QC pass chose (ascending by flagged range).
    pub fn review_frames(&self) -> Vec<i64> {
        self.qc.as_ref().map(QualityReport::review_frames).unwrap_or_default()
    }
}

/// Score a published run with the temporal QC pass, reading the masks back from
/// the cache. Deterministic arithmetic: no model, no inference, no GPU.
pub fn review(root: &Path, key: &MaskKey, object_id: u64, first_frame: i64, count: i64) -> Result<QualityReport, String> {
    if count <= 0 {
        return Err("there is nothing to review".to_string());
    }
    let mut summaries = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    for index in 0..count {
        let frame = first_frame + index;
        let plane = cache::load(root, key, object_id, frame)
            .map_err(|error| format!("frame {frame}: {error}"))?
            .ok_or_else(|| format!("frame {frame} is not in the cache"))?;
        let summary = MaskSummary::from_alpha(frame, plane.data(), key.width, key.height, 128)
            .ok_or_else(|| format!("frame {frame} could not be summarised at {}x{}", key.width, key.height))?;
        summaries.push(summary);
    }
    Ok(analyze(&summaries, &QcLimits::default()))
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

    // 3) the temporal QC verdict, read back from the cache we just published. It
    //    is arithmetic on masks, not inference, so it can run here, once.
    announce("Checking the masks over time…".to_string());
    let qc = review(&request.cache_root, &key, request.object_id, first, count).ok();
    let provenance = provenance(&request.model_id, &request.licence, &request.weights_sha256);

    Ok(RunReport { summary, cache_key: Some(key.digest()), r1_key: Some(r1_key.cache_key()), item: Some(request.item), frames, killed, qc, provenance })
}

/// One line naming exactly what a run used: the backend, its licence and the
/// weights digest the worker was told to expect. An unpinned digest is said out
/// loud — "no digest" is a weaker guarantee than a hash, and the report must not
/// make the two look alike.
pub fn provenance(model_id: &str, licence: &str, weights_sha256: &str) -> String {
    let licence = if licence.trim().is_empty() { "licence unrecorded" } else { licence.trim() };
    let pinned = weights_sha256.len() == 64 && !weights_sha256.trim_matches('0').is_empty() && weights_sha256.chars().all(|c| c.is_ascii_hexdigit());
    let digest = if pinned { format!("weights sha256 {}", &weights_sha256[..12]) } else { "weights digest unpinned".to_string() };
    format!("{model_id} · {licence} · {digest}")
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
    use frameforge_roto::temporal::SignalKind;

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
            licence: "Apache-2.0".to_string(),
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

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("frameforge-roto-review-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn review_key(w: u32, h: u32) -> MaskKey {
        MaskKey {
            model_id: "cutie-base".to_string(),
            weights_sha256: "ab".repeat(32),
            source_identity: "item:7:file:clip.mp4:0f1e2d3c4b5a6978:1048576".to_string(),
            width: w,
            height: h,
            frame_rate_num: 24,
            frame_rate_den: 1,
            object_ids: vec![1],
            correction_frames: Vec::new(),
            pipeline: 1,
        }
    }

    /// A square of side `side` at `(x, y)`, painted into an empty plane.
    fn square(w: u32, h: u32, x: u32, y: u32, side: u32) -> Plane {
        let mut plane = Plane::new(w, h, 0).expect("plane");
        for row in y..(y + side).min(h) {
            for column in x..(x + side).min(w) {
                plane.data_mut()[(row * w + column) as usize] = 255;
            }
        }
        plane
    }

    #[test]
    fn a_clean_run_reviews_clean() {
        let root = temp_root("clean");
        let key = review_key(32, 32);
        let frames: Vec<Plane> = (0..6).map(|index| square(32, 32, 4 + index, 8, 10)).collect();
        cache::publish(&root, &key, 1, 0, &frames, MatteMode::Alpha).expect("publish");

        let qc = review(&root, &key, 1, 0, 6).expect("review");
        assert_eq!(qc.frames, 6);
        assert!(qc.is_clean(), "a steadily moving square is clean: {:?}", qc.signals);
        assert!(qc.review_frames().is_empty());
    }

    #[test]
    fn a_vanished_mask_is_flagged_and_offered_for_review() {
        let root = temp_root("vanish");
        let key = review_key(32, 32);
        let mut frames: Vec<Plane> = (0..6).map(|index| square(32, 32, 4 + index, 8, 10)).collect();
        frames[3] = Plane::new(32, 32, 0).expect("empty"); // the tracker lost the object
        cache::publish(&root, &key, 1, 0, &frames, MatteMode::Alpha).expect("publish");

        let qc = review(&root, &key, 1, 0, 6).expect("review");
        assert!(!qc.is_clean());
        assert!(qc.signals.iter().any(|signal| signal.kind == SignalKind::Disappeared && signal.frame == 3), "{:?}", qc.signals);
        assert!(qc.review_frames().contains(&3), "the review list offers the lost frame");
    }

    #[test]
    fn a_review_of_nothing_is_an_error_rather_than_a_blank_pass() {
        let root = temp_root("empty");
        let key = review_key(32, 32);
        assert!(review(&root, &key, 1, 0, 0).is_err());
    }

    #[test]
    fn provenance_names_the_model_licence_and_pinned_digest() {
        let line = provenance("cutie-base", "Apache-2.0", &"ab".repeat(32));
        assert_eq!(line, "cutie-base · Apache-2.0 · weights sha256 abababababab");
    }

    #[test]
    fn an_unpinned_digest_is_stated_rather_than_looking_like_a_hash() {
        assert!(provenance("cutie-base", "Apache-2.0", &"0".repeat(64)).contains("unpinned"));
        assert!(provenance("cutie-base", "Apache-2.0", "").contains("unpinned"));
        assert!(provenance("cutie-base", "Apache-2.0", "not-a-digest").contains("unpinned"));
        assert!(provenance("cutie-base", "", &"ab".repeat(32)).contains("licence unrecorded"));
    }
}
