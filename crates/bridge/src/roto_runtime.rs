//! Consent-based discovery of the optional XMem runtime (R5).
//!
//! The editor never downloads a runtime or weights, and never guesses that some
//! file on the machine is "the model". The user points at a Python interpreter,
//! the official XMem checkout (MIT) and their own `XMem.pth`; this module then
//! checks, in order, that consent was given, that every path exists, that the
//! checkout is the pinned upstream commit, and that the weights really are the
//! bytes the caller claimed (SHA-256, streamed — the file is never held in
//! memory).
//!
//! Nothing here runs a process or reads a weights file unless consent was
//! given first, so a stray call cannot start an expensive or surprising job.
//! Starting the worker itself is [`crate::roto_worker::XmemWorker::spawn`] and
//! the policy above it is [`crate::roto_job::run_job`].

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::roto::XMEM_COMMIT;
use crate::sha256::{file_sha256, hex};

/// What the user pointed the shell at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotoRuntimeRequest {
    /// The interpreter that runs the worker script (`python.exe`, `py.exe`, …).
    pub python: PathBuf,
    /// The official XMem checkout, at [`XMEM_COMMIT`].
    pub xmem_source: PathBuf,
    /// The user's own weights file (never shipped, never downloaded here).
    pub weights: PathBuf,
    /// The protocol worker script (`tools/xmem_worker.py` in this repository).
    pub worker_script: PathBuf,
    /// The SHA-256 the user expects of `weights` (64 lowercase hex digits).
    pub expected_sha256: String,
}

/// The explicit "yes" that must be recorded before any file is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotoConsent {
    pub accepted: bool,
    /// Who agreed and on what basis, kept for the log and the status line.
    pub note: String,
}

impl RotoConsent {
    pub fn given(note: impl Into<String>) -> Self {
        Self { accepted: true, note: note.into() }
    }

    pub fn refused(note: impl Into<String>) -> Self {
        Self { accepted: false, note: note.into() }
    }
}

/// A resolved, verified runtime: the only thing that may be turned into a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotoRuntime {
    pub python: PathBuf,
    pub xmem_source: PathBuf,
    pub weights: PathBuf,
    pub worker_script: PathBuf,
    pub weights_sha256: String,
    pub xmem_commit: String,
    pub consent_note: String,
}

impl RotoRuntime {
    /// Consent first, then existence, then the checkout, then the weights.
    pub fn resolve(request: &RotoRuntimeRequest, consent: &RotoConsent) -> Result<Self, RotoRuntimeError> {
        if !consent.accepted {
            return Err(RotoRuntimeError::NotConsented(if consent.note.is_empty() {
                "the user has not agreed to use an external runtime and weights".to_string()
            } else {
                consent.note.clone()
            }));
        }

        let expected = request.expected_sha256.trim().to_ascii_lowercase();
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RotoRuntimeError::BadHash(request.expected_sha256.clone()));
        }

        require_file("the Python interpreter", &request.python)?;
        require_file("the worker script", &request.worker_script)?;
        require_file("the weights file", &request.weights)?;
        if !request.xmem_source.is_dir() {
            return Err(RotoRuntimeError::Missing { role: "the XMem checkout", path: request.xmem_source.clone() });
        }

        let commit = checkout_commit(&request.xmem_source)?;
        if commit != XMEM_COMMIT {
            return Err(RotoRuntimeError::Commit { expected: XMEM_COMMIT.to_string(), found: commit });
        }

        let found = file_sha256(&request.weights).map_err(|error| RotoRuntimeError::Io { path: request.weights.clone(), error: error.to_string() })?;
        let found = hex(&found);
        if found != expected {
            return Err(RotoRuntimeError::WeightsHash { expected, found });
        }

        Ok(Self {
            python: request.python.clone(),
            xmem_source: request.xmem_source.clone(),
            weights: request.weights.clone(),
            worker_script: request.worker_script.clone(),
            weights_sha256: found,
            xmem_commit: commit,
            consent_note: consent.note.clone(),
        })
    }

    /// The command line the worker is spawned with: `python -u <script>`.
    /// `-u` keeps the worker's reports unbuffered, which is what makes its
    /// progress events arrive while the job is still running.
    pub fn worker_command(&self) -> (PathBuf, Vec<String>) {
        (self.python.clone(), vec!["-u".to_string(), self.worker_script.display().to_string()])
    }

    /// A one-line summary for the status bar and logs. Never claims that a job
    /// has run; it describes only what was verified.
    pub fn describe(&self) -> String {
        format!(
            "XMem {} · weights {} ({}) · consent: {}",
            &self.xmem_commit[..self.xmem_commit.len().min(7)],
            self.weights.display(),
            &self.weights_sha256[..self.weights_sha256.len().min(12)],
            self.consent_note
        )
    }
}

