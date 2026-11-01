//! The model catalogue and the model manager: what each engine is, what it may
//! legally be used for, what it weighs, and how an install is planned,
//! downloaded, paused, verified and removed.
//!
//! Nothing here runs model inference, and nothing here downloads anything by
//! itself: a catalogue entry is *metadata*, an install produces a plan and a
//! command line, and the only execution is the platform downloader the user
//! starts. Verification reuses [`frameforge_bridge::file_sha256`], so there is
//! exactly one SHA-256 implementation in the tree.
//!
//! Licence facts in this table were taken from each project's own repository
//! (LICENSE file or the repository metadata's SPDX id) — see
//! `docs/ROTO_ENGINE.md`, which records the exact sources and dates. Entries
//! whose redistribution is restricted are marked [`Redistribution::Prohibited`]
//! and can never be offered as a download: the app points at the publisher and
//! the user supplies the file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What an engine does for the rotoscoping pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Task {
    /// Turn prompts (clicks, boxes, masks) into an object mask on one frame.
    Selection,
    /// Propagate an object's identity and mask through time.
    Tracking,
    /// Score propagated masks and flag drift, jumps, occlusion and flicker.
    Temporal,
    /// Turn a hard mask into a soft alpha matte.
    Matting,
}

impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Task::Selection => "Interactive selection",
            Task::Tracking => "Video tracking",
            Task::Temporal => "Temporal quality control",
            Task::Matting => "Matting and alpha",
        }
    }
}

/// The model family, so the UI can group engines without string matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Family {
    Sam2,
    EdgeTam,
    Cutie,
    Samurai,
    XMem,
    Rvm,
    MatAnyone,
    /// Implemented in Rust inside FrameForge (no weights, no runtime).
    BuiltIn,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::Sam2 => "SAM 2.1",
            Family::EdgeTam => "EdgeTAM",
            Family::Cutie => "Cutie",
            Family::Samurai => "SAMURAI",
            Family::XMem => "XMem",
            Family::Rvm => "Robust Video Matting",
            Family::MatAnyone => "MatAnyone",
            Family::BuiltIn => "FrameForge (Rust)",
        }
    }
}

/// How an engine's inference is executed. Every AI option is out-of-process:
/// the editor never links PyTorch, ONNX Runtime or any model runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeKind {
    /// A Python process the user installed, driven over the JSON-lines protocol.
    ExternalPython,
    /// An ONNX Runtime process (execution provider chosen by the user).
    ExternalOnnx,
    /// Rust code inside FrameForge: deterministic image maths, no weights.
    BuiltInRust,
}

impl RuntimeKind {
    pub fn label(self) -> &'static str {
        match self {
            RuntimeKind::ExternalPython => "external Python process",
            RuntimeKind::ExternalOnnx => "external ONNX Runtime process",
            RuntimeKind::BuiltInRust => "built into FrameForge (Rust)",
        }
    }

    /// Whether this engine needs a user-supplied runtime and weights at all.
    pub fn needs_runtime(self) -> bool {
        !matches!(self, RuntimeKind::BuiltInRust)
    }
}

/// What the licence allows FrameForge to do with the weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Redistribution {
    /// Permissive enough to offer an in-app download of the publisher's file.
    Permitted,
    /// Restricted (non-commercial or unknown terms): the app must never offer,
    /// bundle or mirror it; the user obtains it from the publisher.
    Prohibited,
    /// Terms not established yet: treated as prohibited until reviewed.
    Unknown,
}

impl Redistribution {
    pub fn allows_download(self) -> bool {
        matches!(self, Redistribution::Permitted)
    }

    pub fn label(self) -> &'static str {
        match self {
            Redistribution::Permitted => "download offered",
            Redistribution::Prohibited => "user-supplied only",
            Redistribution::Unknown => "unreviewed: user-supplied only",
        }
    }
}

/// One licence record, with the source it was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseRecord {
    /// SPDX identifier, or `NOASSERTION` for a custom licence.
    pub spdx: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    /// Whether commercial use is permitted by the terms read.
    pub commercial_use: bool,
    pub redistribution: Redistribution,
    pub note: &'static str,
    /// The date the terms were read from `url` (ISO-8601), or `None` when they
    /// have not been verified yet.
    pub verified: Option<&'static str>,
}

