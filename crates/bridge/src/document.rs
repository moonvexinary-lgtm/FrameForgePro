//! The FrameForge Pro project document (`.ffproj`).
//!
//! v1 is the FilmCraft editing project — the timeline, clips, tracks and every
//! editing feature that project model carries — plus a compositing
//! [`Composition`] for each media item that has one (see [`source`]). The
//! document is plain serde JSON; saving is atomic and checks the text reads
//! back before anything is written.

use std::collections::HashMap;
use std::path::Path;

use filmcraft_project::{ItemId, Project, keyframe::Param};
use serde::{Deserialize, Serialize};

use crate::BridgeError;

/// The schema this build writes. Bumped whenever a saved value changes meaning;
/// older files get a migration path, newer files a clear refusal.
pub const FFPROJ_SCHEMA: u32 = 1;

/// A compositing composition attached to a media item: the frames of that item
/// pass through this effect chain (in order) before the timeline reads them.
///
/// This is the first-class "open the clip in a compositing workspace" object of
/// the unified application: the composition belongs to the media item, the way
/// a composition belongs to a footage item in the compositing engine, while
/// per-clip editing effects stay where the editing engine keeps them.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Composition {
    pub effects: Vec<EffectLink>,
}

impl Composition {
    /// True when no enabled effect would change a frame (the source is used as is).
    pub fn is_noop(&self) -> bool {
        self.effects.iter().all(|e| !e.enabled)
    }
}

/// One EffectCraft effect inside a [`Composition`].
///
/// `effect` is the registry id (for example `"ec.blur.gaussian"`). Parameters
/// use the editing engine's keyframe system ([`Param`]): static values or
/// keyframes in **media time** (exact ticks), so a composition survives trims
/// and edits of the timeline clips that play it, and parameter evaluation is a
/// pure function of the media time the renderer asks for.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EffectLink {
    pub effect: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub params: HashMap<String, Param>,
}

fn default_true() -> bool {
    true
}

/// The FrameForge Pro project: the editing engine's document plus the
/// compositing compositions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameForgeProject {
    pub schema: u32,
    pub project: Project,
    #[serde(default)]
    pub compositions: HashMap<ItemId, Composition>,
}

impl FrameForgeProject {
    pub fn new(project: Project) -> Self {
        Self { schema: FFPROJ_SCHEMA, project, compositions: HashMap::new() }
    }

    /// The composition of `item`, created empty on first use.
    pub fn composition_mut(&mut self, item: ItemId) -> &mut Composition {
        self.compositions.entry(item).or_default()
    }

    /// Write the document to `path` (`.ffproj`), atomically: the text is
    /// serialized and read back first, then written to a sibling temporary file
    /// and renamed over the target, so a crash mid-save never destroys the
    /// previous file.
    pub fn save(&self, path: &Path) -> Result<(), BridgeError> {
        let text = serde_json::to_string_pretty(self)?;
        // JSON cannot hold NaN or infinity: prove the text reads back before
        // anything is written, so a value that cannot round-trip is reported
        // here instead of corrupting the file.
        serde_json::from_str::<Self>(&text)?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp);
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Read a `.ffproj`. A file with a schema this build does not know is a
    /// clear error, never a silent misread.
    pub fn load(path: &Path) -> Result<Self, BridgeError> {
        let text = std::fs::read_to_string(path)?;
        let doc: Self = serde_json::from_str(&text).map_err(|e| BridgeError::Format(format!("{}: {e}", path.display())))?;
        if doc.schema != FFPROJ_SCHEMA {
            return Err(BridgeError::Format(format!(
                "{} was written with schema {} but this build reads and writes schema {FFPROJ_SCHEMA}",
                path.display(),
                doc.schema
            )));
        }
        Ok(doc)
    }
}
