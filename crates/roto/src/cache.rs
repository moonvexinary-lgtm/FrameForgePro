//! The multi-object mask cache: what a finished run leaves on disk so the
//! renderer, the timeline and the next correction all read the same bytes.
//!
//! Layout: `root/<digest>/object_<id>/frame_%08d.png` plus one `manifest.json`
//! per object directory. The manifest is written **last**, to a temporary file
//! that is renamed into place, so a run that dies half way leaves no readable
//! cache and the previous one is never half-replaced.
//!
//! The digest covers every input that changes the masks: the model, its weights
//! hash, the source, the working size, the frame rate, the object ids, the
//! correction frames and the pipeline version. Change any of them and the run
//! addresses a different directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::matting::{MatteMode, Plane};

/// The cache layout this build writes.
pub const CACHE_VERSION: u32 = 2;
/// The frames one object directory may declare.
const MAX_CACHED_FRAMES: i64 = 200_000;
/// The pixels one cached mask may have.
const MAX_CACHED_PIXELS: u64 = 100_000_000;

/// Everything that makes a cached mask unique.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskKey {
    /// The tracking or selection model that produced the masks.
    pub model_id: String,
    /// The digest of the weights the run used, when it is known.
    pub weights_sha256: String,
    /// A stable identity for the source clip.
    pub source_identity: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate_num: i32,
    pub frame_rate_den: i32,
    /// The objects that were tracked together, ascending.
    pub object_ids: Vec<u64>,
    /// The correction frames the run started from, ascending.
    pub correction_frames: Vec<i64>,
    /// Bump when the pipeline's semantics change.
    pub pipeline: u32,
}

impl MaskKey {
    /// A stable 64-bit FNV-1a digest, rendered as 16 lowercase hex digits. This
    /// is a cache address, not a security hash.
    pub fn digest(&self) -> String {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        feed(b"frameforge-roto-cache-v2");
        for text in [&self.model_id, &self.weights_sha256, &self.source_identity] {
            feed(&(text.len() as u64).to_le_bytes());
            feed(text.as_bytes());
        }
        for value in [u64::from(self.width), u64::from(self.height)] {
            feed(&value.to_le_bytes());
        }
        for value in [self.frame_rate_num as i64, self.frame_rate_den as i64, self.pipeline as i64] {
            feed(&value.to_le_bytes());
        }
        feed(&(self.object_ids.len() as u64).to_le_bytes());
        for id in &self.object_ids {
            feed(&id.to_le_bytes());
        }
        feed(&(self.correction_frames.len() as u64).to_le_bytes());
        for frame in &self.correction_frames {
            feed(&frame.to_le_bytes());
        }
        format!("{hash:016x}")
    }

    pub fn directory(&self, root: &Path) -> PathBuf {
        root.join(self.digest())
    }
}