/// One publisher weight file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightFile {
    pub url: &'static str,
    /// The publisher's file size. `None` means it was not verified, and the
    /// manager then shows "size unknown until the download is planned".
    pub size_bytes: Option<u64>,
    /// The publisher's SHA-256. `None` means the user pins it after the first
    /// download (the manager always computes and shows the digest it saw).
    pub sha256: Option<&'static str>,
}

impl WeightFile {
    pub fn file_name(&self) -> String {
        self.url.rsplit('/').next().unwrap_or("weights").to_string()
    }
}

/// A model version the catalogue can install and use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVersion {
    pub version: &'static str,
    pub variant: &'static str,
    pub weights: &'static [WeightFile],
    /// Rough peak resident set the publisher or our own measurement reported.
    pub approx_ram_mb: Option<u32>,
    pub note: &'static str,
}

/// One catalogue entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub family: Family,
    pub task: Task,
    pub runtime: RuntimeKind,
    pub license: LicenseRecord,
    pub versions: &'static [ModelVersion],
    pub description: &'static str,
    pub intended_use: &'static str,
}

const SAM2_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "Apache-2.0",
    name: "Apache License 2.0",
    url: "https://github.com/facebookresearch/sam2/blob/main/LICENSE",
    commercial_use: true,
    redistribution: Redistribution::Permitted,
    note: "Repository LICENSE read directly; checkpoints are published by Meta at dl.fbaipublicfiles.com.",
    verified: Some("2026-11-01"),
};

const APACHE_META_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "Apache-2.0",
    name: "Apache License 2.0",
    url: "https://github.com/facebookresearch/EdgeTAM",
    commercial_use: true,
    redistribution: Redistribution::Permitted,
    note: "Repository metadata reports Apache-2.0 (SPDX id read from the GitHub API).",
    verified: Some("2026-11-01"),
};

const CUTIE_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "MIT",
    name: "MIT License",
    url: "https://github.com/hkchengrex/Cutie/blob/main/LICENSE",
    commercial_use: true,
    redistribution: Redistribution::Permitted,
    note: "LICENSE read directly (Copyright (c) 2024 Ho Kei Cheng).",
    verified: Some("2026-11-01"),
};

const SAMURAI_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "Apache-2.0",
    name: "Apache License 2.0",
    url: "https://github.com/yangchris11/samurai",
    commercial_use: true,
    redistribution: Redistribution::Permitted,
    note: "Repository metadata reports Apache-2.0; SAMURAI builds on SAM 2 and reuses SAM 2.1 checkpoints.",
    verified: Some("2026-11-01"),
};

const XMEM_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "MIT",
    name: "MIT License",
    url: "https://github.com/hkchengrex/XMem/blob/main/LICENSE",
    commercial_use: true,
    redistribution: Redistribution::Permitted,
    note: "Already the engine FrameForge ships a protocol adapter for; pinned commit recorded in docs/PROVENANCE.md.",
    verified: Some("2026-11-01"),
};

const RVM_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "GPL-3.0-or-later",
    name: "GNU General Public License v3",
    url: "https://github.com/PeterL1n/RobustVideoMatting/blob/master/LICENSE",
    commercial_use: true,
    redistribution: Redistribution::Prohibited,
    note: "Copyleft: must not be bundled with or linked into FrameForge Pro. Any use is an isolated, user-installed external process, and a licence review is required before the adapter is enabled.",
    verified: Some("2026-11-01"),
};

const MATANYONE_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "NOASSERTION",
    name: "S-Lab License 1.0 (non-commercial)",
    url: "https://github.com/pq-yang/MatAnyone/blob/main/LICENSE",
    commercial_use: false,
    redistribution: Redistribution::Prohibited,
    note: "Non-commercial use only; commercial use needs written permission from the authors. MatAnyone 2 is published under an unasserted custom licence. Neither may be bundled, mirrored or offered as a download by this app.",
    verified: Some("2026-11-01"),
};

const BUILTIN_LICENSE: LicenseRecord = LicenseRecord {
    spdx: "MIT OR Apache-2.0",
    name: "FrameForge Pro's own licence",
    url: "https://github.com/moonvexinary-lgtm/FrameForgePro",
    commercial_use: true,
    redistribution: Redistribution::Permitted,
    note: "Deterministic Rust image maths: no weights, no runtime, no inference.",
    verified: Some("2026-11-01"),
};

