//! Interactive segmentation, independent of any one model.
//!
//! The workspace draws prompts ([`crate::selection`]); this module turns them
//! into a versioned request, validates the reply a worker must send back, and
//! describes the process that *would* run. It starts nothing, imports nothing and
//! loads no weights: a segmentation plan is dormant unless the caller reports
//! that the user asked for a mask.
//!
//! # Why the protocol is its own document
//!
//! Tracking (`json-lines-1`) is a long-lived conversation over a frame range.
//! Selection is one question with one answer — prompts in, one mask out — and
//! that difference is worth a smaller, stricter contract:
//!
//! * the request names its version, the frame, the image size, the prompts in
//!   image pixels and the longest side the model may run at;
//! * the reply names the same frame, the size it actually produced and the mask
//!   file path — a reply for another frame or size is rejected, never resized
//!   silently;
//! * the request's digest names the cache entry, so changing a single point
//!   addresses a different result.
//!
//! One worker script can serve several selector families: the plan passes
//! `--model`, and the script chooses its adapter. Which families have an
//! adapter is stated in [`crate::compat`] and enforced here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::engine::{EngineError, ExternalCommand};
use crate::models::{Family, ModelDescriptor, Task};
use crate::selection::Prompt;

/// The protocol string written into every request and reply.
pub const SEGMENT_PROTOCOL: &str = "segment-1";

/// The protocol version this build speaks.
pub const SEGMENT_VERSION: u32 = 1;

/// The model families the shared segmentation worker has an adapter for.
///
/// SAMURAI is SAM 2.1-based but is a tracker in this build, and Cutie/XMem track
/// from a mask rather than answering a click, so they are refused with a reason
/// instead of being given a prompt protocol they cannot honour.
pub const SEGMENT_FAMILIES: [Family; 2] = [Family::Sam2, Family::EdgeTam];

/// One segmentation question: prompts for one frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentRequest {
    pub version: u32,
    pub frame: i64,
    pub width: u32,
    pub height: u32,
    /// The prompts in image pixels, in the order the user placed them.
    pub prompts: Vec<Prompt>,
    /// The 8-bit mask a `Mask` prompt refers to, as a path the worker can open.
    /// `None` is only valid when no `Mask` prompt is present.
    pub mask: Option<String>,
    /// The frame image the worker should read. `None` while the plan is still
    /// dormant: a runner exports the frame before it spawns the worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// The longest side the worker may resize to. Never upscales.
    pub max_side: u32,
}

impl SegmentRequest {
    /// Build a checked request, clamping prompts into the image.
    ///
    /// The rules exist so a backend never receives something it would have to
    /// guess at: a prompt from another frame, a zero-area box, a non-finite
    /// coordinate or an empty image is an error here, not a strange mask later.
    pub fn new(frame: i64, width: u32, height: u32, prompts: Vec<Prompt>, max_side: u32) -> Result<Self, SegmentError> {
        if width == 0 || height == 0 {
            return Err(SegmentError::EmptyImage);
        }
        if prompts.is_empty() {
            return Err(SegmentError::NoPrompts);
        }
        let mut checked = Vec::with_capacity(prompts.len());
        for prompt in prompts {
            if prompt.frame() != frame {
                return Err(SegmentError::FrameMismatch { prompt: prompt.frame(), frame });
            }
            let prompt = match prompt {
                Prompt::Point { x, y, positive, frame } => {
                    if !x.is_finite() || !y.is_finite() {
                        return Err(SegmentError::NonFinite);
                    }
                    let x = x.clamp(0.0, width.saturating_sub(1) as f32);
                    let y = y.clamp(0.0, height.saturating_sub(1) as f32);
                    Prompt::Point { x, y, positive, frame }
                }
                Prompt::Box { rect, frame } => {
                    if !rect.x0.is_finite() || !rect.y0.is_finite() || !rect.x1.is_finite() || !rect.y1.is_finite() {
                        return Err(SegmentError::NonFinite);
                    }
                    let rect = rect.clamp_to(width, height);
                    if rect.area() < 1.0 {
                        return Err(SegmentError::EmptyBox);
                    }
                    Prompt::Box { rect, frame }
                }
                Prompt::Mask { frame } => Prompt::Mask { frame },
            };
            checked.push(prompt);
        }
        Ok(Self { version: SEGMENT_VERSION, frame, width, height, prompts: checked, mask: None, image: None, max_side: max_side.clamp(64, 4_096) })
    }

