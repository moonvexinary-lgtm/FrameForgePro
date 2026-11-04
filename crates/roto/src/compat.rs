//! What each model could run on, and what this machine actually has.
//!
//! Two rules shape this module:
//!
//! * **A path is not a promise.** Knowing that a model family is published as
//!   PyTorch weights, and that ONNX Runtime has a DirectML provider, does not
//!   establish that one model's operators run on the other. Every cell starts
//!   [`SupportStatus::Unverified`] and stays there until a worker reports the
//!   provider and the operators it used.
//! * **Finding a file is not verification.** [`detect_files`] looks for the
//!   runtime's DLLs in directories the caller names. It never loads a library,
//!   starts a process or asks a driver anything, so a hit means "this file
//!   exists", nothing more.
//!
//! The matrix exists so the workspace can say, per model, which route is known,
//! which is a candidate and which is impossible — instead of offering a GPU
//! toggle that would silently fall back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::models::{Family, Task};

/// How far a claim has been established.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportStatus {
    /// Measured or exercised by a test in this tree.
    Verified,
    /// Plausible, but not established on this machine or by a test.
    Unverified,
    /// Not offered by the publisher, or known not to work.
    Unsupported,
}

impl SupportStatus {
    pub fn label(self) -> &'static str {
        match self {
            SupportStatus::Verified => "verified",
            SupportStatus::Unverified => "unverified",
            SupportStatus::Unsupported => "not available",
        }
    }
}

/// The route a model's inference would take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderPath {
    /// The publisher's PyTorch code, driven out of process.
    PythonTorch,
    /// An ONNX graph in an ONNX Runtime process (DirectML or CPU provider).
    OnnxRuntime,
    /// FrameForge's own Rust code, no weights.
    BuiltIn,
}

impl ProviderPath {
    pub fn label(self) -> &'static str {
        match self {
            ProviderPath::PythonTorch => "PyTorch (external Python)",
            ProviderPath::OnnxRuntime => "ONNX Runtime (external process)",
            ProviderPath::BuiltIn => "built in (Rust)",
        }
    }
}

/// One model's compatibility cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ModelCompat {
    pub model_id: &'static str,
    pub family: Family,
    pub task: Task,
    /// The route FrameForge would take today.
    pub path: ProviderPath,
    /// Whether the publisher ships an ONNX export. A conversion we would have to
    /// write ourselves is not a published export.
    pub onnx_export: SupportStatus,
    /// Windows ML wraps ONNX Runtime; the same operator question applies.
    pub windows_ml: SupportStatus,
    /// Execution providers worth trying, in order, for this model's path.
    pub candidate_providers: &'static [&'static str],
    /// What the workspace says, in plain words.
    pub note: &'static str,
}

/// The matrix, one row per model in the catalogue that does inference.
pub fn matrix() -> &'static [ModelCompat] {
    &MATRIX
}

pub fn find(model_id: &str) -> Option<&'static ModelCompat> {
    MATRIX.iter().find(|entry| entry.model_id == model_id)
}

const MATRIX: [ModelCompat; 8] = [
    ModelCompat {
        model_id: "sam2.1-tiny",
        family: Family::Sam2,
        task: Task::Selection,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unverified,
        windows_ml: SupportStatus::Unverified,
        candidate_providers: &["cpu"],
        note: "156 MB checkpoint and the smallest official SAM 2.1 selector. No Meta-published ONNX export; community conversions exist but their operator coverage is unverified. PyTorch on CPU is the route this build would take.",
    },
    ModelCompat {
        model_id: "sam2.1-small",
        family: Family::Sam2,
        task: Task::Selection,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unverified,
        windows_ml: SupportStatus::Unverified,
        candidate_providers: &["cpu"],
        note: "184 MB checkpoint: better masks than Tiny, slower. Same ONNX status: a candidate, not a supported path.",
    },
    ModelCompat {
        model_id: "edgetam",
        family: Family::EdgeTam,
        task: Task::Selection,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unsupported,
        windows_ml: SupportStatus::Unsupported,
        candidate_providers: &["cpu"],
        note: "Meta's mobile SAM 2 variant, published as PyTorch weights only. The catalogue's weight URL and size are still unverified, so nothing is offered for download.",
    },
    ModelCompat {
        model_id: "cutie-base",
        family: Family::Cutie,
        task: Task::Tracking,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unsupported,
        windows_ml: SupportStatus::Unsupported,
        candidate_providers: &["cpu"],
        note: "The preferred tracker, 140 MB, PyTorch only. Preferred means the default candidate, not a speed claim: on integrated graphics it may well be slower than the user expects.",
    },
    ModelCompat {
        model_id: "samurai",
        family: Family::Samurai,
        task: Task::Tracking,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unsupported,
        windows_ml: SupportStatus::Unsupported,
        candidate_providers: &["cpu"],
        note: "Optional motion-aware tracking built on SAM 2.1 checkpoints; PyTorch code, no publishers' ONNX export.",
    },
    ModelCompat {
        model_id: "xmem",
        family: Family::XMem,
        task: Task::Tracking,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unsupported,
        windows_ml: SupportStatus::Unsupported,
        candidate_providers: &["cpu"],
        note: "Legacy fallback with a real worker and a local CPU probe (0.818 fps forward at 480x270 on the target laptop). That number is a measurement, not a target for other models.",
    },
    ModelCompat {
        model_id: "rvm",
        family: Family::Rvm,
        task: Task::Matting,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unsupported,
        windows_ml: SupportStatus::Unsupported,
        candidate_providers: &[],
        note: "GPL-3.0-or-later: catalogued so the licence is visible, never downloaded, bundled, mirrored or driven by this build.",
    },
    ModelCompat {
        model_id: "matanyone",
        family: Family::MatAnyone,
        task: Task::Matting,
        path: ProviderPath::PythonTorch,
        onnx_export: SupportStatus::Unsupported,
        windows_ml: SupportStatus::Unsupported,
        candidate_providers: &[],
        note: "S-Lab 1.0, non-commercial: catalogued so the restriction is visible, never downloaded, bundled, mirrored or driven by this build.",
    },
];