/// The catalogue. Sizes marked `Some` were verified with a HEAD request to the
/// publisher's URL on 2026-11-01 (metadata only — no weights were downloaded).
pub const CATALOGUE: &[ModelDescriptor] = &[
    ModelDescriptor {
        id: "sam2.1-tiny",
        name: "SAM 2.1 Tiny",
        family: Family::Sam2,
        task: Task::Selection,
        runtime: RuntimeKind::ExternalPython,
        license: SAM2_LICENSE,
        versions: &[ModelVersion {
            version: "2.1",
            variant: "hiera-tiny",
            weights: &[WeightFile {
                url: "https://dl.fbaipublicfiles.com/segment_anything_2/092824/sam2.1_hiera_tiny.pt",
                size_bytes: Some(156_008_466),
                sha256: None,
            }],
            approx_ram_mb: Some(1_500),
            note: "Fastest SAM 2.1 promptable segmentation; the default selection backend.",
        }],
        description: "Promptable image and video segmentation (points, boxes and masks) at the smallest SAM 2.1 size.",
        intended_use: "Choose an object on one frame from clicks or a box; hand the mask to a tracking backend.",
    },
    ModelDescriptor {
        id: "sam2.1-small",
        name: "SAM 2.1 Small",
        family: Family::Sam2,
        task: Task::Selection,
        runtime: RuntimeKind::ExternalPython,
        license: SAM2_LICENSE,
        versions: &[ModelVersion {
            version: "2.1",
            variant: "hiera-small",
            weights: &[WeightFile {
                url: "https://dl.fbaipublicfiles.com/segment_anything_2/092824/sam2.1_hiera_small.pt",
                size_bytes: Some(184_416_285),
                sha256: None,
            }],
            approx_ram_mb: Some(2_000),
            note: "Higher-quality alternative to Tiny when the selector needs more capacity.",
        }],
        description: "Promptable segmentation one step above Tiny, for subjects with softer boundaries.",
        intended_use: "Selection backend for hair, fur or cluttered edges where Tiny under-segments.",
    },
    ModelDescriptor {
        id: "edgetam",
        name: "EdgeTAM",
        family: Family::EdgeTam,
        task: Task::Selection,
        runtime: RuntimeKind::ExternalPython,
        license: APACHE_META_LICENSE,
        versions: &[ModelVersion {
            version: "1.0",
            variant: "on-device",
            weights: &[WeightFile { url: "https://github.com/facebookresearch/EdgeTAM", size_bytes: None, sha256: None }],
            approx_ram_mb: Some(1_200),
            note: "Weights are published from the project page; the exact asset URL and size were NOT verified from this environment, so the manager requires the user to confirm both.",
        }],
        description: "On-device track-anything model: interactive segmentation tuned for CPU-class hardware.",
        intended_use: "Alternative selection backend when the machine is CPU-only or the SAM 2.1 download is refused.",
    },
    ModelDescriptor {
        id: "cutie-base",
        name: "Cutie Base",
        family: Family::Cutie,
        task: Task::Tracking,
        runtime: RuntimeKind::ExternalPython,
        license: CUTIE_LICENSE,
        versions: &[ModelVersion {
            version: "1.0",
            variant: "base-mega",
            weights: &[WeightFile {
                url: "https://github.com/hkchengrex/Cutie/releases/download/v1.0/cutie-base-mega.pth",
                size_bytes: Some(140_443_788),
                sha256: None,
            }],
            approx_ram_mb: Some(2_500),
            note: "Preferred tracking engine: object-level memory that tolerates long occlusions.",
        }],
        description: "Video object segmentation with object-level memory and mask-correction support.",
        intended_use: "Default tracking backend for the tracking and propagation stages.",
    },
    ModelDescriptor {
        id: "samurai",
        name: "SAMURAI",
        family: Family::Samurai,
        task: Task::Tracking,
        runtime: RuntimeKind::ExternalPython,
        license: SAMURAI_LICENSE,
        versions: &[ModelVersion {
            version: "1.0",
            variant: "sam2.1-base-plus",
            weights: &[WeightFile {
                url: "https://dl.fbaipublicfiles.com/segment_anything_2/092824/sam2.1_hiera_base_plus.pt",
                size_bytes: Some(323_606_802),
                sha256: None,
            }],
            approx_ram_mb: Some(4_000),
            note: "Motion-aware memory added to SAM 2.1; it reuses the SAM 2.1 base+ checkpoint rather than publishing its own.",
        }],
        description: "Zero-shot visual tracking with motion-aware memory, aimed at fast movement and occlusion.",
        intended_use: "Optional tracking backend for fast or heavily occluded subjects.",
    },
    ModelDescriptor {
        id: "xmem",
        name: "XMem",
        family: Family::XMem,
        task: Task::Tracking,
        runtime: RuntimeKind::ExternalPython,
        license: XMEM_LICENSE,
        versions: &[ModelVersion {
            version: "1.0",
            variant: "XMem.pth",
            weights: &[WeightFile { url: "https://github.com/hkchengrex/XMem/releases/download/v1.0/XMem.pth", size_bytes: Some(249_026_057), sha256: None }],
            approx_ram_mb: Some(710),
            note: "Legacy compatibility backend: the engine FrameForge already speaks protocol 1 to, kept as a fallback.",
        }],
        description: "Long-term video object segmentation (the engine FrameForge's existing protocol adapter targets).",
        intended_use: "Fallback tracker when Cutie or SAMURAI are unavailable, and for existing projects.",
    },
    ModelDescriptor {
        id: "roto.temporal.qc",
        name: "Temporal quality control",
        family: Family::BuiltIn,
        task: Task::Temporal,
        runtime: RuntimeKind::BuiltInRust,
        license: BUILTIN_LICENSE,
        versions: &[ModelVersion {
            version: "1",
            variant: "metrics",
            weights: &[],
            approx_ram_mb: None,
            note: "Model-independent mask metrics: area change, centroid jumps, IoU drops, disappearance and flicker.",
        }],
        description: "Scores propagated masks against each other and flags ranges that need review.",
        intended_use: "Automatic drift detection between tracking and caching, for any backend.",
    },
    ModelDescriptor {
        id: "roto.matte.edge",
        name: "Edge matting (Rust)",
        family: Family::BuiltIn,
        task: Task::Matting,
        runtime: RuntimeKind::BuiltInRust,
        license: BUILTIN_LICENSE,
        versions: &[ModelVersion {
            version: "1",
            variant: "feather-and-smooth",
            weights: &[],
            approx_ram_mb: None,
            note: "Deterministic alpha refinement: feather, expand/contract, edge smoothing and premultiply. No model, no weights.",
        }],
        description: "Softens and corrects a hard mask into an alpha matte without any model.",
        intended_use: "Default matting stage; model-based matting is optional and separately licensed.",
    },
    ModelDescriptor {
        id: "rvm",
        name: "Robust Video Matting",
        family: Family::Rvm,
        task: Task::Matting,
        runtime: RuntimeKind::ExternalPython,
        license: RVM_LICENSE,
        versions: &[ModelVersion {
            version: "1.0",
            variant: "mobilenetv3",
            weights: &[],
            approx_ram_mb: Some(1_500),
            note: "NOT downloadable from this app: GPL-3.0. The owner must supply their own copy and accept the licence before any adapter is enabled.",
        }],
        description: "High-detail video matting for hair, fur, motion blur and semi-transparent edges.",
        intended_use: "Optional external matting backend, disabled until the licence review clears it.",
    },
    ModelDescriptor {
        id: "matanyone",
        name: "MatAnyone / MatAnyone 2",
        family: Family::MatAnyone,
        task: Task::Matting,
        runtime: RuntimeKind::ExternalPython,
        license: MATANYONE_LICENSE,
        versions: &[ModelVersion {
            version: "1/2",
            variant: "video-matting",
            weights: &[],
            approx_ram_mb: Some(2_000),
            note: "NOT downloadable from this app: S-Lab 1.0 (non-commercial) and an unasserted custom licence for version 2.",
        }],
        description: "Video matting with a learned quality evaluator.",
        intended_use: "Considered only for non-commercial use, as an isolated external process the user installed.",
    },
];

