//! The roto job runner: the policy the R2 transport left to its caller.
//!
//! [`run_job`] takes a live [`XmemWorker`], validates the request *before*
//! sending anything, then drives `propagate` with three deadlines — a stall
//! watchdog, an overall budget and a cancel-acknowledgement wait — while
//! watching a cancellation flag. A worker that stalls is killed and reported as
//! a stall; a worker that will not acknowledge `cancel` is killed and the job
//! still reports as cancelled (with `killed: true`, so nothing is hidden).
//!
//! It does not start XMem, find a runtime or read weights: those are the
//! consent-based steps (R5) and the caller's job. Nothing here pretends to be
//! an inference result; it only reports what the worker protocol actually sent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::roto_worker::{WorkerCommand, WorkerDirection, WorkerError, WorkerEvent, XmemWorker};

/// How often the runner looks at the cancel flag while waiting for an event.
/// Small enough to feel immediate, large enough not to spin.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// One mask plane is capped exactly as the R1 cache caps it, so a bad request
/// cannot ask the worker for an allocation the renderer would then refuse.
const MAX_JOB_PIXELS: u64 = 100_000_000;

/// Everything the worker needs for one propagation pass. Mirrors
/// [`WorkerCommand::Propagate`] without the transport enum, so a caller can
/// validate and log the request on its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotoJobSpec {
    pub frames_dir: String,
    pub first_frame: i64,
    pub last_frame: i64,
    pub initial_mask: String,
    pub output_dir: String,
    pub width: u32,
    pub height: u32,
    pub model: String,
    pub model_sha256: String,
    pub direction: WorkerDirection,
}

impl RotoJobSpec {
    /// The inclusive frame count, or `None` when the range is inverted.
    pub fn frame_count(&self) -> Option<i64> {
        self.last_frame.checked_sub(self.first_frame).and_then(|span| span.checked_add(1)).filter(|count| *count > 0)
    }
}

/// The deadlines one job runs under. Defaults are the R3 target-laptop numbers
/// (a slow CPU forward pass on 480x270 took ~4.4 s of inference) with room to
/// spare; a caller may tighten them for a smaller clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotoJobLimits {
    /// Longest silence between events before the job is a stall.
    pub stall: Duration,
    /// Longest total wall time for the whole job.
    pub overall: Duration,
    /// How long a worker has to acknowledge `cancel` before it is killed.
    pub cancel_ack: Duration,
    /// Largest frame count the runner will ask a worker to propagate.
    pub max_frames: i64,
}

impl Default for RotoJobLimits {
    fn default() -> Self {
        Self { stall: Duration::from_secs(120), overall: Duration::from_secs(3_600), cancel_ack: Duration::from_secs(5), max_frames: 20_000 }
    }
}

