//! Tracking: the backends the workspace can propagate with, the per-object
//! track state, and the segmentation of a run into resumable pieces.
//!
//! Cutie is the preferred backend, SAMURAI an optional motion-aware one, and
//! XMem stays available as the legacy fallback — the existing protocol adapter
//! in `frameforge-bridge` keeps working, so no project is stranded. A built-in
//! "no-op" backend exists for tests and for projects that only want manual
//! corrections; it is never presented as an AI engine.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::engine::{Correction, EngineCapabilities, EngineError, ExternalEngine, FrameRange, LaunchPlan, RotoEngine, RuntimeProbe, TrackRequest};
use crate::models::{ModelDescriptor, Task, find};

/// The direction of a propagation pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Forward,
    Backward,
    Both,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Forward => "forward",
            Direction::Backward => "backward",
            Direction::Both => "both ways",
        }
    }
}

/// Where an object's tracking stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackStatus {
    Untracked,
    Queued,
    Tracking,
    /// The temporal pass found something the user should look at.
    NeedsReview,
    Done,
    Failed,
}

impl TrackStatus {
    pub fn label(self) -> &'static str {
        match self {
            TrackStatus::Untracked => "not tracked",
            TrackStatus::Queued => "queued",
            TrackStatus::Tracking => "tracking",
            TrackStatus::NeedsReview => "needs review",
            TrackStatus::Done => "tracked",
            TrackStatus::Failed => "failed",
        }
    }
}

/// One piece of a run. A run is split at every correction so corrected frames are
/// never overwritten by a fresh prediction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub range: FrameRange,
    pub direction: Direction,
    /// True when this piece continues from a correction the user accepted.
    pub from_correction: Option<i64>,
}

impl Segment {
    pub fn frames(&self) -> i64 {
        self.range.count()
    }
}

/// Split a request into resumable segments.
pub fn plan_segments(range: FrameRange, direction: Direction, corrections: &[Correction]) -> Vec<Segment> {
    if range.count() == 0 {
        return Vec::new();
    }
    let mut marks: Vec<i64> = corrections.iter().map(|correction| correction.from).filter(|frame| range.contains(*frame)).collect();
    marks.sort_unstable();
    marks.dedup();

    let directions: Vec<Direction> = match direction {
        Direction::Both => vec![Direction::Forward, Direction::Backward],
        single => vec![single],
    };
    let mut segments: Vec<Segment> = Vec::new();

    for dir in directions {
        // Without corrections a single pass covers the whole range.
        if marks.is_empty() {
            segments.push(Segment { range, direction: dir, from_correction: None });
            continue;
        }
        // With corrections, every piece starts at a correction and runs to the
        // next boundary in the direction of travel: forward to the next
        // correction (or the end of the range), backward from the correction down
        // to the previous one (or the start). Corrected frames are therefore never
        // overwritten, and each piece is independently resumable.
        for (index, mark) in marks.iter().enumerate() {
            let piece = match dir {
                Direction::Forward => {
                    let next = marks.get(index + 1).copied().unwrap_or(range.last);
                    FrameRange::new(*mark, next.max(*mark))
                }
                Direction::Backward => {
                    let previous = index.checked_sub(1).and_then(|earlier| marks.get(earlier)).copied().unwrap_or(range.first);
                    FrameRange::new(previous.min(*mark), *mark)
                }
                // `Both` was expanded into the two directions above.
                Direction::Both => continue,
            };
            if piece.count() > 0 {
                segments.push(Segment { range: piece, direction: dir, from_correction: Some(*mark) });
            }
        }
    }
    segments
}

/// One object's track in the workspace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectTrack {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub status: TrackStatus,
    pub segments: Vec<Segment>,
    /// Frames the user corrected, ascending.
    pub corrections: Vec<i64>,
}

