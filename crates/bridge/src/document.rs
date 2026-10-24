//! The FrameForge Pro project document (`.ffproj`).
//!
//! The FilmCraft editing project — the timeline, clips, tracks and every
//! editing feature that project model carries — plus a compositing
//! [`Composition`] for each media item that has one (see [`source`]). The
//! document is plain serde JSON; saving is atomic and checks the text reads
//! back before anything is written.

use std::collections::HashMap;
use std::path::Path;

use effectcraft_project::{ItemId as EcItemId, Project as EcProject};
use filmcraft_media::Generator;
use filmcraft_media::MediaSource;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_project::{
    ItemId, ItemKind, Label, MediaClip, MediaRef, Project,
    keyframe::{Param, ParamValue},
};
use filmcraft_time::Tick;
use serde::{Deserialize, Serialize};

use crate::BridgeError;

/// The schema this build writes. Bumped whenever a saved value changes meaning;
/// files with an older schema still load (new fields carry defaults), newer
/// files get a clear refusal.
///
/// * 1 — the FilmCraft editing project + per-item effect-chain compositions
///   (M2).
/// * 2 — compositions can also hold a full compositing-engine project with
///   layers (M4); `ec_project`/`comp`/`source_item` default to `None`, so
///   schema-1 files read unchanged.
pub const FFPROJ_SCHEMA: u32 = 2;

/// A compositing composition attached to a media item: what "open the clip in a
/// compositing workspace" edits.
///
/// Two things compose here, in order:
///
/// 1. [`effects`](Composition::effects) — the M2 effect chain, applied to the
///    clip's frames with the editing engine's keyframe system (media time);
/// 2. the compositing-engine project ([`ec_project`](Composition::ec_project))
///    — a real composition with layers (text, shapes, solids, nested comps), a
///    footage layer of which ([`source_item`](Composition::source_item)) is
///    backed by this clip's media *after* the effect chain, rendered by the
///    compositing engine's layer compositor.
///
/// When a composition has a comp, the clip's frames are the comp's output (the
/// comp is the picture); without one, the effect chain's output is.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Composition {
    pub effects: Vec<EffectLink>,
    /// A full compositing-engine project whose layers draw over the clip
    /// (present when the clip is opened in a compositing workspace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ec_project: Option<EcProject>,
    /// The composition inside [`ec_project`](Composition::ec_project) that
    /// renders this clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comp: Option<EcItemId>,
    /// The footage item inside [`ec_project`](Composition::ec_project) that
    /// this clip's media backs: the "source layer" of the compositing
    /// workspace. Other footage items of an imported project have no picture
    /// here (they stay offline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_item: Option<EcItemId>,
    /// The cached XMem mask this clip renders with, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roto: Option<crate::roto::RotoRef>,
}

impl Composition {
    /// True when a compositing-engine composition renders this clip.
    pub fn has_comp(&self) -> bool {
        self.comp.is_some() && self.ec_project.is_some()
    }
    /// True when no enabled effect or composition would change a frame (the
    /// source is used as is).
    pub fn is_noop(&self) -> bool {
        !self.effects.iter().any(|e| e.enabled) && !self.has_comp()
    }

    /// Set a scalar parameter at `time` (media time), preserving animation.
    ///
    /// This is the editing engine's own `effects.setParam` semantics: a
    /// parameter that already has keyframes gains or updates a key at `time`
    /// instead of being flattened to a static value. Replacing the whole
    /// [`Param`] here would silently destroy the user's animation, which is the
    /// bug this method exists to prevent.
    ///
    /// Returns false when there is no effect at `effect`.
    pub fn set_scalar(&mut self, effect: usize, param: &str, time: Tick, value: f64) -> bool {
        let Some(link) = self.effects.get_mut(effect) else { return false };
        let entry = link.params.entry(param.to_string()).or_insert_with(|| Param::new(ParamValue::Float(value)));
        if entry.is_animated() {
            entry.put_keyframe(time, ParamValue::Float(value));
        } else {
            entry.value = ParamValue::Float(value);
        }
        true
    }

