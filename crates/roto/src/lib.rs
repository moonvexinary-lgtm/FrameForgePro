//! FrameForge Pro's rotoscoping engine: a model-independent pipeline for
//! interactive selection, video tracking, temporal quality control, soft matting,
//! refinement, caching, scheduling and export.
//!
//! # The design rule that matters most
//!
//! **Nothing in this crate runs model inference, and nothing here can.** Backends
//! are described by [`engine::RotoEngine::plan`], which returns a
//! [`engine::LaunchPlan`] — a description of the process that *would* run, with
//! `dormant: true` unless the caller explicitly reports that the user asked for a
//! run. The only code in the tree that ever starts a real engine is
//! `frameforge-bridge`'s worker client, which needs the user's consent, a
//! verified runtime and an explicit click in the workspace. Tests in this crate
//! therefore cannot load weights even by accident.
//!
//! # The pipeline
//!
//! ```text
//!  selection  ->  tracking  ->  temporal QC  ->  matting  ->  refinement
//!      |             |              |              |            |
//!      +-------------+--------------+--------------+------------+
//!                                cache -> export
//! ```
//!
//! * [`selection`] — points, boxes and mask prompts in image coordinates.
//! * [`tracking`] — backends (Cutie preferred, SAMURAI optional, XMem legacy),
//!   per-object track state and resumable propagation segments.
//! * [`temporal`] — model-independent mask metrics that flag drift, jumps,
//!   disappearance, flicker and ragged edges, with a recovery suggestion.
//! * [`matting`] — deterministic feather/expand/smooth/alpha work plus the seam
//!   for a learned matting backend.
//! * [`refinement`] — boolean combination, morphology, polygon fill and
//!   correction keyframes.
//! * [`brush`] — the paint/erase strokes the workspace sends.
//! * [`cache`] — a multi-object, multi-model mask cache with its own key.
//! * [`scheduler`] — one queue for every job, with pause, resume and cancel.
//! * [`export`] — binary, grayscale, alpha and RGBA output.
//! * [`runtime`] — hardware and provider capability *reporting* (never probing
//!   by running a model).
//!
//! # Models, licences and where they live
//!
//! The catalogue in [`models`] is the single source of truth: names, versions,
//! verified sizes, runtimes and licences. As of 2026-11-01 the verified licences
//! are: SAM 2.1 and EdgeTAM (Meta, Apache-2.0), SAMURAI (Apache-2.0), Cutie and
//! XMem (MIT) — all `Redistribution::Permitted`; Robust Video Matting (GPL-3.0)
//! and MatAnyone / MatAnyone 2 (S-Lab 1.0, non-commercial, plus an unasserted
//! custom licence for version 2) are `Redistribution::Prohibited`, so this app
//! never bundles, mirrors or offers them for download.
//!
//! SAM 3 and SAM 3.1 are deliberately absent: the catalogue test fails if any
//! entry so much as mentions them.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod brush;
pub mod cache;
pub mod engine;
pub mod export;
pub mod matting;
pub mod models;
pub mod refinement;
pub mod runtime;
pub mod scheduler;
pub mod selection;
pub mod temporal;
pub mod tracking;

pub use engine::{
    EngineCapabilities, EngineError, ExternalEngine, FileProbe, FrameRange, LaunchPlan, NoRuntime, ObjectSpec, RotoEngine, RuntimeProbe, TrackRequest,
};
pub use matting::{MatteMode, MattingParams, Plane};
pub use models::{Family, InstallPlan, InstallState, LicenseRecord, ModelDescriptor, PlanError, Redistribution, RuntimeKind, Task, catalogue, install_plan};
pub use selection::{BoxRect, Prompt, SelectionSession};
pub use temporal::{MaskSummary, QcLimits, QualityReport, RecoveryAction, SignalKind};
pub use tracking::{BackendRegistry, Direction, ObjectTrack, Segment, TrackStatus};