/// The whole catalogue.
pub fn catalogue() -> &'static [ModelDescriptor] {
    CATALOGUE
}

/// Look one entry up by id.
pub fn find(id: &str) -> Option<&'static ModelDescriptor> {
    CATALOGUE.iter().find(|model| model.id == id)
}

/// Every entry that serves a task, in catalogue order.
pub fn by_task(task: Task) -> Vec<&'static ModelDescriptor> {
    CATALOGUE.iter().filter(|model| model.task == task).collect()
}

/// The tracking engine the workspace prefers when nothing else is chosen.
pub const DEFAULT_TRACKING_MODEL: &str = "cutie-base";
/// The selection engine the workspace prefers.
pub const DEFAULT_SELECTION_MODEL: &str = "sam2.1-tiny";

/// The default tracking backend, falling back to the legacy engine if the
/// preferred one ever leaves the catalogue.
pub fn default_tracking() -> &'static ModelDescriptor {
    find(DEFAULT_TRACKING_MODEL).unwrap_or(&CATALOGUE[0])
}

/// Whether a model may be offered as an in-app download.
pub fn may_download(id: &str) -> bool {
    find(id).is_some_and(|model| model.license.redistribution.allows_download() && model.runtime.needs_runtime())
}

/// The total size of the given models' weights, or `None` when any size is
/// still unverified. Never guesses.
pub fn total_bytes(ids: &[&str]) -> Option<u64> {
    let mut total = 0u64;
    for id in ids {
        let model = find(id)?;
        for version in model.versions {
            for weight in version.weights {
                total = total.checked_add(weight.size_bytes?)?;
            }
        }
    }
    Some(total)
}