    /// The comp's layers, as (name, video-switch) rows — read for the shell.
    pub fn comp_layers(&self) -> Vec<(String, bool)> {
        self.ec_project
            .as_ref()
            .zip(self.comp)
            .and_then(|(p, cid)| p.comp(cid))
            .map(|comp| comp.layers.iter().map(|l| (l.name.clone(), l.switches.video)).collect())
            .unwrap_or_default()
    }

    /// Toggle a comp layer's video switch. False when there is no comp or no
    /// layer at `index`.
    pub fn comp_toggle_layer(&mut self, index: usize) -> bool {
        let Some(comp) = self.ec_project.as_mut().zip(self.comp).and_then(|(p, cid)| p.comp_mut(cid)) else { return false };
        if index >= comp.layers.len() {
            return false;
        }
        let layer = &mut comp.layers[index];
        layer.switches.video = !layer.switches.video;
        true
    }

    /// Remove a comp layer. False when there is no comp or no layer at
    /// `index`.
    pub fn comp_remove_layer(&mut self, index: usize) -> bool {
        let Some(comp) = self.ec_project.as_mut().zip(self.comp).and_then(|(p, cid)| p.comp_mut(cid)) else { return false };
        if index >= comp.layers.len() {
            return false;
        }
        comp.layers.remove(index);
        true
    }

    /// Add a text layer to the top of the comp. False when there is no comp.
    pub fn comp_add_text_layer(&mut self, text: &str) -> bool {
        let Some((width, height, duration)) = self.comp_bounds() else { return false };
        // Clone the comp (dropping the immutable project borrow) — build::layer
        // reads it for the layer's defaults while the project is mutated.
        let Some(comp) = self.ec_project.as_ref().zip(self.comp).and_then(|(p, cid)| p.comp(cid)).cloned() else { return false };
        let Some(project) = &mut self.ec_project else { return false };
        let Some(cid) = self.comp else { return false };
        let mut layer = effectcraft_project::build::layer(project, &comp, "Text", effectcraft_project::LayerSource::Text, (width, height), Some(duration));
        if let Some(source_text) = layer.props.prop_mut("text/sourceText") {
            source_text.value =
                effectcraft_keyframe::Value::Text(Box::new(effectcraft_keyframe::TextDoc { text: text.to_string(), size: 48.0, ..Default::default() }));
        }
        let Some(comp) = project.comp_mut(cid) else { return false };
        comp.layers.insert(0, layer);
        true
    }

    /// Add a solid layer to the top of the comp. False when there is no comp.
    pub fn comp_add_solid_layer(&mut self, color: [f32; 3]) -> bool {
        let Some((width, height, duration)) = self.comp_bounds() else { return false };
        let Some(comp) = self.ec_project.as_ref().zip(self.comp).and_then(|(p, cid)| p.comp(cid)).cloned() else { return false };
        let Some(project) = &mut self.ec_project else { return false };
        let Some(cid) = self.comp else { return false };
        let sid = project.add_item(
            "Solid",
            effectcraft_color::Label::Red,
            None,
            effectcraft_project::ItemKind::Solid(effectcraft_project::Solid { color, width, height, pixel_aspect: 1.0 }),
        );
        let layer =
            effectcraft_project::build::layer(project, &comp, "Solid", effectcraft_project::LayerSource::Solid { item: sid }, (width, height), Some(duration));
        let Some(comp) = project.comp_mut(cid) else { return false };
        comp.layers.insert(0, layer);
        true
    }