/// What a finished job actually did. `killed` is true only when the runner had
/// to terminate the process itself (a stall, a deadline, or an unacknowledged
/// cancel), so a killed job is never reported as a clean pass.
#[derive(Clone, Debug, PartialEq)]
pub struct RotoJobReport {
    pub frames: i64,
    pub elapsed: f64,
    pub progress_events: usize,
    pub killed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RotoJobOutcome {
    Done(RotoJobReport),
    Cancelled(RotoJobReport),
}

#[derive(Debug, Error)]
pub enum RotoJobError {
    #[error("roto job refused before start: {0}")]
    Refused(String),
    #[error("roto job worker: {0}")]
    Worker(#[from] WorkerError),
    #[error("roto job stalled: {0}")]
    Stalled(String),
    #[error("roto job exceeded its overall deadline: {0}")]
    Deadline(String),
}

/// Check the request and the limits before a byte is sent. Every refusal names
/// the field, so a caller can surface it verbatim.
fn validate(spec: &RotoJobSpec, limits: &RotoJobLimits) -> Result<i64, RotoJobError> {
    let count = spec.frame_count().ok_or_else(|| RotoJobError::Refused(format!("inverted frame range {}..{}", spec.first_frame, spec.last_frame)))?;
    if spec.first_frame < 0 {
        return Err(RotoJobError::Refused(format!("negative first frame {}", spec.first_frame)));
    }
    if count > limits.max_frames {
        return Err(RotoJobError::Refused(format!("{count} frames exceeds the {}-frame job limit", limits.max_frames)));
    }
    if spec.width == 0 || spec.height == 0 {
        return Err(RotoJobError::Refused(format!("invalid working size {}x{}", spec.width, spec.height)));
    }
    let pixels = u64::from(spec.width) * u64::from(spec.height);
    if pixels > MAX_JOB_PIXELS {
        return Err(RotoJobError::Refused(format!("{}x{} exceeds the {MAX_JOB_PIXELS}-pixel working size", spec.width, spec.height)));
    }
    if spec.frames_dir.is_empty() || spec.initial_mask.is_empty() || spec.output_dir.is_empty() {
        return Err(RotoJobError::Refused("frames directory, initial mask and output directory must not be empty".to_string()));
    }
    if spec.model.is_empty() {
        return Err(RotoJobError::Refused("no model file was named".to_string()));
    }
    if spec.model_sha256.len() != 64 || !spec.model_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(RotoJobError::Refused("model SHA-256 must be 64 hex digits".to_string()));
    }
    if limits.stall.is_zero() || limits.overall.is_zero() {
        return Err(RotoJobError::Refused("stall and overall deadlines must be greater than zero".to_string()));
    }
    Ok(count)
}

/// Drive one `propagate` pass to completion, a stall, a deadline or a cancel.
///
/// `on_progress` is called for every `progress` event with `(frame, total, fps)`
/// exactly as the worker reported them; the runner never invents a rate.
pub fn run_job(
    worker: &mut XmemWorker,
    spec: &RotoJobSpec,
    limits: &RotoJobLimits,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(i64, i64, f64),
) -> Result<RotoJobOutcome, RotoJobError> {
    let count = validate(spec, limits)?;
    let start = Instant::now();
    if cancel.load(Ordering::Relaxed) {
        return Ok(RotoJobOutcome::Cancelled(RotoJobReport { frames: 0, elapsed: start.elapsed().as_secs_f64(), progress_events: 0, killed: false }));
    }

    worker.send(&WorkerCommand::Propagate {
        frames_dir: spec.frames_dir.clone(),
        first_frame: spec.first_frame,
        last_frame: spec.last_frame,
        initial_mask: spec.initial_mask.clone(),
        output_dir: spec.output_dir.clone(),
        width: spec.width,
        height: spec.height,
        model: spec.model.clone(),
        model_sha256: spec.model_sha256.clone(),
        direction: spec.direction,
    })?;

    // A worker that emits far more events than frames is not making progress;
    // the cap stops a hostile or looping worker from running forever.
    let event_cap = usize::try_from(count.saturating_mul(4).saturating_add(64)).unwrap_or(usize::MAX);
    let mut last_event = Instant::now();
    let mut events = 0usize;
    let mut progress_events = 0usize;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return cancel_job(worker, limits, progress_events, start);
        }
        let now = Instant::now();
        let elapsed = now.duration_since(start);
        if elapsed > limits.overall {
            kill_quietly(worker);
            return Err(RotoJobError::Deadline(format!("overall deadline of {:.1}s passed", limits.overall.as_secs_f64())));
        }
        if now.duration_since(last_event) > limits.stall {
            kill_quietly(worker);
            return Err(RotoJobError::Stalled(format!("no event for {:.1}s", limits.stall.as_secs_f64())));
        }
        // Sleep in short slices so cancel stays responsive, but never past the
        // overall deadline.
        let remaining = limits.overall.saturating_sub(elapsed);
        let slice = CANCEL_POLL.min(remaining.max(Duration::from_millis(1)));

