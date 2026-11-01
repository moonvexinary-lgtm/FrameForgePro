//! The rotoscoping job queue: one place where selection, tracking, QC, matting
//! and export jobs wait, run, pause and get cancelled.
//!
//! The queue is a *state machine*, not a thread pool: it decides what may run
//! next, and the app owns the actual work (and the inference). It starts nothing
//! by itself — an empty queue never runs anything, and a job only becomes
//! `Running` when the queue is asked for the next job by the caller.
//!
//! While a job runs, the queue is where pause, resume and cancel live, so the
//! workspace has one consistent story for every stage.

use serde::{Deserialize, Serialize};

use crate::engine::FrameRange;

/// What a job does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobKind {
    /// Ask a selection engine for a mask on one frame.
    Selection,
    /// Propagate an object through a range.
    Tracking,
    /// Score the propagated masks.
    TemporalQc,
    /// Refine a mask into a matte.
    Matting,
    /// Write masks, alpha or RGBA to disk.
    Export,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            JobKind::Selection => "selection",
            JobKind::Tracking => "tracking",
            JobKind::TemporalQc => "quality check",
            JobKind::Matting => "matte refinement",
            JobKind::Export => "export",
        }
    }

    /// Whether this kind starts a model process (and therefore needs consent,
    /// weights and a runtime).
    pub fn needs_runtime(self) -> bool {
        matches!(self, JobKind::Selection | JobKind::Tracking)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Paused { done_frames: i64 },
    Done,
    Failed { reason: String },
    Cancelled { done_frames: i64 },
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Failed { .. } | JobStatus::Cancelled { .. })
    }

    pub fn is_active(&self) -> bool {
        matches!(self, JobStatus::Queued | JobStatus::Running | JobStatus::Paused { .. })
    }

    pub fn label(&self) -> String {
        match self {
            JobStatus::Queued => "queued".to_string(),
            JobStatus::Running => "running".to_string(),
            JobStatus::Paused { done_frames } => format!("paused after {done_frames} frames"),
            JobStatus::Done => "done".to_string(),
            JobStatus::Failed { reason } => format!("failed: {reason}"),
            JobStatus::Cancelled { done_frames } => format!("cancelled after {done_frames} frames"),
        }
    }
}

/// One queued job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    /// The object this job serves, when it serves one.
    pub object_id: Option<u64>,
    /// The model the job uses (empty for kinds that need none).
    pub model_id: String,
    pub range: FrameRange,
    pub status: JobStatus,
    /// Larger runs first.
    pub priority: u8,
    /// Frames completed so far, for the progress bar.
    pub done_frames: i64,
}

impl Job {
    pub fn fraction(&self) -> f32 {
        let total = self.range.count().max(1);
        (self.done_frames as f32 / total as f32).clamp(0.0, 1.0)
    }
}

/// The queue.
#[derive(Debug)]
pub struct Queue {
    jobs: Vec<Job>,
    max_running: usize,
    next_id: u64,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Queue {
    /// `max_running` is the number of jobs the app is willing to have in flight;
    /// one is the default because inference is heavy and serial by nature.
    pub fn new(max_running: usize) -> Self {
        Self { jobs: Vec::new(), max_running: max_running.max(1), next_id: 1 }
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    pub fn running(&self) -> usize {
        self.jobs.iter().filter(|job| job.status == JobStatus::Running).count()
    }

    pub fn active(&self) -> usize {
        self.jobs.iter().filter(|job| job.status.is_active()).count()
    }

    /// Add a job. Nothing starts here — the caller decides when to run.
    pub fn enqueue(&mut self, kind: JobKind, model_id: impl Into<String>, object_id: Option<u64>, range: FrameRange, priority: u8) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(Job { id, kind, object_id, model_id: model_id.into(), range, status: JobStatus::Queued, priority, done_frames: 0 });
        id
    }

    /// The next job that may start: highest priority first, then oldest. Returns
    /// `None` when the queue is empty or `max_running` jobs are already running.
    pub fn start_next(&mut self) -> Option<u64> {
        if self.running() >= self.max_running {
            return None;
        }
        let candidate =
            self.jobs.iter().filter(|job| job.status == JobStatus::Queued).max_by_key(|job| (job.priority, std::cmp::Reverse(job.id))).map(|job| job.id)?;
        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == candidate) {
            job.status = JobStatus::Running;
        }
        Some(candidate)
    }