    /// Attach the mask file a `Mask` prompt refers to.
    pub fn with_mask(mut self, path: &Path) -> Self {
        self.mask = Some(path.display().to_string());
        self
    }

    /// Attach the frame image the worker should read.
    pub fn with_image(mut self, path: &Path) -> Self {
        self.image = Some(path.display().to_string());
        self
    }

    /// Whether the prompts ask the worker to start from a mask it cannot open.
    pub fn misses_mask_file(&self) -> bool {
        self.prompts.iter().any(|prompt| matches!(prompt, Prompt::Mask { .. })) && self.mask.as_deref().is_none_or(|path| path.trim().is_empty())
    }

    /// The working size the worker should run the model at, preserving aspect.
    pub fn working_size(&self) -> (u32, u32) {
        let longest = self.width.max(self.height).max(1);
        if longest <= self.max_side {
            return (self.width, self.height);
        }
        let scale = self.max_side as f32 / longest as f32;
        (((self.width as f32 * scale).round() as u32).max(1), ((self.height as f32 * scale).round() as u32).max(1))
    }

    /// Canonical JSON: the exact bytes a worker is given.
    pub fn to_json(&self) -> Result<String, SegmentError> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self, SegmentError> {
        let request: Self = serde_json::from_str(text)?;
        if request.version != SEGMENT_VERSION {
            return Err(SegmentError::UnsupportedVersion(request.version));
        }
        if request.width == 0 || request.height == 0 {
            return Err(SegmentError::EmptyImage);
        }
        if request.prompts.is_empty() {
            return Err(SegmentError::NoPrompts);
        }
        if request.misses_mask_file() {
            return Err(SegmentError::MaskPromptWithoutFile);
        }
        Ok(request)
    }

    /// A digest over the canonical request, naming the cached result.
    pub fn digest(&self) -> Result<String, SegmentError> {
        let json = self.to_json()?;
        let mut hasher = frameforge_bridge::Sha256::new();
        hasher.update(json.as_bytes());
        Ok(frameforge_bridge::hex(&hasher.finish()))
    }

    /// A one-line description for the panel.
    pub fn summary(&self) -> String {
        let points = self.prompts.iter().filter(|prompt| matches!(prompt, Prompt::Point { .. })).count();
        let boxes = self.prompts.iter().filter(|prompt| matches!(prompt, Prompt::Box { .. })).count();
        let masks = self.prompts.iter().filter(|prompt| matches!(prompt, Prompt::Mask { .. })).count();
        let (w, h) = self.working_size();
        format!("frame {} · {points} point, {boxes} box, {masks} mask · runs at {w}x{h}", self.frame)
    }
}

/// What a worker must answer with: one mask, for the frame it was asked about.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentReply {
    pub version: u32,
    pub frame: i64,
    pub width: u32,
    pub height: u32,
    /// Path to the 8-bit mask the worker wrote.
    pub mask: String,
    /// The model's own confidence, when it reports one.
    pub score: Option<f32>,
    /// The execution provider that actually ran, e.g. `cpu`.
    pub provider: Option<String>,
    pub elapsed_ms: Option<u64>,
}

impl SegmentReply {
    pub fn parse(text: &str) -> Result<Self, SegmentError> {
        let reply: Self = serde_json::from_str(text)?;
        if reply.version != SEGMENT_VERSION {
            return Err(SegmentError::UnsupportedVersion(reply.version));
        }
        if reply.mask.trim().is_empty() {
            return Err(SegmentError::EmptyMask);
        }
        if reply.width == 0 || reply.height == 0 {
            return Err(SegmentError::EmptyImage);
        }
        Ok(reply)
    }

    pub fn mask_path(&self) -> PathBuf {
        PathBuf::from(&self.mask)
    }