        match worker.recv_timeout(slice) {
            Ok(event) => {
                last_event = Instant::now();
                events += 1;
                if events > event_cap {
                    kill_quietly(worker);
                    return Err(RotoJobError::Worker(WorkerError::Protocol(format!("more than {event_cap} events for {count} frames"))));
                }
                match event {
                    WorkerEvent::Progress { frame, total, fps, .. } => {
                        if total != count {
                            kill_quietly(worker);
                            return Err(RotoJobError::Worker(WorkerError::Protocol(format!(
                                "worker total {total} does not match the requested {count} frames"
                            ))));
                        }
                        if frame < spec.first_frame || frame > spec.last_frame {
                            kill_quietly(worker);
                            return Err(RotoJobError::Worker(WorkerError::Protocol(format!(
                                "progress frame {frame} is outside {}..{}",
                                spec.first_frame, spec.last_frame
                            ))));
                        }
                        progress_events += 1;
                        on_progress(frame, total, fps);
                    }
                    WorkerEvent::Done { frames, elapsed } => {
                        return Ok(RotoJobOutcome::Done(RotoJobReport { frames, elapsed, progress_events, killed: false }));
                    }
                    WorkerEvent::Cancelled => {
                        return Ok(RotoJobOutcome::Cancelled(RotoJobReport {
                            frames: 0,
                            elapsed: start.elapsed().as_secs_f64(),
                            progress_events,
                            killed: false,
                        }));
                    }
                    WorkerEvent::Error { message } => {
                        kill_quietly(worker);
                        return Err(RotoJobError::Worker(WorkerError::Protocol(format!("worker error: {message}"))));
                    }
                    other => {
                        kill_quietly(worker);
                        return Err(RotoJobError::Worker(WorkerError::Protocol(format!("unexpected worker event: {other:?}"))));
                    }
                }
            }
            // A quiet slice is not a stall: the deadlines above decide that.
            Err(WorkerError::Timeout(_)) => {}
            Err(error) => return Err(RotoJobError::Worker(error)),
        }
    }
}

/// Ask the worker to stop, give it `cancel_ack` to say so, then kill it.
fn cancel_job(worker: &mut XmemWorker, limits: &RotoJobLimits, progress_events: usize, start: Instant) -> Result<RotoJobOutcome, RotoJobError> {
    if let Err(error) = worker.send(&WorkerCommand::Cancel) {
        // A worker whose stdin is already gone cannot be asked politely.
        kill_quietly(worker);
        return Err(RotoJobError::Worker(error));
    }
    let deadline = Instant::now() + limits.cancel_ack;
    loop {
        let now = Instant::now();
        if now >= deadline {
            kill_quietly(worker);
            return Ok(RotoJobOutcome::Cancelled(RotoJobReport { frames: 0, elapsed: start.elapsed().as_secs_f64(), progress_events, killed: true }));
        }
        match worker.recv_timeout(CANCEL_POLL.min(deadline.saturating_duration_since(now))) {
            Ok(WorkerEvent::Cancelled) => {
                return Ok(RotoJobOutcome::Cancelled(RotoJobReport { frames: 0, elapsed: start.elapsed().as_secs_f64(), progress_events, killed: false }));
            }
            // The worker finished the pass before it saw the cancel; report the
            // frames it really produced and say it was not killed.
            Ok(WorkerEvent::Done { frames, .. }) => {
                return Ok(RotoJobOutcome::Cancelled(RotoJobReport { frames, elapsed: start.elapsed().as_secs_f64(), progress_events, killed: false }));
            }
            Ok(WorkerEvent::Error { message }) => {
                kill_quietly(worker);
                return Err(RotoJobError::Worker(WorkerError::Protocol(format!("worker error during cancel: {message}"))));
            }
            Ok(_) => {}
            Err(WorkerError::Timeout(_)) => {}
            Err(error) => return Err(RotoJobError::Worker(error)),
        }
    }
}

/// Kill and keep the reason: the caller reports the stall, not the kill result.
fn kill_quietly(worker: &mut XmemWorker) {
    let _ = worker.kill();
}
