//! Cached roto masks: the bytes preview and export both read.
//!
//! The cache is a directory of 8-bit grayscale PNGs plus a small manifest, keyed
//! by every input that can change the masks (source identity, model hash,
//! working size, frame rate, correction ids and the XMem commit). Changing an
//! input addresses a different directory, so stale alpha can never be served
//! under a new edit.
//!
//! R1 only reads the cache. The R2 worker writes this exact layout; the Rust
//! renderer never runs PyTorch.

use std::path::{Path, PathBuf};

use filmcraft_time::{FrameRate, Tick};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The upstream XMem commit the cache key names.
pub const XMEM_COMMIT: &str = "f3b841d50df058910bbf690229ddc15fb1aef7d6";

/// The cache layout this build reads.
const MASK_CACHE_VERSION: u32 = 1;

/// Upper bound on one mask plane (100 megapixels). A corrupt manifest cannot
/// make the renderer allocate an unbounded image.
const MAX_MASK_PIXELS: u64 = 100_000_000;

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
    /// A stable 64-bit FNV-1a digest, rendered as 16 lowercase hex digits.
    ///
    /// This is a cache address, not a security hash: it only has to be stable
    /// across runs and change when an input changes.
    pub fn cache_key(&self) -> String {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        feed(&mut hash, b"frameforge-roto-cache-v1");
        feed_str(&mut hash, &self.source_identity);
        feed_str(&mut hash, &self.model_sha256);
        feed_u64(&mut hash, u64::from(self.width));
        feed_u64(&mut hash, u64::from(self.height));
        feed_u64(&mut hash, self.frame_rate.num as u64);
        feed_u64(&mut hash, self.frame_rate.den as u64);
        feed_u64(&mut hash, self.correction_ids.len() as u64);
        for id in &self.correction_ids {
            feed_u64(&mut hash, *id);
        }
        feed_str(&mut hash, &self.xmem_commit);
        format!("{hash:016x}")
    }
}

fn feed(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn feed_u64(hash: &mut u64, value: u64) {
    feed(hash, &value.to_le_bytes());
}

fn feed_str(hash: &mut u64, value: &str) {
    feed_u64(hash, value.len() as u64);
    feed(hash, value.as_bytes());
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

/// The manifest the worker writes beside the mask PNGs.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct MaskManifest {
    version: u32,
    key: String,
    width: u32,
    height: u32,
    frame_rate: FrameRate,
    frame_start: i64,
    frame_count: i64,
}

/// What can go wrong while reading a mask cache.
#[derive(Debug, Error)]
pub enum MaskError {
    #[error("roto mask cache is not available: {0}")]
    Missing(String),
    #[error("roto mask cache I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("roto mask cache format: {0}")]
    Format(String),
    #[error("roto mask frame {frame} cannot be read: {source}")]
    Image {
        frame: i64,
        #[source]
        source: image::ImageError,
    },
}

/// Upper bound on the frames one adoption may publish, so a bad manifest cannot
/// make the writer loop for hours.
const MAX_MASK_FRAMES: i64 = 100_000;

/// A worker's output, as the cache should record it. `output_dir` holds
/// `frame_<index>.png` at the working size — the same names and numbering
/// [`crate::roto_worker::export_frames`] writes for the worker's *input*.
#[derive(Clone, Copy, Debug)]
pub struct MaskAdoption<'a> {
    pub output_dir: &'a Path,
    pub first_frame: i64,
    pub frame_count: i64,
    pub frame_rate: FrameRate,
    pub width: u32,
    pub height: u32,
}

/// A readable mask cache: a validated manifest and the directory it names.
#[derive(Debug)]
pub struct MaskCache {
    dir: PathBuf,
    manifest: MaskManifest,
}