/// One file an install has to fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadStep {
    pub url: String,
    pub file_name: String,
    pub size_bytes: Option<u64>,
    pub destination: PathBuf,
    /// Always true for a fresh plan: a resumed download is the manager's job.
    pub resumable: bool,
}

/// What installing one model involves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub model: &'static str,
    pub steps: Vec<DownloadStep>,
    /// Bytes the catalogue could verify.
    pub known_bytes: u64,
    /// Files whose size the catalogue could not verify.
    pub unknown_sizes: usize,
    /// Where the model expects to find its weights at run time.
    pub install_root: PathBuf,
}

impl InstallPlan {
    /// The total size when every file's size is known.
    pub fn exact_bytes(&self) -> Option<u64> {
        (self.unknown_sizes == 0).then_some(self.known_bytes)
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("unknown model '{0}'")]
    UnknownModel(String),
    #[error("the model '{0}' has no downloadable weights; the publisher supplies them")]
    PublisherSupplied(String),
    #[error("the licence for '{0}' does not allow this app to download or mirror it")]
    LicenseRefused(String),
    #[error("'{0}' is already installed at {1}")]
    AlreadyInstalled(String, String),
    #[error("the model needs a runtime; {0}")]
    NoRuntime(String),
}

/// Plan an install for one model. `installed` is consulted so an existing
/// install is reported instead of silently re-downloading duplicate weights.
pub fn install_plan(model_id: &str, install_root: &Path, installed: bool) -> Result<InstallPlan, PlanError> {
    let model = find(model_id).ok_or_else(|| PlanError::UnknownModel(model_id.to_string()))?;
    if installed {
        return Err(PlanError::AlreadyInstalled(model_id.to_string(), install_root.display().to_string()));
    }
    if !model.runtime.needs_runtime() {
        return Err(PlanError::NoRuntime(format!("{} is built in", model.name)));
    }
    let mut steps = Vec::new();
    for version in model.versions {
        for weight in version.weights {
            let file_name = weight.file_name();
            steps.push(DownloadStep {
                url: weight.url.to_string(),
                destination: install_root.join(model.id).join(&file_name),
                file_name,
                size_bytes: weight.size_bytes,
                resumable: true,
            });
        }
    }
    if steps.is_empty() {
        // Distinguish "the publisher does not host a file we may mirror" from a
        // broken catalogue entry.
        return Err(if model.license.redistribution.allows_download() {
            PlanError::PublisherSupplied(model_id.to_string())
        } else {
            PlanError::LicenseRefused(model_id.to_string())
        });
    }
    if !model.license.redistribution.allows_download() {
        return Err(PlanError::LicenseRefused(model_id.to_string()));
    }
    let known_bytes = steps.iter().filter_map(|step| step.size_bytes).sum();
    let unknown_sizes = steps.iter().filter(|step| step.size_bytes.is_none()).count();
    Ok(InstallPlan { model: model.id, steps, known_bytes, unknown_sizes, install_root: install_root.join(model.id) })
}