    pub fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.iter().find(|job| job.id == id)
    }

    fn job_mut(&mut self, id: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    /// Report progress and, when the range is complete, finish the job.
    pub fn report(&mut self, id: u64, done_frames: i64) {
        let Some(job) = self.job_mut(id) else { return };
        job.done_frames = done_frames.clamp(0, job.range.count().max(0));
        if job.done_frames >= job.range.count() && job.range.count() > 0 {
            job.status = JobStatus::Done;
        }
    }

    /// Pause a running job; a queued or finished job is left alone.
    pub fn pause(&mut self, id: u64) {
        if let Some(job) = self.job_mut(id)
            && job.status == JobStatus::Running
        {
            job.status = JobStatus::Paused { done_frames: job.done_frames };
        }
    }

    /// Resume a paused job.
    pub fn resume(&mut self, id: u64) {
        if let Some(job) = self.job_mut(id)
            && matches!(job.status, JobStatus::Paused { .. })
        {
            job.status = JobStatus::Queued;
        }
    }

    /// Cancel any active job. A cancelled job frees its slot immediately.
    pub fn cancel(&mut self, id: u64) {
        let Some(job) = self.job_mut(id) else { return };
        if job.status.is_active() {
            job.status = JobStatus::Cancelled { done_frames: job.done_frames };
        }
    }

    pub fn fail(&mut self, id: u64, reason: impl Into<String>) {
        let Some(job) = self.job_mut(id) else { return };
        if !job.status.is_terminal() {
            job.status = JobStatus::Failed { reason: reason.into() };
        }
    }

    /// Cancel everything, for the "stop all processing" button.
    pub fn cancel_all(&mut self) -> usize {
        let ids: Vec<u64> = self.jobs.iter().filter(|job| job.status.is_active()).map(|job| job.id).collect();
        let count = ids.len();
        for id in ids {
            self.cancel(id);
        }
        count
    }

    /// Remove finished jobs, keeping the list short.
    pub fn clear_finished(&mut self) -> usize {
        let before = self.jobs.len();
        self.jobs.retain(|job| !job.status.is_terminal());
        before - self.jobs.len()
    }

    /// A one-line summary for the status bar.
    pub fn summary(&self) -> String {
        if self.jobs.is_empty() {
            return "no rotoscoping jobs".to_string();
        }
        let done = self.jobs.iter().filter(|job| job.status == JobStatus::Done).count();
        let failed = self.jobs.iter().filter(|job| matches!(job.status, JobStatus::Failed { .. })).count();
        format!("{} job(s): {} active, {} running, {done} done, {failed} failed", self.jobs.len(), self.active(), self.running())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range() -> FrameRange {
        FrameRange::new(0, 9)
    }

    #[test]
    fn an_empty_queue_never_starts_anything() {
        let mut queue = Queue::default();
        assert!(queue.is_empty());
        assert_eq!(queue.start_next(), None, "nothing was enqueued, so nothing may run");
        assert_eq!(queue.summary(), "no rotoscoping jobs");
    }

    #[test]
    fn jobs_run_by_priority_then_by_age_and_only_up_to_the_limit() {
        let mut queue = Queue::new(1);
        let low = queue.enqueue(JobKind::Export, "", None, range(), 1);
        let high = queue.enqueue(JobKind::Tracking, "cutie-base", Some(1), range(), 9);

        let first = queue.start_next().expect("a job starts");
        assert_eq!(first, high, "the higher priority job goes first");
        assert_eq!(queue.running(), 1);
        assert_eq!(queue.start_next(), None, "the runner is busy");
        assert_eq!(queue.job(low).expect("job").status, JobStatus::Queued);

        queue.report(high, 10);
        assert_eq!(queue.job(high).expect("job").status, JobStatus::Done);
        assert_eq!(queue.start_next(), Some(low), "the slot is free again");
    }

    #[test]
    fn progress_tracks_the_range_and_finishes_the_job() {
        let mut queue = Queue::default();
        let id = queue.enqueue(JobKind::Tracking, "cutie-base", Some(2), range(), 0);
        queue.start_next();
        queue.report(id, 5);
        let job = queue.job(id).expect("job");
        assert_eq!(job.done_frames, 5);
        assert!((job.fraction() - 0.5).abs() < 0.01);
        assert_eq!(job.status, JobStatus::Running);
        queue.report(id, 99);
        assert_eq!(queue.job(id).expect("job").done_frames, 10, "progress is clamped to the range");
        assert_eq!(queue.job(id).expect("job").status, JobStatus::Done);
    }

    #[test]
    fn pause_resume_and_cancel_behave_like_a_workspace_expects() {
        let mut queue = Queue::default();
        let id = queue.enqueue(JobKind::Tracking, "cutie-base", Some(1), range(), 0);
        queue.start_next();
        queue.report(id, 4);

        queue.pause(id);
        assert_eq!(queue.job(id).expect("job").status, JobStatus::Paused { done_frames: 4 });
        assert!(queue.job(id).expect("job").status.label().contains("paused after 4"));

        queue.resume(id);
        assert_eq!(queue.job(id).expect("job").status, JobStatus::Queued);
        assert_eq!(queue.start_next(), Some(id), "a resumed job can start again");

        queue.cancel(id);
        assert_eq!(queue.job(id).expect("job").status, JobStatus::Cancelled { done_frames: 4 });
        assert_eq!(queue.running(), 0, "a cancelled job frees the slot");
        assert!(queue.job(id).expect("job").status.is_terminal());
    }

    #[test]
    fn cancelling_everything_and_clearing_finished_jobs_keep_the_list_honest() {
        let mut queue = Queue::new(2);
        let a = queue.enqueue(JobKind::Tracking, "cutie-base", Some(1), range(), 0);
        let b = queue.enqueue(JobKind::Selection, "sam2.1-tiny", Some(1), FrameRange::new(0, 0), 0);
        let c = queue.enqueue(JobKind::Matting, "", Some(1), range(), 0);
        assert_eq!(queue.start_next(), Some(a));
        assert_eq!(queue.start_next(), Some(b), "two may run at once");
        assert_eq!(queue.start_next(), None, "but not three");

        queue.fail(a, "worker exited");
        assert!(queue.job(a).expect("job").status.label().contains("worker exited"));
        queue.report(b, 1);
        assert_eq!(queue.cancel_all(), 1, "only the still-active job is cancelled");
        assert!(!queue.job(c).expect("job").status.is_active());

        let removed = queue.clear_finished();
        assert_eq!(removed, 3, "every job is terminal now");
        assert!(queue.is_empty());
        assert!(!JobKind::Tracking.label().is_empty() && JobKind::Tracking.needs_runtime());
        assert!(!JobKind::Matting.needs_runtime(), "matting is built in");
    }
}