impl ObjectTrack {
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), visible: true, status: TrackStatus::Untracked, segments: Vec::new(), corrections: Vec::new() }
    }

    pub fn rename(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Record a manual correction. Corrections always invalidate whatever has
    /// been propagated from that frame on, which is why the status drops back to
    /// review rather than staying "tracked".
    pub fn correct(&mut self, frame: i64, range: FrameRange) {
        if !self.corrections.contains(&frame) {
            self.corrections.push(frame);
            self.corrections.sort_unstable();
        }
        self.status = TrackStatus::NeedsReview;
        self.plan(range, Direction::Both);
    }

    /// Recompute the propagation plan from the current corrections.
    pub fn plan(&mut self, range: FrameRange, direction: Direction) {
        let corrections: Vec<Correction> = self.corrections.iter().map(|frame| Correction::at(*frame)).collect();
        self.segments = plan_segments(range, direction, &corrections);
    }

    pub fn total_frames(&self) -> i64 {
        self.segments.iter().map(Segment::frames).sum()
    }
}

/// The registry of tracking backends: model id in, engine adapter out.
pub struct BackendRegistry {
    probe: Arc<dyn RuntimeProbe>,
    program: PathBuf,
    worker: PathBuf,
}

impl BackendRegistry {
    /// `program` runs `worker`: the same JSON-lines contract the bridge already
    /// speaks, so a new backend is a new worker script, not a new protocol.
    pub fn new(probe: Arc<dyn RuntimeProbe>, program: impl Into<PathBuf>, worker: impl Into<PathBuf>) -> Self {
        Self { probe, program: program.into(), worker: worker.into() }
    }