/// A platform downloader invocation. FrameForge does not implement its own TLS
/// stack: it drives the downloader the operating system already has, which is
/// also what makes pause/resume real (`curl -C -` / `-Resume`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Render the download (or resume) command for one step.
pub fn download_command(step: &DownloadStep, resume: bool) -> DownloadCommand {
    #[cfg(windows)]
    {
        let mut args = vec!["-L".to_string(), "--fail".to_string(), "--create-dirs".to_string(), "-o".to_string(), step.destination.display().to_string()];
        if resume {
            args.push("-C".to_string());
            args.push("-".to_string());
        }
        args.push(step.url.clone());
        DownloadCommand { program: "curl.exe".to_string(), args }
    }
    #[cfg(not(windows))]
    {
        let mut args = vec!["-L".to_string(), "--fail".to_string(), "--create-dirs".to_string(), "-o".to_string(), step.destination.display().to_string()];
        if resume {
            args.push("-C".to_string());
            args.push("-".to_string());
        }
        args.push(step.url.clone());
        DownloadCommand { program: "curl".to_string(), args }
    }
}

/// How a caller stops a download: kill the process, then resume with the same
/// command plus `resume = true`.
pub const CANCEL_HINT: &str = "Cancel kills the downloader; the partial file stays on disk and Resume restarts the same command with -C -";

/// The install state machine the manager drives. Every transition is explicit so
/// the UI can pause, resume, cancel and verify without re-deriving anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallState {
    NotInstalled,
    Downloading {
        done_bytes: u64,
        total_bytes: Option<u64>,
    },
    Paused {
        done_bytes: u64,
        total_bytes: Option<u64>,
    },
    Verifying,
    Installed {
        sha256: String,
    },
    Failed {
        reason: String,
    },
    Cancelled {
        done_bytes: u64,
    },
    /// The licence forbids this app from fetching it.
    Refused {
        reason: String,
    },
}

impl InstallState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, InstallState::Installed { .. } | InstallState::Failed { .. } | InstallState::Cancelled { .. } | InstallState::Refused { .. })
    }

    /// Progress in `0.0..=1.0`, when the total size is known.
    pub fn fraction(&self) -> Option<f64> {
        let (done, total) = match self {
            InstallState::Downloading { done_bytes, total_bytes } | InstallState::Paused { done_bytes, total_bytes } => (*done_bytes, (*total_bytes)?),
            _ => return None,
        };
        (total > 0).then(|| (done as f64 / total as f64).clamp(0.0, 1.0))
    }
}

/// One tracked install in the manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub model: &'static str,
    pub state: InstallState,
    /// Set once the file is on disk, for verification.
    pub file: Option<PathBuf>,
    /// The digest the user pinned, when they have one.
    pub expected_sha256: Option<String>,
}

impl Install {
    pub fn new(model_id: &str) -> Option<Install> {
        find(model_id).map(|model| Install { model: model.id, state: InstallState::NotInstalled, file: None, expected_sha256: None })
    }

    pub fn begin(&mut self, plan: &InstallPlan) -> Result<(), PlanError> {
        if !may_download(self.model) {
            self.state = InstallState::Refused { reason: format!("the licence for {} does not allow an in-app download", self.model) };
            return Err(PlanError::LicenseRefused(self.model.to_string()));
        }
        self.state = InstallState::Downloading { done_bytes: 0, total_bytes: plan.exact_bytes() };
        Ok(())
    }

    /// Report downloaded bytes. The manager computes this from the file it can
    /// see, so a resumed download continues from the real size on disk.
    pub fn report(&mut self, done_bytes: u64, total_bytes: Option<u64>) {
        if let InstallState::Downloading { .. } = self.state {
            self.state = InstallState::Downloading { done_bytes, total_bytes };
        }
    }

    pub fn pause(&mut self) {
        if let InstallState::Downloading { done_bytes, total_bytes } = self.state {
            self.state = InstallState::Paused { done_bytes, total_bytes };
        }
    }

    pub fn resume(&mut self) {
        if let InstallState::Paused { done_bytes, total_bytes } = self.state {
            self.state = InstallState::Downloading { done_bytes, total_bytes };
        }
    }

    pub fn cancel(&mut self, done_bytes: u64) {
        if !self.state.is_terminal() {
            self.state = InstallState::Cancelled { done_bytes };
        }
    }

    pub fn fail(&mut self, reason: impl Into<String>) {
        if !self.state.is_terminal() {
            self.state = InstallState::Failed { reason: reason.into() };
        }
    }