fn require_file(role: &'static str, path: &Path) -> Result<(), RotoRuntimeError> {
    if path.is_file() { Ok(()) } else { Err(RotoRuntimeError::Missing { role, path: path.to_path_buf() }) }
}

/// Read the checkout's commit without running `git`: `.git` may be a directory,
/// a `gitdir:` file (worktree/submodule), a symbolic ref, a packed ref or a
/// detached HEAD. Anything unreadable is reported, never guessed.
fn checkout_commit(dir: &Path) -> Result<String, RotoRuntimeError> {
    let dot_git = dir.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).map_err(|error| RotoRuntimeError::Io { path: dot_git.clone(), error: error.to_string() })?;
        let target = text
            .strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .ok_or_else(|| RotoRuntimeError::NotACheckout { path: dir.to_path_buf(), reason: ".git names no git directory".to_string() })?;
        let target = Path::new(target);
        if target.is_absolute() { target.to_path_buf() } else { dir.join(target) }
    } else {
        return Err(RotoRuntimeError::NotACheckout { path: dir.to_path_buf(), reason: "no .git (not a git checkout)".to_string() });
    };

    if !dir.join("model").is_dir() {
        return Err(RotoRuntimeError::NotACheckout { path: dir.to_path_buf(), reason: "no model/ directory (not the XMem repository)".to_string() });
    }

    let head_path = git_dir.join("HEAD");
    let head = std::fs::read_to_string(&head_path).map_err(|error| RotoRuntimeError::Io { path: head_path.clone(), error: error.to_string() })?;
    let head = head.trim();

    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        let loose = git_dir.join(reference);
        if let Ok(text) = std::fs::read_to_string(&loose) {
            let sha = text.trim().to_ascii_lowercase();
            if !sha.is_empty() {
                return Ok(sha);
            }
        }
        if let Some(sha) = packed_ref(&git_dir.join("packed-refs"), reference) {
            return Ok(sha);
        }
        return Err(RotoRuntimeError::CommitUnreadable { path: dir.to_path_buf(), reason: format!("{reference} is not in refs or packed-refs") });
    }

    if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(head.to_ascii_lowercase());
    }
    Err(RotoRuntimeError::CommitUnreadable { path: dir.to_path_buf(), reason: "HEAD is neither a ref nor a commit id".to_string() })
}

/// `<sha> <ref>` lines in `packed-refs` (`^` lines carry the peeled tag after it).
fn packed_ref(packed: &Path, reference: &str) -> Option<String> {
    let text = std::fs::read_to_string(packed).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(sha) = parts.next() else { continue };
        if parts.next() == Some(reference) {
            return Some(sha.to_ascii_lowercase());
        }
    }
    None
}

/// Interpreters to try, in order: an explicitly configured one, then the usual
/// names in the `PATH` directories. Pure and side-effect free — it looks for
/// files, it never runs an interpreter to ask its version.
pub fn python_candidates(configured: Option<&Path>, path_entries: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    if let Some(configured) = configured
        && configured.is_file()
    {
        found.push(configured.to_path_buf());
    }
    for name in ["python.exe", "py.exe", "python3.exe"] {
        for entry in path_entries {
            let candidate = entry.join(name);
            if candidate.is_file() && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// [`python_candidates`] against the real environment.
pub fn python_candidates_from_env() -> Vec<PathBuf> {
    let configured = std::env::var_os("FRAMEFORGE_PYTHON").map(PathBuf::from);
    let entries: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
    python_candidates(configured.as_deref(), &entries)
}

#[derive(Debug, Error)]
pub enum RotoRuntimeError {
    #[error("roto runtime not consented: {0}")]
    NotConsented(String),
    #[error("roto runtime is missing {role}: {path}")]
    Missing { role: &'static str, path: PathBuf },
    #[error("not an XMem checkout at {path}: {reason}")]
    NotACheckout { path: PathBuf, reason: String },
    #[error("the weights SHA-256 does not match: expected {expected}, found {found}")]
    WeightsHash { expected: String, found: String },
    #[error("expected XMem commit {expected}, found {found}")]
    Commit { expected: String, found: String },
    #[error("the XMem checkout commit at {path} could not be read: {reason}")]
    CommitUnreadable { path: PathBuf, reason: String },
    #[error("the weights SHA-256 is not 64 hex digits: {0}")]
    BadHash(String),
    #[error("roto runtime I/O at {path}: {error}")]
    Io { path: PathBuf, error: String },
}