    pub fn tracking_models(&self) -> Vec<&'static ModelDescriptor> {
        crate::models::by_task(Task::Tracking)
    }

    /// The preferred backend when the workspace has no preference.
    pub fn default_model(&self) -> &'static ModelDescriptor {
        crate::models::default_tracking()
    }

    /// Build the adapter for a model id. `None` when the id is unknown, an error
    /// when the model cannot track at all.
    pub fn engine(&self, model_id: &str) -> Result<Box<dyn RotoEngine>, EngineError> {
        let model = find(model_id).ok_or_else(|| EngineError::UnknownModel(model_id.to_string()))?;
        if model.task != Task::Tracking {
            return Err(EngineError::Unsupported { model: model.id, what: "tracking" });
        }
        Ok(Box::new(ExternalEngine::new(model, Arc::clone(&self.probe), self.program.clone(), self.worker.clone())))
    }

    pub fn capabilities(&self, model_id: &str) -> Result<EngineCapabilities, EngineError> {
        Ok(self.engine(model_id)?.capabilities())
    }

    /// Describe a run for `request` on `model_id` without starting anything.
    pub fn plan(&self, model_id: &str, request: &TrackRequest, user_started: bool) -> Result<LaunchPlan, EngineError> {
        self.engine(model_id)?.plan(request, user_started)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FileProbe, NoRuntime, ObjectSpec};
    use crate::models::{DEFAULT_TRACKING_MODEL, find};
    use crate::selection::Prompt;

    #[test]
    fn a_run_without_corrections_is_one_segment_per_direction() {
        let range = FrameRange::new(0, 23);
        let forward = plan_segments(range, Direction::Forward, &[]);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].range, range);
        assert_eq!(forward[0].from_correction, None);
        assert_eq!(forward[0].frames(), 24);

        let both = plan_segments(range, Direction::Both, &[]);
        assert_eq!(both.len(), 2, "both directions plan two passes: {both:?}");
        assert_eq!(both[0].direction, Direction::Forward);
        assert_eq!(both[1].direction, Direction::Backward);
    }

    #[test]
    fn corrections_split_the_run_so_corrected_frames_are_never_overwritten() {
        let range = FrameRange::new(0, 23);
        let corrections = vec![Correction::at(10), Correction::at(20)];
        let segments = plan_segments(range, Direction::Forward, &corrections);
        assert_eq!(segments.len(), 2, "{segments:?}");
        assert_eq!(segments[0].range, FrameRange::new(10, 20));
        assert_eq!(segments[0].from_correction, Some(10));
        assert_eq!(segments[1].range, FrameRange::new(20, 23));
        assert!(segments.iter().all(|segment| segment.range.first >= 10), "frames before the first correction are untouched");

        let backward = plan_segments(range, Direction::Backward, &corrections);
        assert_eq!(backward.len(), 2, "{backward:?}");
        assert!(backward.iter().all(|segment| segment.from_correction.is_some()));
        assert_eq!(backward[0].range, FrameRange::new(0, 10), "the first backward piece runs down to the start of the range");
        assert_eq!(backward[1].range, FrameRange::new(10, 20), "and the next one stops at the previous correction");
    }

    #[test]
    fn an_object_track_keeps_identity_visibility_and_corrections() {
        let mut track = ObjectTrack::new(7, "Actor");
        track.plan(FrameRange::new(0, 11), Direction::Forward);
        assert_eq!(track.total_frames(), 12);
        assert_eq!(track.status, TrackStatus::Untracked);

        track.correct(6, FrameRange::new(0, 11));
        assert_eq!(track.corrections, vec![6]);
        assert_eq!(track.segments.len(), 2, "a correction re-plans both ways from it: {:?}", track.segments);
        assert!(
            track.segments.iter().any(|segment| segment.direction == Direction::Forward && segment.range.first == 6),
            "the forward piece starts at the correction: {:?}",
            track.segments
        );
        assert!(track.segments.iter().any(|segment| segment.direction == Direction::Backward && segment.range.last == 6));
        assert_eq!(track.status, TrackStatus::NeedsReview);

        track.set_visible(false);
        track.rename("Actor (left)");
        assert!(!track.visible);
        assert_eq!(track.name, "Actor (left)");
    }

    #[test]
    fn the_registry_offers_cutie_by_default_and_xmem_as_an_option() {
        let registry = BackendRegistry::new(Arc::new(NoRuntime), "python.exe", "tools/roto_worker.py");
        assert_eq!(registry.default_model().id, DEFAULT_TRACKING_MODEL);
        let ids: Vec<&str> = registry.tracking_models().iter().map(|model| model.id).collect();
        assert!(ids.len() >= 3, "XMem must not be the only tracker: {ids:?}");

        // XMem plans through the legacy worker script the bridge already speaks to.
        let xmem = registry.engine("xmem").expect("xmem adapter");
        assert_eq!(xmem.descriptor().family, crate::models::Family::XMem);
        let error = registry.capabilities("sam2.1-tiny").expect_err("SAM 2.1 selects, it does not track");
        assert!(matches!(error, EngineError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn an_installed_cutie_is_planned_with_the_protocol_the_bridge_speaks() {
        let root = std::env::temp_dir().join(format!("frameforge-roto-registry-{}", std::process::id()));
        let dir = root.join("cutie-base");
        std::fs::create_dir_all(&dir).expect("model dir");
        std::fs::write(dir.join("cutie-base-mega.pth"), b"placeholder").expect("weights placeholder");
        let registry = BackendRegistry::new(Arc::new(FileProbe::new(&root)), "python.exe", "tools/roto_worker.py");

        let object = ObjectSpec::new(3, "Subject").with_prompts(vec![Prompt::Mask { frame: 0 }]);
        let request = TrackRequest::new(object, FrameRange::new(0, 9), Direction::Forward, (480, 270));
        let plan = registry.plan(DEFAULT_TRACKING_MODEL, &request, false).expect("planned");
        assert_eq!(plan.protocol, "json-lines-1");
        assert!(plan.dormant, "planning is dormant until the user runs it");
        assert!(plan.command.is_some(), "an external tracker has a command to describe");
        assert_eq!(plan.model, find(DEFAULT_TRACKING_MODEL).expect("catalogue").id);

        let started = registry.plan(DEFAULT_TRACKING_MODEL, &request, true).expect("planned");
        assert!(!started.dormant);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_request_without_prompts_is_refused_before_any_backend_is_chosen() {
        let registry = BackendRegistry::new(Arc::new(NoRuntime), "python.exe", "tools/roto_worker.py");
        let object = ObjectSpec::new(1, "Empty");
        let request = TrackRequest::new(object, FrameRange::new(0, 4), Direction::Forward, (64, 36));
        let error = registry.plan(DEFAULT_TRACKING_MODEL, &request, false).expect_err("a tracking run needs a prompt");
        assert!(matches!(error, EngineError::InvalidRequest(_)), "{error}");
    }
}