    /// Verify the downloaded file and, on success, mark the model installed.
    /// The digest is computed with the one SHA-256 in the tree.
    pub fn verify(&mut self, path: &Path, expected: Option<&str>) -> Result<String, VerifyError> {
        self.state = InstallState::Verifying;
        match verify_file(path, expected) {
            Ok(digest) => {
                self.state = InstallState::Installed { sha256: digest.clone() };
                self.file = Some(path.to_path_buf());
                self.expected_sha256 = expected.map(str::to_string);
                Ok(digest)
            }
            Err(error) => {
                self.fail(error.to_string());
                Err(error)
            }
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("cannot read {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("the SHA-256 of {path} is {found}, not the {expected} the user pinned")]
    Mismatch { path: String, expected: String, found: String },
    #[error("the pinned SHA-256 is not 64 hex digits: {0}")]
    BadPin(String),
}

/// Hash a file (streamed) and, when the caller pinned a digest, check it.
pub fn verify_file(path: &Path, expected: Option<&str>) -> Result<String, VerifyError> {
    let digest = frameforge_bridge::file_sha256(path)
        .map(|bytes| frameforge_bridge::hex(&bytes))
        .map_err(|error| VerifyError::Io { path: path.display().to_string(), reason: error.to_string() })?;
    if let Some(expected) = expected {
        let expected = expected.trim().to_ascii_lowercase();
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(VerifyError::BadPin(expected));
        }
        if digest != expected {
            return Err(VerifyError::Mismatch { path: path.display().to_string(), expected, found: digest });
        }
    }
    Ok(digest)
}

/// The size of a file on disk, if it is there. The manager uses this to show
/// download progress without trusting the downloader's output.
pub fn installed_bytes(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// Whether a planned install fits the budget the user set. Free-space probing
/// needs an OS API this crate deliberately does not add a dependency for, so the
/// caller passes the budget it wants to honour and the unknown sizes are
/// reported rather than assumed to be zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskBudget {
    pub budget_mb: u64,
}

impl DiskBudget {
    pub fn fits(self, plan: &InstallPlan) -> bool {
        plan.exact_bytes().is_some_and(|bytes| bytes <= self.budget_mb.saturating_mul(1024 * 1024))
    }

    pub fn describe(self, plan: &InstallPlan) -> String {
        match plan.exact_bytes() {
            Some(bytes) => format!("{:.1} GB of a {} GB budget", bytes as f64 / 1e9, self.budget_mb as f64 / 1024.0),
            None => format!("size unknown for {} file(s); budget {} GB", plan.unknown_sizes, self.budget_mb as f64 / 1024.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_never_mentions_sam3_and_every_entry_is_documented() {
        for model in catalogue() {
            let haystack = format!("{} {} {} {}", model.id, model.name, model.description, model.intended_use).to_lowercase();
            assert!(!haystack.contains("sam3") && !haystack.contains("sam 3"), "{} references SAM 3", model.id);
            assert!(!model.license.url.is_empty(), "{} has no licence source", model.id);
            assert!(model.license.verified.is_some(), "{} has an unverified licence record", model.id);
            assert!(!model.description.is_empty() && !model.intended_use.is_empty(), "{} is undocumented", model.id);
        }
    }

    #[test]
    fn the_default_tracking_engine_is_cutie_and_xmem_is_kept_as_a_fallback() {
        assert_eq!(default_tracking().id, DEFAULT_TRACKING_MODEL);
        assert_eq!(default_tracking().family, Family::Cutie);
        let trackers = by_task(Task::Tracking);
        let ids: Vec<&str> = trackers.iter().map(|model| model.id).collect();
        assert!(ids.contains(&"cutie-base"), "Cutie must be a tracking backend: {ids:?}");
        assert!(ids.contains(&"samurai"), "SAMURAI must be an optional tracking backend: {ids:?}");
        assert!(ids.contains(&"xmem"), "XMem must stay available: {ids:?}");
        assert!(ids.len() >= 3, "XMem must not be the only tracking engine: {ids:?}");
    }

    #[test]
    fn restricted_licences_cannot_be_downloaded_and_say_why() {
        assert!(!may_download("rvm"), "RVM is GPL-3.0 and must not be mirrored by this app");
        assert!(!may_download("matanyone"), "MatAnyone is non-commercial and must not be mirrored");
        assert!(may_download("cutie-base"), "Cutie is MIT");
        assert!(may_download("sam2.1-tiny"), "SAM 2.1 is Apache-2.0");

        let root = Path::new("C:\\models");
        let refused = install_plan("matanyone", root, false).expect_err("a restricted licence is refused");
        assert!(matches!(refused, PlanError::LicenseRefused(_)), "{refused}");
        let rvm = install_plan("rvm", root, false).expect_err("RVM has no offered weights");
        assert!(matches!(rvm, PlanError::LicenseRefused(_)), "{rvm}");
    }

    #[test]
    fn an_install_plan_uses_the_publishers_verified_sizes_and_writes_where_it_says() {
        let root = Path::new("C:\\models");
        let plan = install_plan("cutie-base", root, false).expect("Cutie is installable");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.exact_bytes(), Some(140_443_788), "the size was verified with a HEAD request");
        assert_eq!(plan.steps[0].file_name, "cutie-base-mega.pth");
        assert!(plan.steps[0].destination.ends_with("cutie-base/cutie-base-mega.pth"), "the plan writes into the model's own directory");
        assert_eq!(DiskBudget { budget_mb: 200 }.fits(&plan), true, "140 MB fits a 200 MB budget");
        assert_eq!(DiskBudget { budget_mb: 100 }.fits(&plan), false);
    }

    #[test]
    fn a_model_with_an_unverified_size_reports_unknown_rather_than_guessing() {
        let root = Path::new("C:\\models");
        let plan = install_plan("edgetam", root, false).expect("EdgeTAM is installable once the user confirms the asset");
        assert_eq!(plan.exact_bytes(), None);
        assert_eq!(plan.unknown_sizes, 1);
        assert!(DiskBudget { budget_mb: 1 }.describe(&plan).contains("unknown"));
    }

    #[test]
    fn resuming_downloads_the_same_file_with_the_continue_flag() {
        let step = DownloadStep {
            url: "https://example.invalid/model.pth".to_string(),
            file_name: "model.pth".to_string(),
            size_bytes: Some(10),
            destination: PathBuf::from("C:\\models\\m\\model.pth"),
            resumable: true,
        };
        let fresh = download_command(&step, false);
        let resumed = download_command(&step, true);
        assert!(fresh.args.contains(&step.url), "the command names the publisher's url");
        assert!(!fresh.args.contains(&"-C".to_string()), "a fresh download does not ask to continue");
        assert!(resumed.args.contains(&"-C".to_string()), "a resume continues the partial file");
        assert_eq!(fresh.args.iter().filter(|arg| *arg == &step.url).count(), 1);
        assert!(CANCEL_HINT.contains("partial file stays"), "{CANCEL_HINT}");
    }

    #[test]
    fn the_install_state_machine_pauses_resumes_cancels_and_verifies() {
        let mut install = Install::new("cutie-base").expect("catalogue entry");
        assert_eq!(install.state, InstallState::NotInstalled);

        let plan = install_plan("cutie-base", Path::new("C:\\models"), false).expect("plan");
        install.begin(&plan).expect("begin");
        assert_eq!(install.state.fraction(), Some(0.0));

        install.report(70_000_000, Some(140_443_788));
        let half = install.state.fraction().expect("progress");
        assert!((half - 0.498).abs() < 0.01, "{half}");

        install.pause();
        assert!(matches!(install.state, InstallState::Paused { .. }));
        install.resume();
        assert!(matches!(install.state, InstallState::Downloading { .. }));

        install.cancel(70_000_000);
        assert!(matches!(install.state, InstallState::Cancelled { done_bytes: 70_000_000 }));
        assert!(install.state.is_terminal());

        // A cancelled install can be failed into, but never silently overwritten.
        install.fail("network gone");
        assert!(matches!(install.state, InstallState::Cancelled { .. }), "a terminal state is preserved");
    }

    #[test]
    fn verification_pins_the_digest_and_reports_a_mismatch_with_both_hashes() {
        let root = std::env::temp_dir().join(format!("frameforge-roto-verify-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp root");
        let file = root.join("abc.bin");
        std::fs::write(&file, b"abc").expect("write");

        // The FIPS 180-4 digest of "abc", and the file the manager would verify.
        let digest = verify_file(&file, None).expect("hash the file");
        assert_eq!(digest, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        verify_file(&file, Some(&digest)).expect("a matching pin passes");

        let error = verify_file(&file, Some(&"1".repeat(64))).expect_err("a wrong pin is refused");
        match error {
            VerifyError::Mismatch { expected, found, .. } => {
                assert!(expected.starts_with('1'));
                assert_eq!(found, digest);
            }
            other => panic!("expected a mismatch, got {other}"),
        }
        assert!(matches!(verify_file(&file, Some("nope")).expect_err("a short pin is refused"), VerifyError::BadPin(_)));
        assert_eq!(installed_bytes(&file), Some(3));
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&root);
    }
}