impl MaskCache {
    /// Open the cache `root/<key>/`. The key is validated before it touches the
    /// filesystem, so a hand-edited project cannot escape the cache root.
    pub fn open(root: &Path, key: &str) -> Result<Self, MaskError> {
        if key.len() != 16 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(MaskError::Format(format!("'{key}' is not a 16-digit roto cache key")));
        }
        let dir = root.join(key);
        let manifest_path = dir.join("manifest.json");
        let text = match std::fs::read_to_string(&manifest_path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(MaskError::Missing(format!("{}", dir.display())));
            }
            Err(error) => return Err(error.into()),
        };
        let manifest: MaskManifest = serde_json::from_str(&text).map_err(|error| MaskError::Format(format!("{}: {error}", manifest_path.display())))?;
        if manifest.version != MASK_CACHE_VERSION {
            return Err(MaskError::Format(format!(
                "{} is cache version {} but this build reads version {MASK_CACHE_VERSION}",
                manifest_path.display(),
                manifest.version
            )));
        }
        if manifest.key != key {
            return Err(MaskError::Format(format!("{} names cache key '{}' but lives in '{key}'", manifest_path.display(), manifest.key)));
        }
        if manifest.width == 0 || manifest.height == 0 {
            return Err(MaskError::Format(format!("{} declares an empty mask", manifest_path.display())));
        }
        let pixels = u64::from(manifest.width) * u64::from(manifest.height);
        if pixels > MAX_MASK_PIXELS {
            return Err(MaskError::Format(format!(
                "{} declares a {}x{} mask, larger than the {MAX_MASK_PIXELS}-pixel limit",
                manifest_path.display(),
                manifest.width,
                manifest.height
            )));
        }
        if manifest.frame_start < 0 || manifest.frame_count < 0 {
            return Err(MaskError::Format(format!("{} declares a negative frame range", manifest_path.display())));
        }
        if manifest.frame_rate.num <= 0 || manifest.frame_rate.den <= 0 {
            return Err(MaskError::Format(format!("{} declares an invalid frame rate", manifest_path.display())));
        }
        Ok(Self { dir, manifest })
    }

    /// Adopt a worker's masks into `root/<key>/` — the write side of this seam.
    ///
    /// Every PNG the manifest will promise is decoded and size-checked **before**
    /// anything is published, so a half-finished worker run can never be read as
    /// a cache. The masks are copied first and the manifest is written last, to a
    /// temporary file that is renamed into place, so [`MaskCache::open`] sees
    /// either no manifest ([`MaskError::Missing`]) or a complete, consistent one.
    /// Returns the number of frames published.
    ///
    /// The token `key` is the same [`RotoCacheKey`] a [`RotoRef`] stores, so
    /// changing any input (source, model hash, size, rate, corrections, commit)
    /// addresses a different directory and stale alpha can never be served.
    pub fn adopt(root: &Path, key: &RotoCacheKey, adoption: &MaskAdoption<'_>) -> Result<i64, MaskError> {
        let cache_key = key.cache_key();
        let MaskAdoption { output_dir, first_frame, frame_count, frame_rate, width, height } = *adoption;
        if first_frame < 0 {
            return Err(MaskError::Format(format!("adoption starts at a negative frame {first_frame}")));
        }
        if !(0..=MAX_MASK_FRAMES).contains(&frame_count) {
            return Err(MaskError::Format(format!("adoption declares {frame_count} frames, outside 0..={MAX_MASK_FRAMES}")));
        }
        if width == 0 || height == 0 {
            return Err(MaskError::Format(format!("adoption declares an empty mask {width}x{height}")));
        }
        if u64::from(width) * u64::from(height) > MAX_MASK_PIXELS {
            return Err(MaskError::Format(format!("adoption declares a {width}x{height} mask, larger than the {MAX_MASK_PIXELS}-pixel limit")));
        }
        if frame_rate.num <= 0 || frame_rate.den <= 0 {
            return Err(MaskError::Format("adoption declares an invalid frame rate".to_string()));
        }

        let dir = root.join(&cache_key);
        std::fs::create_dir_all(&dir)?;

        // Pass 1: the whole promised range exists and matches the working size.
        for index in 0..frame_count {
            read_worker_mask(output_dir, first_frame, index, width, height)?;
        }
        // Pass 2: publish the masks, one plane at a time (the writer never holds
        // the whole run in memory, and neither does the reader).
        for index in 0..frame_count {
            let frame = first_frame.checked_add(index).ok_or_else(|| MaskError::Format("adoption frame index overflows".to_string()))?;
            let gray = read_worker_mask(output_dir, first_frame, index, width, height)?;
            gray.save(dir.join(format!("frame_{frame:08}.png"))).map_err(|source| MaskError::Image { frame, source })?;
        }

        let manifest = MaskManifest { version: MASK_CACHE_VERSION, key: cache_key, width, height, frame_rate, frame_start: first_frame, frame_count };
        let text = serde_json::to_vec_pretty(&manifest).map_err(|error| MaskError::Format(format!("mask manifest JSON: {error}")))?;
        let temp = dir.join("manifest.json.tmp");
        std::fs::write(&temp, text)?;
        let published = dir.join("manifest.json");
        if published.exists() {
            std::fs::remove_file(&published)?;
        }
        std::fs::rename(&temp, &published)?;
        Ok(frame_count)
    }

    pub fn key(&self) -> &str {
        &self.manifest.key
    }

    pub fn frame_rate(&self) -> FrameRate {
        self.manifest.frame_rate
    }

    pub fn frame_size(&self) -> (u32, u32) {
        (self.manifest.width, self.manifest.height)
    }

    /// The mask covering `t`, or `None` when `t` lies outside the propagated
    /// range. A frame *inside* the declared range whose PNG is missing is an
    /// error: rendering it unmasked would silently export the wrong picture.
    pub fn frame(&self, t: Tick) -> Result<Option<MaskFrame>, MaskError> {
        let end =
            self.manifest.frame_start.checked_add(self.manifest.frame_count).ok_or_else(|| MaskError::Format("roto mask frame range overflows".to_string()))?;
        let frame = self.manifest.frame_rate.frame_at(t);
        if frame < self.manifest.frame_start || frame >= end {
            return Ok(None);
        }
        let path = self.dir.join(format!("frame_{frame:08}.png"));
        let image = image::open(&path).map_err(|source| MaskError::Image { frame, source })?;
        let gray = image.to_luma8();
        if gray.width() != self.manifest.width || gray.height() != self.manifest.height {
            return Err(MaskError::Format(format!(
                "{} is {}x{} but the manifest declares {}x{}",
                path.display(),
                gray.width(),
                gray.height(),
                self.manifest.width,
                self.manifest.height
            )));
        }
        Ok(Some(MaskFrame { width: gray.width(), height: gray.height(), alpha: gray.into_raw() }))
    }
}