    fn comp_bounds(&self) -> Option<(u32, u32, effectcraft_time::Tick)> {
        let (width, height, duration) = self.ec_project.as_ref().zip(self.comp).and_then(|(p, cid)| p.comp(cid)).map(|c| (c.width, c.height, c.duration))?;
        Some((width, height, duration))
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

    /// Read a `.ffproj`. Older schemas load (their newer fields default); a file
    /// with a schema this build does not know yet is a clear error, never a
    /// silent misread.
    pub fn load(path: &Path) -> Result<Self, BridgeError> {
        let text = std::fs::read_to_string(path)?;
        let doc: Self = serde_json::from_str(&text).map_err(|e| BridgeError::Format(format!("{}: {e}", path.display())))?;
        if doc.schema > FFPROJ_SCHEMA {
            return Err(BridgeError::Format(format!(
                "{} was written with schema {} but this build reads up to schema {FFPROJ_SCHEMA}",
                path.display(),
                doc.schema
            )));
        }
        Ok(doc)
    }

    /// Import a FilmCraft `.fcproj` (the editing engine's own project format:
    /// the serialized `Project`). The editing project is adopted as-is; the
    /// document starts without compositing compositions, so a FilmCraft project
    /// opens in FrameForge Pro with everything FilmCraft wrote and can then
    /// gain compositions per media item.
    pub fn import_fcproj(path: &Path) -> Result<Self, BridgeError> {
        let text = std::fs::read_to_string(path)?;
        let project: Project = serde_json::from_str(&text).map_err(|e| BridgeError::Format(format!("{}: not a FilmCraft project: {e}", path.display())))?;
        Ok(Self::new(project))
    }

    /// Import an EffectCraft `.ecproj` (the compositing engine's own project
    /// format). Its first composition becomes a composition attached to a new
    /// transparent-video media item: the comp is content with alpha, so on the
    /// timeline it overlays the tracks below it. Footage items inside the
    /// imported project have no backing here and render offline (their frames
    /// are not FilmCraft media); a clip opened in the compositing workspace
    /// gets a proper source layer instead.
    ///
    /// Returns the new editing-engine media item that carries the composition.
    pub fn import_ecproj(&mut self, path: &Path) -> Result<ItemId, BridgeError> {
        let text = std::fs::read_to_string(path)?;
        let ec_project = EcProject::from_json(&text).map_err(|e| BridgeError::Format(format!("{}: not an EffectCraft project: {e}", path.display())))?;
        let Some((comp_id, comp)) = ec_project.items.iter().find_map(|(id, it)| it.as_comp().map(|c| (*id, c.clone()))) else {
            return Err(BridgeError::Format(format!("{}: no composition to import", path.display())));
        };
        let info = GeneratorSource::new(
            Generator::TransparentVideo,
            comp.width,
            comp.height,
            filmcraft_time::FrameRate { num: comp.frame_rate.num, den: comp.frame_rate.den },
            Tick(comp.duration.0),
        )
        .with_name("Imported Composition")
        .info()
        .clone();
        let item = self.project.add_item(
            "Imported Composition",
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::Generator(Generator::TransparentVideo),
                info,
                interpret: Default::default(),
                mark_in: None,
                mark_out: None,
                markers: Vec::new(),
                offline: false,
                proxy: None,
                identity: None,
            }),
            None,
        );
        let composition = self.composition_mut(item);
        composition.ec_project = Some(ec_project);
        composition.comp = Some(comp_id);
        Ok(item)
    }

    /// Point media item `item` at `path` (relink after a file moved). The next
    /// [`ProjectSources`](crate::ProjectSources) build opens the new path; a
    /// file that is still missing keeps the offline slate.
    pub fn relink(&mut self, item: ItemId, path: &str) -> Result<(), BridgeError> {
        let it = self.project.items.get_mut(&item).ok_or_else(|| BridgeError::Format(format!("cannot relink: there is no item {item:?}")))?;
        match &mut it.kind {
            ItemKind::Media(clip) => match &clip.media {
                MediaRef::File { .. } => {
                    clip.media = MediaRef::File { path: path.to_string() };
                    Ok(())
                }
                _ => Err(BridgeError::Format(format!("cannot relink: item {item:?} is not file media"))),
            },
            _ => Err(BridgeError::Format(format!("cannot relink: item {item:?} is not media"))),
        }
    }
}
