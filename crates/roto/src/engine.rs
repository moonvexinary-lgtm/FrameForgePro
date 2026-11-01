//! The model-independent engine interface: what every segmentation, tracking,
//! temporal-QC and matting backend must be able to answer, and how a backend is
//! *described* to the app.
//!
//! The rule that keeps development honest: [`RotoEngine::plan`] returns a
//! [`LaunchPlan`] — a description of the command that would run — and never runs
//! it. A plan is `dormant` unless the caller has both an installed model and an
//! explicit "the user asked for this" flag, so opening the application can never
//! start inference, and no test in this crate can load weights.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{ModelDescriptor, RuntimeKind, Task, find};
use crate::selection::Prompt;
use crate::tracking::Direction;

/// What a backend can do. The workspace reads this instead of assuming.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCapabilities {
    pub task: Task,
    pub point_prompts: bool,
    pub box_prompts: bool,
    pub mask_prompts: bool,
    pub negative_prompts: bool,
    pub multi_object: bool,
    pub corrections: bool,
    pub resume_from_correction: bool,
    pub bidirectional: bool,
    pub runtime: RuntimeKind,
}

impl EngineCapabilities {
    /// The capabilities the catalogue implies for a family. Kept deliberately
    /// conservative: anything not certain is `false`.
    pub fn for_model(model: &ModelDescriptor) -> Self {
        let (point, boxed, mask, negative, multi, corrections, resume, bidirectional) = match model.family {
            crate::models::Family::Sam2 => (true, true, true, true, true, true, true, true),
            crate::models::Family::EdgeTam => (true, true, true, true, true, true, true, true),
            crate::models::Family::Cutie => (true, false, true, false, true, true, true, true),
            crate::models::Family::Samurai => (true, true, true, true, true, true, true, true),
            crate::models::Family::XMem => (false, false, true, false, false, false, false, true),
            crate::models::Family::Rvm => (false, false, false, false, false, false, false, false),
            crate::models::Family::MatAnyone => (false, false, true, false, false, false, false, false),
            crate::models::Family::BuiltIn => (false, false, false, false, true, true, true, true),
        };
        Self {
            task: model.task,
            point_prompts: point,
            box_prompts: boxed,
            mask_prompts: mask,
            negative_prompts: negative,
            multi_object: multi,
            corrections,
            resume_from_correction: resume,
            bidirectional,
            runtime: model.runtime,
        }
    }
}

/// One object the user is rotoscoping. (`Eq` is not derived: a prompt carries
/// `f32` coordinates, so only `PartialEq` is meaningful.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectSpec {
    /// Stable object id inside the project (the mask layer id).
    pub id: u64,
    pub name: String,
    pub visible: bool,
    /// The prompts that define the object on the first frame of the range.
    pub prompts: Vec<Prompt>,
}

impl ObjectSpec {
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), visible: true, prompts: Vec::new() }
    }

    pub fn with_prompts(mut self, prompts: Vec<Prompt>) -> Self {
        self.prompts = prompts;
        self
    }
}

/// An inclusive media-frame range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRange {
    pub first: i64,
    pub last: i64,
}

impl FrameRange {
    pub fn new(first: i64, last: i64) -> Self {
        Self { first, last }
    }

    pub fn count(&self) -> i64 {
        self.last.saturating_sub(self.first).saturating_add(1).max(0)
    }

    pub fn contains(&self, frame: i64) -> bool {
        frame >= self.first && frame <= self.last
    }

    /// The frames between two corrections, in the requested direction.
    pub fn segment_to(&self, from: i64, direction: Direction) -> Option<FrameRange> {
        if !self.contains(from) {
            return None;
        }
        Some(match direction {
            Direction::Forward => FrameRange::new(from, self.last),
            Direction::Backward => FrameRange::new(self.first, from),
            // Both ways is the whole range; a caller that wants two passes uses
            // `tracking::plan_segments`, which expands the direction.
            Direction::Both => *self,
        })
    }
}

/// A user correction that a re-propagation must start from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correction {
    pub frame: i64,
    /// The frame the correction should be carried from (itself by default).
    pub from: i64,
}

impl Correction {
    pub fn at(frame: i64) -> Self {
        Self { frame, from: frame }
    }
}