    /// Check that the reply answers *this* request. A worker that returns a mask
    /// for the wrong frame or size is a protocol error, not something to resize.
    pub fn validate(&self, request: &SegmentRequest) -> Result<(), SegmentError> {
        if self.frame != request.frame {
            return Err(SegmentError::ReplyFrameMismatch { expected: request.frame, got: self.frame });
        }
        if self.width != request.width || self.height != request.height {
            return Err(SegmentError::ReplySizeMismatch { expected: (request.width, request.height), got: (self.width, self.height) });
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let provider = self.provider.as_deref().unwrap_or("provider not reported");
        let score = self.score.map(|score| format!(" · score {score:.3}")).unwrap_or_default();
        let time = self.elapsed_ms.map(|ms| format!(" · {ms} ms")).unwrap_or_default();
        format!("{}x{} on {provider}{score}{time}", self.width, self.height)
    }
}

/// The process a segmentation would start, described and never run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentPlan {
    pub protocol: &'static str,
    pub model: String,
    pub command: ExternalCommand,
    /// Where the runner must write [`SegmentPlan::request_json`] before spawning.
    pub request_path: PathBuf,
    /// Where the worker must write its reply.
    pub reply_path: PathBuf,
    pub request_json: String,
    pub weights: Vec<PathBuf>,
    /// True until the caller reports that the user asked for this mask.
    pub dormant: bool,
}

impl SegmentPlan {
    pub fn command_line(&self) -> String {
        let mut line = self.command.program.clone();
        for arg in &self.command.args {
            line.push(' ');
            line.push_str(arg);
        }
        line
    }

    pub fn starts_a_process(&self) -> bool {
        self.command.args.contains(&"--request".to_string())
    }
}

/// Describe a segmentation run for `model`, refusing what this build cannot do.
///
/// Nothing is written and nothing is spawned: the caller writes
/// [`SegmentPlan::request_json`] to [`SegmentPlan::request_path`] only when the
/// user has actually asked for a mask.
pub fn plan_segment(
    request: &SegmentRequest,
    worker: &Path,
    python: &Path,
    model: &ModelDescriptor,
    weights: &[PathBuf],
    workspace: &Path,
    user_started: bool,
) -> Result<SegmentPlan, EngineError> {
    if model.task != Task::Selection {
        return Err(EngineError::Unsupported { model: model.id, what: "interactive selection: this backend tracks from a mask instead" });
    }
    if !SEGMENT_FAMILIES.contains(&model.family) {
        return Err(EngineError::Unsupported { model: model.id, what: "the shared segmentation worker has no adapter for this family" });
    }
    if weights.is_empty() {
        return Err(EngineError::NotInstalled { model: model.id });
    }
    if request.misses_mask_file() {
        return Err(EngineError::InvalidRequest("a mask prompt needs the mask file it refers to".to_string()));
    }
    let request_json = request.to_json().map_err(|error| EngineError::InvalidRequest(error.to_string()))?;
    let digest = request.digest().map_err(|error| EngineError::InvalidRequest(error.to_string()))?;
    let stem = &digest[..16.min(digest.len())];
    let request_path = workspace.join(format!("segment-{stem}.request.json"));
    let reply_path = workspace.join(format!("segment-{stem}.reply.json"));

    let mut args = vec![
        "-u".to_string(),
        worker.display().to_string(),
        "--model".to_string(),
        model.id.to_string(),
        "--request".to_string(),
        request_path.display().to_string(),
        "--reply".to_string(),
        reply_path.display().to_string(),
        "--max-side".to_string(),
        request.max_side.to_string(),
    ];
    for weight in weights {
        args.push("--weights".to_string());
        args.push(weight.display().to_string());
    }
    Ok(SegmentPlan {
        protocol: SEGMENT_PROTOCOL,
        model: model.id.to_string(),
        command: ExternalCommand { program: python.display().to_string(), args },
        request_path,
        reply_path,
        request_json,
        weights: weights.to_vec(),
        dormant: !user_started,
    })
}

#[derive(Debug, Error)]
pub enum SegmentError {
    #[error("the image is empty")]
    EmptyImage,
    #[error("a segmentation request needs at least one prompt")]
    NoPrompts,
    #[error("a prompt is tagged frame {prompt} but the request is for frame {frame}")]
    FrameMismatch { prompt: i64, frame: i64 },
    #[error("a prompt coordinate is not finite")]
    NonFinite,
    #[error("the box prompt has no area; draw a box wider and taller than one pixel")]
    EmptyBox,
    #[error("a mask prompt needs the mask file it refers to")]
    MaskPromptWithoutFile,
    #[error("the request's JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("protocol version {0} is not supported by this build (expected 1)")]
    UnsupportedVersion(u32),
    #[error("the worker reported no mask")]
    EmptyMask,
    #[error("the worker answered for frame {got}, but the request was for frame {expected}")]
    ReplyFrameMismatch { expected: i64, got: i64 },
    #[error("the worker answered at {got:?}, but the request was for {expected:?}")]
    ReplySizeMismatch { expected: (u32, u32), got: (u32, u32) },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::find;
    use crate::selection::BoxRect;