/// Why this build does not offer DirectML for the PyTorch path.
///
/// DirectML is an ONNX Runtime execution provider (and a Windows ML one). It is
/// not an execution provider for the PyTorch wheels these checkpoints need, and
/// ROCm has no consumer Windows build. So on a Ryzen 5 7530U the honest PyTorch
/// route is the CPU; DirectML only becomes reachable through a validated ONNX
/// export of a specific checkpoint.
pub const DIRECTML_NOTE: &str = "DirectML is reachable through ONNX Runtime / Windows ML, not through the PyTorch wheels these checkpoints use. No ONNX export has been validated for these models, so no DirectML run is offered.";

/// A runtime file found on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeFile {
    pub provider: &'static str,
    pub file: &'static str,
    pub path: PathBuf,
}

/// The files whose presence would make a provider worth testing. Presence is
/// not compatibility and no file here is loaded.
pub const RUNTIME_FILES: &[(&str, &str)] = &[
    ("onnx-directml", "onnxruntime.dll"),
    ("onnx-directml", "onnxruntime_providers_dml.dll"),
    ("onnx-directml", "DirectML.dll"),
    ("windows-ml", "Microsoft.AI.MachineLearning.dll"),
    ("python-torch-cpu", "torch_cpu.dll"),
];

/// Look for known runtime files in the given directories. No library is loaded,
/// no process is started and nothing is queried.
pub fn detect_files(dirs: &[&Path]) -> Vec<RuntimeFile> {
    let mut found = Vec::new();
    for dir in dirs {
        for (provider, file) in RUNTIME_FILES.iter().copied() {
            let path = dir.join(file);
            if path.is_file() {
                found.push(RuntimeFile { provider, file, path });
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path && a.file == b.file);
    found
}

/// The directories worth checking without asking anyone: the executable's own
/// folder (where a runtime shipped beside the app would live) and `PATH`.
pub fn standard_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe) = std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)) {
        dirs.push(exe);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if !dir.as_os_str().is_empty() {
                dirs.push(dir);
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// A sentence the panel can show about what was found. It never says "available".
pub fn detection_note(files: &[RuntimeFile]) -> String {
    if files.is_empty() {
        return "No known runtime files were found beside the app or on PATH. Nothing is claimed about DirectML or Windows ML.".to_string();
    }
    let mut names: Vec<&str> = files.iter().map(|file| file.file).collect();
    names.sort_unstable();
    names.dedup();
    format!(
        "Found {} beside the app or on PATH. Presence is not verification: the provider is still unprobed and no model is claimed to run on it.",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("frameforge-roto-compat-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn nothing_in_the_matrix_claims_a_verified_onnx_or_directml_path() {
        for entry in matrix() {
            assert_ne!(entry.onnx_export, SupportStatus::Verified, "{} claims a verified ONNX export", entry.model_id);
            assert_ne!(entry.windows_ml, SupportStatus::Verified, "{} claims verified Windows ML", entry.model_id);
            assert!(!entry.candidate_providers.contains(&"directml"), "{} must not offer DirectML", entry.model_id);
            if !entry.candidate_providers.is_empty() {
                assert!(entry.candidate_providers.contains(&"cpu"), "{} keeps a CPU fallback", entry.model_id);
            }
        }
    }

    #[test]
    fn the_matrix_names_every_inference_model_in_the_catalogue() {
        for model in crate::models::catalogue() {
            if model.family == Family::BuiltIn {
                continue;
            }
            assert!(find(model.id).is_some(), "{} has no compatibility row", model.id);
            let entry = find(model.id).expect("row");
            assert_eq!(entry.task, model.task, "{} row disagrees with the catalogue", model.id);
            assert_eq!(entry.family, model.family, "{} row disagrees with the catalogue", model.id);
        }
    }

    #[test]
    fn cutie_and_sam_keep_their_expected_routes() {
        assert_eq!(find("cutie-base").expect("row").path, ProviderPath::PythonTorch);
        assert_eq!(find("sam2.1-tiny").expect("row").task, Task::Selection);
        assert!(DIRECTML_NOTE.contains("not through the PyTorch wheels"));
    }

    #[test]
    fn detection_reports_a_file_without_claiming_compatibility() {
        let dir = temp("found");
        fs::write(dir.join("DirectML.dll"), b"not a real library").expect("write");
        let found = detect_files(&[dir.as_path()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, "DirectML.dll");
        let note = detection_note(&found);
        assert!(note.contains("DirectML.dll"));
        assert!(note.contains("Presence is not verification"), "{note}");
        assert!(!note.contains("available"), "{note}");

        let empty = temp("empty");
        assert!(detect_files(&[empty.as_path()]).is_empty());
        assert!(detection_note(&[]).contains("Nothing is claimed"));
    }
}