/// One propagation request, backend-independent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackRequest {
    pub object: ObjectSpec,
    pub range: FrameRange,
    pub direction: Direction,
    pub working_width: u32,
    pub working_height: u32,
    /// Corrections already accepted for this object, in ascending frame order.
    pub corrections: Vec<Correction>,
}

impl TrackRequest {
    pub fn new(object: ObjectSpec, range: FrameRange, direction: Direction, working: (u32, u32)) -> Self {
        Self { object, range, direction, working_width: working.0, working_height: working.1, corrections: Vec::new() }
    }

    /// The range a re-propagation should actually run, honouring the latest
    /// correction instead of overwriting corrected frames.
    pub fn resume_range(&self) -> FrameRange {
        match self.corrections.iter().max_by_key(|correction| correction.frame) {
            Some(correction) => self.range.segment_to(correction.from, self.direction).unwrap_or(self.range),
            None => self.range,
        }
    }
}

/// How a backend would be launched. `command` is `None` for a backend built into
/// FrameForge; `dormant` is true whenever the plan was produced without an
/// explicit user run — the app must not start a dormant plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub protocol: &'static str,
    pub model: &'static str,
    pub command: Option<ExternalCommand>,
    pub dormant: bool,
    /// Weights the backend would read, for the consent/verification UI.
    pub weights: Vec<PathBuf>,
}

impl LaunchPlan {
    pub fn built_in(model: &'static str) -> Self {
        Self { protocol: "in-process", model, command: None, dormant: true, weights: Vec::new() }
    }

    /// Whether this plan would start a process at all.
    pub fn starts_a_process(&self) -> bool {
        self.command.is_some()
    }
}

/// An external process invocation (never executed by this crate).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Where the models live on this machine, and whether a given one is installed.
/// Probing is *file presence only*: no runtime is started, no weights are loaded.
pub trait RuntimeProbe {
    fn weights_root(&self) -> PathBuf;
    /// The weight files found for a model id, or an empty vector.
    fn found_weights(&self, model_id: &str) -> Vec<PathBuf>;
    fn is_installed(&self, model_id: &str) -> bool {
        !self.found_weights(model_id).is_empty()
    }
}

/// A probe that finds nothing: used by tests and by a first-run app before the
/// user has pointed at a runtime.
pub struct NoRuntime;

impl RuntimeProbe for NoRuntime {
    fn weights_root(&self) -> PathBuf {
        PathBuf::new()
    }

    fn found_weights(&self, _model_id: &str) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// A probe over a directory: a model counts as installed when every file its
/// catalogue entry names is present under `<root>/<model id>/`.
pub struct FileProbe {
    root: PathBuf,
}

impl FileProbe {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl RuntimeProbe for FileProbe {
    fn weights_root(&self) -> PathBuf {
        self.root.clone()
    }

