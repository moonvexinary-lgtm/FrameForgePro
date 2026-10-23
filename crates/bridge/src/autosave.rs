//! Crash-recovery autosaves.
//!
//! While a saved project has unsaved changes, the shell writes an atomic
//! `<project>.ffproj.autosave` sidecar. If the application crashes, the next
//! open of that project finds an autosave **newer than the project file** and
//! offers it back. Saving the project clears the sidecar.
//!
//! The sidecar is a normal `.ffproj` document (same schema, same round-trip
//! check, same atomic write), so recovery is just a load — no second format and
//! no partial state. `/autosave` is a sibling path, so it never alters the
//! user's project file until they choose to save.

use std::path::{Path, PathBuf};

use crate::{BridgeError, FrameForgeProject};

/// The suffix appended to a project path for its autosave sidecar.
pub const AUTOSAVE_SUFFIX: &str = ".autosave";

/// The autosave path belonging to `project` (`name.ffproj` becomes
/// `name.ffproj.autosave`).
pub fn autosave_path(project: &Path) -> PathBuf {
    let mut path = project.as_os_str().to_owned();
    path.push(AUTOSAVE_SUFFIX);
    PathBuf::from(path)
}

/// Write `doc` as the autosave of `project`. Atomic and round-trip checked by
/// [`FrameForgeProject::save`], exactly like a normal save.
pub fn write(doc: &FrameForgeProject, project: &Path) -> Result<PathBuf, BridgeError> {
    let path = autosave_path(project);
    doc.save(&path)?;
    Ok(path)
}

/// Load the autosave of `project`.
pub fn load(project: &Path) -> Result<FrameForgeProject, BridgeError> {
    FrameForgeProject::load(&autosave_path(project))
}

/// The autosave path when it is **newer than the project file**, or when the
/// project file no longer exists. An autosave no newer than the project is
/// stale (the project was saved after it) and is ignored.
pub fn newer_than(project: &Path) -> Option<PathBuf> {
    let autosave = autosave_path(project);
    let autosave_time = std::fs::metadata(&autosave).ok()?.modified().ok()?;
    match std::fs::metadata(project).ok().and_then(|m| m.modified().ok()) {
        Some(project_time) if autosave_time <= project_time => None,
        _ => Some(autosave),
    }
}

/// Remove the autosave of `project`. A missing sidecar is success.
pub fn clear(project: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(autosave_path(project)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
