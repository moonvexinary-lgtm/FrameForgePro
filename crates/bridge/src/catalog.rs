//! The machine-readable effects registry: both engines' catalogs in one shape,
//! plus the cross-engine duplicate audit.
//!
//! The unified Effects Studio needs one searchable catalog whose entries say
//! where an effect comes from, what its parameters are, and what it supports.
//! Both engines already describe themselves in data:
//!
//! * the editing engine: [`filmcraft_project::effect::effect_defs`] — every
//!   video/audio effect and transition, with parameter schemas, GPU and 32-bit
//!   badges;
//! * the compositing engine: [`effectcraft_effects::registry`] — every effect
//!   with its parameters, defaults, GPU and float support.
//!
//! [`catalog`] merges them without losing either's identity (engine + original
//! id + original name), [`name_collisions`] reports the display names both
//! engines share — the duplicate candidates the "one shared implementation"
//! audit works through — and [`catalog_report`] is the JSON-ready form the
//! `frameforge-pro catalog` command prints.
//!
//! What this registry deliberately does *not* claim: it lists what exists in
//! both catalogs. Which effects run through the composed-source bridge is
//! measured, per entry, by the bridge's test suite (`tests/m3_media_catalog.rs`
//! sweeps every catalog entry through the real path and classifies it), not
//! asserted here.

use std::collections::BTreeMap;

use effectcraft_effects as fx;
use effectcraft_keyframe::Value;
use effectcraft_project::ParamUi;
use filmcraft_project::effect::{EffectKind, effect_defs};
use serde::Serialize;

/// One effect, from either engine, in the unified shape. `engine` + `id` is the
/// stable identity; the original display name and category are kept as the
/// engine defines them.
#[derive(Clone, Debug, Serialize)]
pub struct CatalogEntry {
    pub engine: &'static str,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub category: String,
    /// GPU-accelerated badge (the engine's own claim; the bridge measures what
    /// its CPU path does).
    pub gpu: bool,
    pub float32: bool,
    /// Intrinsic fixed effects (Motion, Opacity…) exist on every clip.
    pub intrinsic: bool,
    /// The parameter schema, in the engine's own serialization.
    pub params: Vec<serde_json::Value>,
}

/// A parameter of a compositing-engine effect, in the engine's own shape
/// (its `Value` and `ParamUi` types serialize as-is).
#[derive(Serialize)]
struct EffectCraftParam<'a> {
    id: &'a str,
    name: &'a str,
    default: &'a Value,
    ui: &'a ParamUi,
}

/// The whole catalog, both engines, editing engine first.
pub fn catalog() -> Vec<CatalogEntry> {
    let mut out = Vec::new();
    for d in effect_defs() {
        out.push(CatalogEntry {
            engine: "filmcraft",
            id: d.id.to_string(),
            name: d.name.to_string(),
            kind: match d.kind {
                EffectKind::Video => "video",
                EffectKind::Audio => "audio",
                EffectKind::VideoTransition => "video-transition",
                EffectKind::AudioTransition => "audio-transition",
            }
            .to_string(),
            category: d.category.join(" / "),
            gpu: d.accelerated,
            float32: d.float32,
            intrinsic: d.intrinsic,
            params: d.params.iter().filter_map(|p| serde_json::to_value(p).ok()).collect(),
        });
    }
    for s in fx::registry() {
        out.push(CatalogEntry {
            engine: "effectcraft",
            id: s.id.to_string(),
            name: s.name.to_string(),
            kind: "effect".to_string(),
            category: s.category.to_string(),
            gpu: s.gpu,
            float32: s.float,
            intrinsic: false,
            params: s
                .params
                .iter()
                .filter_map(|p| serde_json::to_value(EffectCraftParam { id: p.id, name: p.name, default: &p.default, ui: &p.ui }).ok())
                .collect(),
        });
    }
    out
}

/// Display names that appear in **both** engines' catalogs: the duplicate
/// candidates the shared-implementation audit works through (one engine's
/// implementation is chosen per name, or the difference is documented).
pub fn name_collisions() -> BTreeMap<String, Vec<(String, String)>> {
    let mut by_name: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for e in catalog() {
        by_name.entry(e.name.to_lowercase()).or_default().push((e.engine.to_string(), e.id));
    }
    by_name.retain(|_, v| v.iter().any(|(en, _)| en == "filmcraft") && v.iter().any(|(en, _)| en == "effectcraft"));
    by_name
}

/// The JSON-ready registry report: the merged catalog and the duplicate audit.
#[derive(Serialize)]
pub struct CatalogReport {
    pub catalog: Vec<CatalogEntry>,
    /// Display name → (engine, effect id) of every engine sharing it.
    pub collisions: BTreeMap<String, Vec<(String, String)>>,
}

pub fn catalog_report() -> CatalogReport {
    CatalogReport { collisions: name_collisions(), catalog: catalog() }
}