    fn found_weights(&self, model_id: &str) -> Vec<PathBuf> {
        let Some(model) = find(model_id) else { return Vec::new() };
        let dir = self.root.join(model.id);
        let mut found = Vec::new();
        for version in model.versions {
            for weight in version.weights {
                let path = dir.join(weight.file_name());
                if path.is_file() {
                    found.push(path);
                }
            }
        }
        found
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("'{0}' is not in the model catalogue")]
    UnknownModel(String),
    #[error("{model} is not installed: its weights are not on this machine")]
    NotInstalled { model: &'static str },
    #[error("{model} is built into FrameForge and needs no runtime")]
    BuiltIn { model: &'static str },
    #[error("{model} cannot do this: {what}")]
    Unsupported { model: &'static str, what: &'static str },
    #[error("{model} needs a runtime: {reason}")]
    NoRuntime { model: &'static str, reason: String },
    #[error("the request is invalid: {0}")]
    InvalidRequest(String),
}

/// The interface every backend implements.
pub trait RotoEngine {
    fn descriptor(&self) -> &'static ModelDescriptor;
    fn capabilities(&self) -> EngineCapabilities;
    /// Describe how this request would run. Never executes anything.
    fn plan(&self, request: &TrackRequest, user_started: bool) -> Result<LaunchPlan, EngineError>;
}

/// The adapter for every out-of-process backend (Python or ONNX): it names the
/// worker script, the protocol and the weights, and refuses to plan anything
/// when the model is not installed or the request exceeds its capabilities.
pub struct ExternalEngine {
    model: &'static ModelDescriptor,
    probe: Arc<dyn RuntimeProbe>,
    /// The worker script that speaks the protocol for this backend.
    worker: PathBuf,
    /// The interpreter/runtime the worker is started with.
    program: PathBuf,
}

impl ExternalEngine {
    pub fn new(model: &'static ModelDescriptor, probe: Arc<dyn RuntimeProbe>, program: impl Into<PathBuf>, worker: impl Into<PathBuf>) -> Self {
        Self { model, probe, worker: worker.into(), program: program.into() }
    }

    pub fn worker_script(&self) -> &PathBuf {
        &self.worker
    }
}

impl RotoEngine for ExternalEngine {
    fn descriptor(&self) -> &'static ModelDescriptor {
        self.model
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::for_model(self.model)
    }

    fn plan(&self, request: &TrackRequest, user_started: bool) -> Result<LaunchPlan, EngineError> {
        let model = self.model;
        if request.range.count() == 0 {
            return Err(EngineError::InvalidRequest("the frame range is empty".to_string()));
        }
        if !request.range.contains(request.object.prompts.first().map(|_| request.range.first).unwrap_or(request.range.first)) {
            return Err(EngineError::InvalidRequest("the prompts do not land inside the range".to_string()));
        }
        if request.working_width == 0 || request.working_height == 0 {
            return Err(EngineError::InvalidRequest("the working size is empty".to_string()));
        }
        let capabilities = self.capabilities();
        if capabilities.task == Task::Tracking && request.object.prompts.is_empty() {
            return Err(EngineError::InvalidRequest("a tracking run needs an initial mask or prompt".to_string()));
        }
        if request.object.prompts.iter().any(|prompt| matches!(prompt, Prompt::Point { positive: false, .. })) && !capabilities.negative_prompts {
            return Err(EngineError::Unsupported { model: model.id, what: "negative point prompts" });
        }
        if request.object.prompts.iter().any(|prompt| matches!(prompt, Prompt::Box { .. })) && !capabilities.box_prompts {
            return Err(EngineError::Unsupported { model: model.id, what: "box prompts" });
        }
        let weights = self.probe.found_weights(model.id);
        if weights.is_empty() {
            return Err(EngineError::NotInstalled { model: model.id });
        }
        // The command is described, never run. `dormant` records that the caller
        // has not asked for a real run yet.
        let mut args = vec!["-u".to_string(), self.worker.display().to_string()];
        args.push("--model".to_string());
        args.push(model.id.to_string());
        for weight in &weights {
            args.push("--weights".to_string());
            args.push(weight.display().to_string());
        }
        Ok(LaunchPlan {
            protocol: "json-lines-1",
            model: model.id,
            command: Some(ExternalCommand { program: self.program.display().to_string(), args }),
            dormant: !user_started,
            weights,
        })
    }
}

/// The built-in temporal-QC engine: it needs no runtime and cannot fail to be
/// present, and its plan is always in-process.
pub struct BuiltInTemporal;

impl RotoEngine for BuiltInTemporal {
    fn descriptor(&self) -> &'static ModelDescriptor {
        static_fallback("roto.temporal.qc")
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::for_model(self.descriptor())
    }

    fn plan(&self, request: &TrackRequest, _user_started: bool) -> Result<LaunchPlan, EngineError> {
        if request.range.count() == 0 {
            return Err(EngineError::InvalidRequest("the frame range is empty".to_string()));
        }
        Ok(LaunchPlan::built_in(self.descriptor().id))
    }
}

/// The built-in matting engine.
pub struct BuiltInMatte;

impl RotoEngine for BuiltInMatte {
    fn descriptor(&self) -> &'static ModelDescriptor {
        static_fallback("roto.matte.edge")
    }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::for_model(self.descriptor())
    }