/// Read one mask the worker wrote, decoded to 8-bit gray and checked against the
/// declared working size. `index` is relative to the adopted range.
fn read_worker_mask(output_dir: &Path, first_frame: i64, index: i64, width: u32, height: u32) -> Result<image::GrayImage, MaskError> {
    let frame = first_frame.checked_add(index).ok_or_else(|| MaskError::Format("worker mask frame index overflows".to_string()))?;
    let path = output_dir.join(format!("frame_{frame:08}.png"));
    let image = image::open(&path).map_err(|source| MaskError::Image { frame, source })?;
    let gray = image.to_luma8();
    if gray.width() != width || gray.height() != height {
        return Err(MaskError::Format(format!("{} is {}x{} but the working size is {width}x{height}", path.display(), gray.width(), gray.height())));
    }
    Ok(gray)
}

/// One decoded alpha plane at the worker's working resolution.
#[derive(Clone, Debug)]
pub struct MaskFrame {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
}

impl MaskFrame {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }
}

/// Where the cache lives when the host has no override.
pub fn default_cache_root() -> PathBuf {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("FrameForgePro").join("roto");
    }
    if let Some(base) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(base).join("FrameForgePro").join("roto");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("FrameForgePro").join("roto");
    }
    std::env::temp_dir().join("FrameForgePro").join("roto")
}
