//! The integration layer of FrameForge Pro: EffectCraft effects, with keyframed
//! parameters, running on FilmCraft timeline media.
//!
//! Milestone M2 (see `docs/ARCHITECTURE.md`). The two engines meet here and only
//! here:
//!
//! * the **editing engine** (FilmCraft) owns the project, the timeline, the media
//!   sources, the CPU compositor and the export pipeline;
//! * the **compositing engine** (EffectCraft) owns the effect catalog and the
//!   pixel implementations behind each effect.
//!
//! [`ComposedSource`] is a `MediaSource` whose frames pass through a chain of
//! real EffectCraft effects before the timeline reads them, so preview and
//! export — both driven through FilmCraft's renderer — see exactly the same
//! picture. Parameters are animated with the *editing* engine's keyframe system
//! ([`filmcraft_project::Param`], media time, exact integer ticks), evaluated at
//! whatever time the renderer asks for: scrubbing forward and backward gives the
//! same frame, because evaluation is a pure function of time.
//!
//! [`FrameForgeProject`] (`.ffproj`) is the project document: the FilmCraft
//! editing project plus a compositing [`Composition`] per media item that has
//! one. Save and reopen are a JSON round-trip checked before anything is
//! written.
//!
//! Never-crash rules: no `unwrap`, `expect`, `panic!`, `todo!` or `unsafe` in
//! this crate; every failure is a value the caller can act on.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod autosave;
pub mod catalog;
pub mod convert;
pub mod document;
pub mod roto;
pub mod roto_worker;
pub mod source;

pub use catalog::{CatalogEntry, CatalogReport, ParamInfo, catalog, catalog_report, effect_name, effect_params, name_collisions};
/// The compositing engine's value and parameter-UI types, for shells that edit
/// effect parameters without depending on the engine crates directly.
pub use effectcraft_keyframe::Value as EffectValue;
pub use effectcraft_project::ParamUi as EffectParamUi;

pub use convert::param_to_value;
pub use document::{Composition, EffectLink, FFPROJ_SCHEMA, FrameForgeProject};
pub use roto::{MaskCache, MaskError, MaskFrame, RotoCacheKey, RotoRef, XMEM_COMMIT, default_cache_root};
pub use roto_worker::{
    ExportedFrames, FrameExportError, WORKER_PROTOCOL, WorkerCommand, WorkerDirection, WorkerError, WorkerEvent, WorkerInfo, XmemWorker, export_frames,
};
pub use source::{ComposedSource, ProjectSources};

use thiserror::Error;

/// What can go wrong in the integration layer. Every variant carries something
/// the user or an agent can act on.
#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    /// The document cannot be read with this build (wrong or newer schema).
    #[error("project format: {0}")]
    Format(String),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}