    fn request(prompts: Vec<Prompt>) -> SegmentRequest {
        SegmentRequest::new(7, 200, 100, prompts, 480).expect("request")
    }

    #[test]
    fn a_request_clamps_prompts_into_the_image() {
        let request = request(vec![
            Prompt::Point { x: -20.0, y: 5_000.0, positive: true, frame: 7 },
            Prompt::Box { rect: BoxRect::from_corners(10.0, 10.0, 900.0, 900.0), frame: 7 },
        ]);
        let point = match &request.prompts[0] {
            Prompt::Point { x, y, .. } => (*x, *y),
            other => panic!("expected a point, got {other:?}"),
        };
        assert_eq!(point, (0.0, 99.0));
        let boxed = match &request.prompts[1] {
            Prompt::Box { rect, .. } => *rect,
            other => panic!("expected a box, got {other:?}"),
        };
        assert_eq!((boxed.x1, boxed.y1), (199.0, 99.0));
    }

    #[test]
    fn a_request_refuses_what_a_worker_could_only_guess_at() {
        assert!(SegmentRequest::new(7, 0, 100, vec![Prompt::Mask { frame: 7 }], 480).is_err(), "empty image");
        assert!(SegmentRequest::new(7, 200, 100, Vec::new(), 480).is_err(), "no prompts");
        assert!(SegmentRequest::new(7, 200, 100, vec![Prompt::Mask { frame: 8 }], 480).is_err(), "a prompt from another frame must be refused");
        assert!(
            SegmentRequest::new(7, 200, 100, vec![Prompt::Box { rect: BoxRect::from_corners(5.0, 5.0, 5.0, 5.0), frame: 7 }], 480).is_err(),
            "a zero-area box must be refused"
        );
        assert!(
            SegmentRequest::new(7, 200, 100, vec![Prompt::Point { x: f32::NAN, y: 1.0, positive: true, frame: 7 }], 480).is_err(),
            "a NaN coordinate must be refused"
        );
    }

    #[test]
    fn the_working_size_fits_the_longest_side_and_never_upscales() {
        let small = SegmentRequest::new(0, 320, 180, vec![Prompt::Mask { frame: 0 }], 480).expect("small");
        assert_eq!(small.working_size(), (320, 180), "no upscaling");

        let large = SegmentRequest::new(0, 1_920, 1_080, vec![Prompt::Mask { frame: 0 }], 480).expect("large");
        assert_eq!(large.working_size(), (480, 270));

        let tall = SegmentRequest::new(0, 1_080, 1_920, vec![Prompt::Mask { frame: 0 }], 240).expect("tall");
        assert_eq!(tall.working_size(), (135, 240));
    }

    #[test]
    fn the_request_json_round_trips_and_its_digest_tracks_the_prompts() {
        let one = request(vec![Prompt::Point { x: 40.0, y: 30.0, positive: true, frame: 7 }]);
        let json = one.to_json().expect("json");
        let back = SegmentRequest::from_json(&json).expect("round trip");
        assert_eq!(back, one);
        assert_eq!(back.digest().expect("digest"), one.digest().expect("digest"));
        assert_eq!(back.digest().expect("digest").len(), 64);

        let mut moved = one.clone();
        moved.prompts[0] = Prompt::Point { x: 41.0, y: 30.0, positive: true, frame: 7 };
        assert_ne!(moved.digest().expect("digest"), one.digest().expect("digest"), "one pixel is a different question");
    }