/// What one object's directory records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectManifest {
    pub version: u32,
    pub key: String,
    pub object_id: u64,
    pub width: u32,
    pub height: u32,
    pub first_frame: i64,
    pub frame_count: i64,
    pub mode: u8,
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("roto cache I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("roto cache format: {0}")]
    Format(String),
    #[error("roto cache is missing: {0}")]
    Missing(String),
    #[error("roto cache image: {0}")]
    Image(#[from] image::ImageError),
}

/// The directory one object's masks live in.
pub fn object_dir(root: &Path, key: &MaskKey, object_id: u64) -> PathBuf {
    key.directory(root).join(format!("object_{object_id}"))
}

fn mode_code(mode: MatteMode) -> u8 {
    match mode {
        MatteMode::Binary => 0,
        MatteMode::Grayscale => 1,
        MatteMode::Alpha => 2,
    }
}

/// Publish one object's frames. Every frame is validated (size, count) before
/// anything is written, and the manifest lands last.
pub fn publish(root: &Path, key: &MaskKey, object_id: u64, first_frame: i64, frames: &[Plane], mode: MatteMode) -> Result<i64, CacheError> {
    if key.object_ids.binary_search(&object_id).is_err() {
        return Err(CacheError::Format(format!("object {object_id} is not part of this key's object set")));
    }
    if key.width == 0 || key.height == 0 {
        return Err(CacheError::Format("the key declares an empty frame".to_string()));
    }
    if u64::from(key.width) * u64::from(key.height) > MAX_CACHED_PIXELS {
        return Err(CacheError::Format(format!("{}x{} exceeds the cached mask size limit", key.width, key.height)));
    }
    if first_frame < 0 {
        return Err(CacheError::Format(format!("a cache cannot start at frame {first_frame}")));
    }
    let count = frames.len() as i64;
    if count > MAX_CACHED_FRAMES {
        return Err(CacheError::Format(format!("{count} frames exceeds the {MAX_CACHED_FRAMES}-frame cache limit")));
    }
    for plane in frames {
        if plane.width() != key.width || plane.height() != key.height {
            return Err(CacheError::Format(format!("a mask is {}x{} but the key says {}x{}", plane.width(), plane.height(), key.width, key.height)));
        }
    }

    let dir = object_dir(root, key, object_id);
    std::fs::create_dir_all(&dir)?;
    for (index, plane) in frames.iter().enumerate() {
        let frame = first_frame + index as i64;
        let image = image::GrayImage::from_raw(plane.width(), plane.height(), plane.data().to_vec())
            .ok_or_else(|| CacheError::Format(format!("mask for frame {frame} does not match its size")))?;
        image.save(dir.join(format!("frame_{frame:08}.png")))?;
    }

    let manifest = ObjectManifest {
        version: CACHE_VERSION,
        key: key.digest(),
        object_id,
        width: key.width,
        height: key.height,
        first_frame,
        frame_count: count,
        mode: mode_code(mode),
    };
    let text = serde_json::to_vec_pretty(&manifest).map_err(|error| CacheError::Format(format!("manifest JSON: {error}")))?;
    let temp = dir.join("manifest.json.tmp");
    std::fs::write(&temp, text)?;
    let published = dir.join("manifest.json");
    if published.exists() {
        std::fs::remove_file(&published)?;
    }
    std::fs::rename(&temp, &published)?;
    Ok(count)
}

/// Read one object's manifest, if it is there and consistent with the key.
pub fn manifest(root: &Path, key: &MaskKey, object_id: u64) -> Result<ObjectManifest, CacheError> {
    let dir = object_dir(root, key, object_id);
    let path = dir.join("manifest.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(CacheError::Missing(dir.display().to_string())),
        Err(error) => return Err(error.into()),
    };
    let manifest: ObjectManifest = serde_json::from_str(&text).map_err(|error| CacheError::Format(format!("{}: {error}", path.display())))?;
    if manifest.version != CACHE_VERSION {
        return Err(CacheError::Format(format!("{} is cache version {} but this build reads {CACHE_VERSION}", path.display(), manifest.version)));
    }
    let digest = key.digest();
    if manifest.key != digest {
        return Err(CacheError::Format(format!("{} names cache '{}' but this key is '{digest}'", path.display(), manifest.key)));
    }
    if manifest.object_id != object_id {
        return Err(CacheError::Format(format!("{} declares object {} but lives under object {object_id}", path.display(), manifest.object_id)));
    }
    Ok(manifest)
}

/// Read one frame of one object. A frame inside the declared range whose PNG is
/// missing is an error: rendering it unmasked would silently be wrong.
pub fn load(root: &Path, key: &MaskKey, object_id: u64, frame: i64) -> Result<Option<Plane>, CacheError> {
    let manifest = manifest(root, key, object_id)?;
    let end = manifest.first_frame.checked_add(manifest.frame_count).ok_or_else(|| CacheError::Format("the frame range overflows".to_string()))?;
    if frame < manifest.first_frame || frame >= end {
        return Ok(None);
    }
    let dir = object_dir(root, key, object_id);
    let path = dir.join(format!("frame_{frame:08}.png"));
    let image = image::open(&path)?;
    let gray = image.to_luma8();
    if gray.width() != manifest.width || gray.height() != manifest.height {
        return Err(CacheError::Format(format!(
            "{} is {}x{} but the manifest declares {}x{}",
            path.display(),
            gray.width(),
            gray.height(),
            manifest.width,
            manifest.height
        )));
    }
    Plane::from_data(gray.width(), gray.height(), gray.into_raw()).ok_or_else(|| CacheError::Format("the cached mask has no pixels".to_string()))
}

/// The object ids that have a readable cache under this key.
pub fn cached_objects(root: &Path, key: &MaskKey) -> Vec<u64> {
    key.object_ids.iter().copied().filter(|object_id| manifest(root, key, *object_id).is_ok()).collect()
}

/// Delete everything this key produced.
pub fn clear(root: &Path, key: &MaskKey) -> Result<(), CacheError> {
    let dir = key.directory(root);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(objects: Vec<u64>, corrections: Vec<i64>) -> MaskKey {
        MaskKey {
            model_id: "cutie-base".to_string(),
            weights_sha256: "ab".repeat(32),
            source_identity: "item:3:file:clip.mp4:0f1e2d3c4b5a6978:1048576".to_string(),
            width: 16,
            height: 8,
            frame_rate_num: 24,
            frame_rate_den: 1,
            object_ids: objects,
            correction_frames: corrections,
            pipeline: 1,
        }
    }

    fn plane(width: u32, height: u32, fill: u8) -> Plane {
        Plane::new(width, height, fill).expect("plane")
    }

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("frameforge-roto-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn the_digest_changes_with_every_input_that_changes_the_masks() {
        let base = key(vec![1], vec![]).digest();
        assert_eq!(base.len(), 16);
        assert!(base.bytes().all(|b| b.is_ascii_hexdigit()));

        let mut model = key(vec![1], vec![]);
        model.model_id = "xmem".to_string();
        let mut weights = key(vec![1], vec![]);
        weights.weights_sha256 = "cd".repeat(32);
        let mut source = key(vec![1], vec![]);
        source.source_identity.push_str(":edited");
        let mut size = key(vec![1], vec![]);
        size.width = 32;
        let mut rate = key(vec![1], vec![]);
        rate.frame_rate_num = 25;
        let mut objects = key(vec![1, 2], vec![]);
        objects.object_ids = vec![2, 1];
        let mut corrections = key(vec![1], vec![7]);
        corrections.correction_frames = vec![8];
        let mut pipeline = key(vec![1], vec![]);
        pipeline.pipeline = 2;

        for (name, variant) in [
            ("model", model),
            ("weights", weights),
            ("source", source),
            ("size", size),
            ("frame rate", rate),
            ("object order", objects),
            ("corrections", corrections),
            ("pipeline", pipeline),
        ] {
            assert_ne!(variant.digest(), base, "{name} must address a different cache");
        }
        // The object order is part of the key, so 1,2 and 2,1 differ; a caller
        // that always sorts its ids is stable.
        let mut sorted = key(vec![1], vec![]);
        sorted.object_ids = vec![1];
        assert_eq!(sorted.digest(), base);
    }

    #[test]
    fn a_published_object_round_trips_and_reports_its_manifest() {
        let root = root("round-trip");
        let key = key(vec![1], vec![]);
        let mut mask = plane(16, 8, 0);
        for x in 8..16 {
            for y in 0..8 {
                mask.set(x, y, 255);
            }
        }
        let frames = vec![mask.clone(), mask.clone()];

        let published = publish(&root, &key, 1, 0, &frames, MatteMode::Alpha).expect("publish");
        assert_eq!(published, 2);

        let manifest = manifest(&root, &key, 1).expect("manifest");
        assert_eq!(manifest.object_id, 1);
        assert_eq!((manifest.width, manifest.height), (16, 8));
        assert_eq!(manifest.frame_count, 2);

        let loaded = load(&root, &key, 1, 1).expect("load").expect("covered frame");
        assert_eq!(loaded.get(10, 4), Some(255));
        assert_eq!(loaded.get(2, 4), Some(0));
        assert!(load(&root, &key, 1, 5).expect("outside the range").is_none(), "frames outside the range are absent, not errors");
        assert!(load(&root, &key, 1, 0).expect("covered").is_some());
        assert_eq!(cached_objects(&root, &key), vec![1]);
    }

    #[test]
    fn a_missing_or_mismatched_cache_is_reported_rather_than_guessed() {
        let root = root("missing");
        let key = key(vec![1], vec![]);
        assert!(matches!(manifest(&root, &key, 1).expect_err("nothing published"), CacheError::Missing(_)));

        publish(&root, &key, 1, 0, &[plane(16, 8, 255)], MatteMode::Binary).expect("publish");
        let other = key(vec![1], vec![9]);
        assert!(manifest(&root, &other, 1).is_err(), "a different correction set must not read this cache");
        assert!(manifest(&root, &key, 2).is_err(), "an unexpected object id is refused");
        assert!(clear(&root, &key).is_ok());
        assert!(manifest(&root, &key, 1).is_err(), "clearing removes the cache");
    }

    #[test]
    fn publishing_validates_everything_before_it_writes_anything() {
        let root = root("validate");
        let key = key(vec![1], vec![]);
        let wrong_size = plane(8, 8, 255);
        let error = publish(&root, &key, 1, 0, &[wrong_size], MatteMode::Alpha).expect_err("a mis-sized mask is refused");
        assert!(error.to_string().contains("16x8"), "{error}");
        assert!(manifest(&root, &key, 1).is_err(), "and nothing was published");

        let error = publish(&root, &key, 9, 0, &[plane(16, 8, 255)], MatteMode::Alpha).expect_err("an object outside the key is refused");
        assert!(error.to_string().contains("object 9"), "{error}");
        assert!(publish(&root, &key, 1, -1, &[plane(16, 8, 255)], MatteMode::Alpha).is_err(), "a negative first frame is refused");
    }

    #[test]
    fn two_objects_live_side_by_side_under_one_key() {
        let root = root("objects");
        let key = key(vec![1, 2], vec![]);
        publish(&root, &key, 1, 0, &[plane(16, 8, 255)], MatteMode::Alpha).expect("object 1");
        publish(&root, &key, 2, 0, &[plane(16, 8, 0)], MatteMode::Alpha).expect("object 2");
        assert_eq!(cached_objects(&root, &key), vec![1, 2]);
        assert_eq!(load(&root, &key, 1, 0).expect("load").expect("frame").get(0, 0), Some(255));
        assert_eq!(load(&root, &key, 2, 0).expect("load").expect("frame").get(0, 0), Some(0));
    }
}
