//! Cached roto masks: the bytes preview and export both read.

use std::path::{Path, PathBuf};

use filmcraft_time::{FrameRate, Tick};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The upstream XMem commit the cache key names.
pub const XMEM_COMMIT: &str = "f3b841d50df058910bbf690229ddc15fb1aef7d6";

/// The inputs that make one mask cache unique.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotoCacheKey {
    pub source_identity: String,
    pub model_sha256: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    pub correction_ids: Vec<u64>,
    pub xmem_commit: String,
}

impl RotoCacheKey {
    pub fn cache_key(&self) -> String {
        String::new()
    }
}

/// A roto mask reference stored in a composition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotoRef {
    pub key: String,
}

impl RotoRef {
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

/// What can go wrong while reading a mask cache.
#[derive(Clone, Debug, Error)]
pub enum MaskError {
    #[error("roto mask cache is not available: {0}")]
    Unavailable(String),
}

/// A readable mask cache.
#[derive(Clone, Debug)]
pub struct MaskCache;

impl MaskCache {
    pub fn open(_root: &Path, _key: &str) -> Result<Self, MaskError> {
        Err(MaskError::Unavailable("not implemented".to_string()))
    }

    pub fn frame(&self, _t: Tick) -> Result<Option<MaskFrame>, MaskError> {
        Err(MaskError::Unavailable("not implemented".to_string()))
    }
}

/// One decoded alpha plane.
#[derive(Clone, Debug)]
pub struct MaskFrame {
    alpha: Vec<u8>,
}

impl MaskFrame {
    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }
}

/// Where the cache lives when the host has no override.
pub fn default_cache_root() -> PathBuf {
    std::env::temp_dir().join("FrameForgePro").join("roto")
}