    #[test]
    fn a_reply_must_answer_the_frame_and_size_it_was_asked_about() {
        let request = request(vec![Prompt::Mask { frame: 7 }]);
        let text = r#"{"version":1,"frame":7,"width":200,"height":100,"mask":"C:\\m\\7.png","score":0.9,"provider":"cpu","elapsed_ms":120}"#;
        let reply = SegmentReply::parse(text).expect("reply");
        reply.validate(&request).expect("valid reply");
        assert_eq!(reply.mask_path(), PathBuf::from("C:\\m\\7.png"));
        assert!(reply.summary().contains("cpu"));

        let wrong_frame = SegmentReply { frame: 8, ..reply.clone() };
        assert!(wrong_frame.validate(&request).is_err());
        let wrong_size = SegmentReply { width: 100, ..reply.clone() };
        assert!(wrong_size.validate(&request).is_err());
        assert!(SegmentReply::parse(r#"{"version":2,"frame":7,"width":200,"height":100,"mask":"m.png"}"#).is_err());
        assert!(SegmentReply::parse(r#"{"version":1,"frame":7,"width":200,"height":100,"mask":"  "}"#).is_err());
    }

    #[test]
    fn a_plan_names_the_request_and_stays_dormant_until_the_user_starts_it() {
        let model = find("sam2.1-tiny").expect("sam2.1-tiny is catalogued");
        let weights = vec![PathBuf::from("C:\\models\\sam2.1_t.pt")];
        let request = request(vec![Prompt::Point { x: 4.0, y: 5.0, positive: true, frame: 7 }]);
        let workspace = PathBuf::from("C:\\work");

        let dormant = plan_segment(&request, Path::new("segment_worker.py"), Path::new("python.exe"), model, &weights, &workspace, false).expect("plan");
        assert!(dormant.dormant, "no click, no run");
        assert!(dormant.starts_a_process());
        assert_eq!(dormant.protocol, SEGMENT_PROTOCOL);
        let line = dormant.command_line();
        assert!(line.contains("--request") && line.contains("--model sam2.1-tiny"), "{line}");
        assert!(line.contains("--max-side 480"), "{line}");
        assert!(dormant.request_path.starts_with(&workspace) && dormant.reply_path.starts_with(&workspace));
        assert_eq!(SegmentRequest::from_json(&dormant.request_json).expect("json"), request);

        let started = plan_segment(&request, Path::new("segment_worker.py"), Path::new("python.exe"), model, &weights, &workspace, true).expect("plan");
        assert!(!started.dormant);
    }

    #[test]
    fn a_plan_refuses_a_tracker_and_a_missing_checkpoint() {
        let request = request(vec![Prompt::Point { x: 4.0, y: 5.0, positive: true, frame: 7 }]);
        let workspace = PathBuf::from("C:\\work");
        let cutie = find("cutie-base").expect("cutie is catalogued");
        let error = plan_segment(&request, Path::new("w.py"), Path::new("python.exe"), cutie, &[PathBuf::from("c.pth")], &workspace, true)
            .expect_err("cutie tracks from a mask");
        assert!(matches!(error, EngineError::Unsupported { .. }), "{error}");

        let sam2 = find("sam2.1-tiny").expect("sam2 is catalogued");
        let error = plan_segment(&request, Path::new("w.py"), Path::new("python.exe"), sam2, &[], &workspace, true).expect_err("no weights");
        assert!(matches!(error, EngineError::NotInstalled { .. }), "{error}");
    }

    #[test]
    fn a_mask_prompt_must_carry_its_mask_file() {
        let model = find("sam2.1-tiny").expect("sam2 is catalogued");
        let weights = vec![PathBuf::from("sam2.1_t.pt")];
        let bare = request(vec![Prompt::Mask { frame: 7 }]);
        assert!(bare.misses_mask_file());
        assert!(matches!(
            plan_segment(&bare, Path::new("w.py"), Path::new("python.exe"), model, &weights, Path::new("C:\\work"), true),
            Err(EngineError::InvalidRequest(_))
        ));

        let masked = bare.clone().with_mask(Path::new("C:\\work\\frame-7.png"));
        assert!(!masked.misses_mask_file());
        let json = masked.to_json().expect("json");
        assert_eq!(SegmentRequest::from_json(&json).expect("round trip"), masked);
        assert_ne!(masked.digest().expect("digest"), bare.digest().expect("digest"), "the mask file is part of the question");
        plan_segment(&masked, Path::new("w.py"), Path::new("python.exe"), model, &weights, Path::new("C:\\work"), true).expect("plan");

        let framed = masked.clone().with_image(Path::new("C:\\work\\frame-7.rgb.png"));
        assert_ne!(framed.digest().expect("digest"), masked.digest().expect("digest"), "the image is part of the question");
        assert_eq!(SegmentRequest::from_json(&framed.to_json().expect("json")).expect("round trip"), framed);
    }

    #[test]
    fn the_supported_families_are_exactly_the_selectors() {
        let names: Vec<&str> = SEGMENT_FAMILIES.iter().map(|family| family.label()).collect();
        assert_eq!(names, vec!["SAM 2.1", "EdgeTAM"]);
    }
}