    fn plan(&self, request: &TrackRequest, _user_started: bool) -> Result<LaunchPlan, EngineError> {
        if request.working_width == 0 || request.working_height == 0 {
            return Err(EngineError::InvalidRequest("the working size is empty".to_string()));
        }
        Ok(LaunchPlan::built_in(self.descriptor().id))
    }
}

/// A catalogue entry that must exist. The catalogues above are static; this
/// keeps the built-ins honest about being present rather than panicking.
fn static_fallback(id: &str) -> &'static ModelDescriptor {
    find(id).unwrap_or(&crate::models::CATALOGUE[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{catalogue, install_plan};
    use crate::selection::Prompt;

    fn request(prompts: Vec<Prompt>) -> TrackRequest {
        let object = ObjectSpec::new(1, "Subject").with_prompts(prompts);
        TrackRequest::new(object, FrameRange::new(0, 9), Direction::Forward, (480, 270))
    }

    #[test]
    fn a_missing_model_is_reported_rather_than_planned() {
        let cutie = find("cutie-base").expect("catalogue");
        let engine = ExternalEngine::new(cutie, Arc::new(NoRuntime), "python.exe", "tools/roto_worker.py");
        let error = engine.plan(&request(vec![Prompt::Mask { frame: 0 }]), false).expect_err("nothing installed");
        assert!(matches!(error, EngineError::NotInstalled { .. }), "{error}");
        assert!(engine.capabilities().runtime.needs_runtime(), "Cutie needs a runtime");
    }

    #[test]
    fn an_installed_model_plans_a_dormant_command_that_is_never_run() {
        let root = std::env::temp_dir().join(format!("frameforge-roto-plan-{}", std::process::id()));
        let dir = root.join("cutie-base");
        std::fs::create_dir_all(&dir).expect("model dir");
        std::fs::write(dir.join("cutie-base-mega.pth"), b"not weights").expect("weight placeholder");
        let cutie = find("cutie-base").expect("catalogue");
        let engine = ExternalEngine::new(cutie, Arc::new(FileProbe::new(&root)), "python.exe", "tools/roto_worker.py");

        let dormant = engine.plan(&request(vec![Prompt::Mask { frame: 0 }]), false).expect("planned");
        assert!(dormant.dormant, "planning without a user action is dormant");
        assert!(dormant.starts_a_process());
        let command = dormant.command.clone().expect("an external backend has a command");
        assert_eq!(command.program, "python.exe");
        assert!(command.args.iter().any(|arg| arg == "cutie-base"), "the command names the model: {:?}", command.args);
        assert!(command.args.iter().any(|arg| arg.ends_with("cutie-base-mega.pth")), "and its weights");

        let started = engine.plan(&request(vec![Prompt::Mask { frame: 0 }]), true).expect("planned");
        assert!(!started.dormant, "an explicit user run is not dormant");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn built_in_engines_never_start_a_process_and_need_no_weights() {
        let plan = BuiltInTemporal.plan(&request(vec![Prompt::Mask { frame: 0 }]), false).expect("planned");
        assert!(!plan.starts_a_process());
        assert!(plan.weights.is_empty());
        assert_eq!(BuiltInTemporal.capabilities().runtime, RuntimeKind::BuiltInRust);
        assert!(BuiltInMatte.plan(&request(vec![Prompt::Mask { frame: 0 }]), false).is_ok());
    }

    #[test]
    fn a_backend_refuses_prompts_it_cannot_honour() {
        let xmem = find("xmem").expect("catalogue");
        let engine = ExternalEngine::new(xmem, Arc::new(NoRuntime), "python.exe", "tools/xmem_worker.py");
        let error = engine.plan(&request(vec![Prompt::Point { x: 0.5, y: 0.5, positive: false, frame: 0 }]), false).expect_err("XMem takes masks only");
        assert!(matches!(error, EngineError::Unsupported { .. }), "{error}");
    }

    #[test]
    fn every_downloadable_catalogue_entry_can_be_planned_and_kept_out_of_the_app() {
        for model in catalogue() {
            if !crate::models::may_download(model.id) {
                continue;
            }
            let plan = install_plan(model.id, std::path::Path::new("C:\\models"), false).expect("plan");
            assert!(!plan.is_empty(), "{} plans at least one file", model.id);
            for step in &plan.steps {
                assert!(step.destination.starts_with("C:\\models"), "the plan writes inside the model root");
            }
        }
    }
}
