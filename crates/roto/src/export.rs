//! Output: binary masks, grayscale masks, alpha mattes and RGBA frames, written
//! as image sequences with a sidecar manifest.
//!
//! Everything here consumes [`Plane`]s that the rest of the pipeline produced, so
//! export works identically for a built-in matte and for a future model's output.
//! The files it writes are what the compositing and effects systems read: a mask
//! sequence for the R1-style alpha path, and RGBA for transparent-background
//! delivery.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::matting::{MatteMode, Plane, apply_alpha};

/// How the files are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameNaming {
    /// `mask_00000012.png` — masks and alpha.
    Mask,
    /// `alpha_00000012.png` — mattes meant to be read as alpha.
    Alpha,
    /// `frame_00000012.png` — RGBA frames.
    Frame,
}

impl FrameNaming {
    pub fn label(self) -> &'static str {
        match self {
            FrameNaming::Mask => "mask_%08d.png",
            FrameNaming::Alpha => "alpha_%08d.png",
            FrameNaming::Frame => "frame_%08d.png",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            FrameNaming::Mask => "mask",
            FrameNaming::Alpha => "alpha",
            FrameNaming::Frame => "frame",
        }
    }
}

/// What to write.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportSpec {
    pub mode: MatteMode,
    pub naming: FrameNaming,
    /// Invert the matte on the way out.
    pub invert: bool,
    /// Multiply RGB by alpha when writing RGBA.
    pub premultiply: bool,
}

impl Default for ExportSpec {
    fn default() -> Self {
        Self { mode: MatteMode::Alpha, naming: FrameNaming::Alpha, invert: false, premultiply: false }
    }
}

impl ExportSpec {
    /// The spec for a transparent-background RGBA sequence.
    pub fn rgba(premultiply: bool) -> Self {
        Self { mode: MatteMode::Alpha, naming: FrameNaming::Frame, invert: false, premultiply }
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("roto export I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("roto export image: {0}")]
    Image(#[from] image::ImageError),
    #[error("roto export: {0}")]
    Invalid(String),
}

/// The manifest written beside a sequence, so a reader knows exactly what it is
/// looking at without guessing from the file names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SequenceManifest {
    pub version: u32,
    pub naming: String,
    pub mode: String,
    pub inverted: bool,
    pub premultiplied: bool,
    pub first_frame: i64,
    pub frame_count: i64,
    pub width: u32,
    pub height: u32,
    /// The cache key the frames came from, when they came from a cache.
    pub cache_key: Option<String>,
    /// The object the sequence belongs to.
    pub object_id: Option<u64>,
}

const MANIFEST_VERSION: u32 = 1;

fn validate(frames: &[(i64, Plane)]) -> Result<(u32, u32), ExportError> {
    let Some((_, first)) = frames.first() else { return Err(ExportError::Invalid("there is nothing to export".to_string())) };
    let (width, height) = (first.width(), first.height());
    if width == 0 || height == 0 {
        return Err(ExportError::Invalid("the frames are empty".to_string()));
    }
    for (frame, plane) in frames {
        if *frame < 0 {
            return Err(ExportError::Invalid(format!("negative frame {frame}")));
        }
        if plane.width() != width || plane.height() != height {
            return Err(ExportError::Invalid(format!("frame {frame} is {}x{} but the sequence is {width}x{height}", plane.width(), plane.height())));
        }
    }
    Ok((width, height))
}

/// Write a mask/alpha sequence and its manifest. Returns the frame count.
pub fn write_mask_sequence(
    directory: &Path,
    frames: &[(i64, Plane)],
    spec: &ExportSpec,
    cache_key: Option<&str>,
    object_id: Option<u64>,
) -> Result<i64, ExportError> {
    let (width, height) = validate(frames)?;
    std::fs::create_dir_all(directory)?;
    for (frame, plane) in frames {
        let plane = if spec.invert { plane.inverted() } else { plane.clone() };
        let plane = match spec.mode {
            MatteMode::Binary => plane.thresholded(128),
            MatteMode::Grayscale | MatteMode::Alpha => plane,
        };
        let image = image::GrayImage::from_raw(plane.width(), plane.height(), plane.data().to_vec())
            .ok_or_else(|| ExportError::Invalid(format!("frame {frame} does not match its size")))?;
        image.save(directory.join(format!("{}_{frame:08}.png", spec.naming.prefix())))?;
    }
    let manifest = SequenceManifest {
        version: MANIFEST_VERSION,
        naming: spec.naming.label().to_string(),
        mode: spec.mode.label().to_string(),
        inverted: spec.invert,
        premultiplied: false,
        first_frame: frames.first().map(|(frame, _)| *frame).unwrap_or(0),
        frame_count: frames.len() as i64,
        width,
        height,
        cache_key: cache_key.map(str::to_string),
        object_id,
    };
    let text = serde_json::to_vec_pretty(&manifest).map_err(|error| ExportError::Invalid(format!("manifest JSON: {error}")))?;
    std::fs::write(directory.join("roto_sequence.json"), text)?;
    Ok(frames.len() as i64)
}

/// Write RGBA frames: `rgb` supplies the picture, the planes supply the alpha.
pub fn write_rgba_sequence(
    directory: &Path,
    frames: &[(i64, Plane)],
    rgb: &[u8],
    spec: &ExportSpec,
    cache_key: Option<&str>,
    object_id: Option<u64>,
) -> Result<i64, ExportError> {
    let (width, height) = validate(frames)?;
    std::fs::create_dir_all(directory)?;
    for (frame, plane) in frames {
        let alpha = if spec.invert { plane.inverted() } else { plane.clone() };
        let rgba = apply_alpha(rgb, &alpha, spec.premultiply)
            .ok_or_else(|| ExportError::Invalid(format!("the rgb buffer does not match {width}x{height} for frame {frame}")))?;
        let image = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| ExportError::Invalid(format!("frame {frame} does not match its size")))?;
        image.save(directory.join(format!("frame_{frame:08}.png")))?;
    }
    let manifest = SequenceManifest {
        version: MANIFEST_VERSION,
        naming: FrameNaming::Frame.label().to_string(),
        mode: MatteMode::Alpha.label().to_string(),
        inverted: spec.invert,
        premultiplied: spec.premultiply,
        first_frame: frames.first().map(|(frame, _)| *frame).unwrap_or(0),
        frame_count: frames.len() as i64,
        width,
        height,
        cache_key: cache_key.map(str::to_string),
        object_id,
    };
    let text = serde_json::to_vec_pretty(&manifest).map_err(|error| ExportError::Invalid(format!("manifest JSON: {error}")))?;
    std::fs::write(directory.join("roto_sequence.json"), text)?;
    Ok(frames.len() as i64)
}

/// Read a written sequence's manifest.
pub fn read_manifest(directory: &Path) -> Result<SequenceManifest, ExportError> {
    let path = directory.join("roto_sequence.json");
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map_err(|error| ExportError::Invalid(format!("{}: {error}", path.display())))
}

/// The files a sequence wrote, ascending by frame.
pub fn written_frames(directory: &Path, naming: FrameNaming) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(naming.prefix()) && name.ends_with(".png")))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("frameforge-roto-export-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn plane(width: u32, height: u32, left: u8, right: u8) -> Plane {
        let mut plane = Plane::new(width, height, 0).expect("plane");
        for y in 0..height {
            for x in 0..width {
                plane.set(x, y, if x < width / 2 { left } else { right });
            }
        }
        plane
    }

    #[test]
    fn an_alpha_sequence_writes_the_frames_it_says_it_did() {
        let root = temp_root("alpha");
        let frames = vec![(4i64, plane(8, 4, 0, 255)), (5i64, plane(8, 4, 255, 255))];
        let written = write_mask_sequence(&root, &frames, &ExportSpec::default(), Some("abc123"), Some(2)).expect("write");
        assert_eq!(written, 2);

        let files = written_frames(&root, FrameNaming::Alpha);
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("alpha_00000004.png"), "{:?}", files[0]);
        assert!(files[1].ends_with("alpha_00000005.png"));

        let manifest = read_manifest(&root).expect("manifest");
        assert_eq!(manifest.frame_count, 2);
        assert_eq!(manifest.first_frame, 4);
        assert_eq!((manifest.width, manifest.height), (8, 4));
        assert_eq!(manifest.cache_key.as_deref(), Some("abc123"));
        assert_eq!(manifest.object_id, Some(2));
        assert!(!manifest.inverted && !manifest.premultiplied);

        let saved = image::open(&files[0]).expect("png").to_luma8();
        assert_eq!(saved.get_pixel(1, 1).0[0], 0);
        assert_eq!(saved.get_pixel(6, 1).0[0], 255);
    }

    #[test]
    fn binary_and_inverted_output_are_what_they_claim() {
        let root = temp_root("binary");
        let frames = vec![(0i64, plane(8, 4, 100, 200))];
        let spec = ExportSpec { mode: MatteMode::Binary, naming: FrameNaming::Mask, invert: true, premultiply: false };
        write_mask_sequence(&root, &frames, &spec, None, None).expect("write");
        let file = written_frames(&root, FrameNaming::Mask).into_iter().next().expect("a file");
        let saved = image::open(&file).expect("png").to_luma8();
        // Inversion runs first (100 -> 155, 200 -> 55), then the binary mode
        // thresholds at 128, so the left half is covered and the right is not.
        assert_eq!(saved.get_pixel(1, 1).0[0], 255);
        assert_eq!(saved.get_pixel(6, 1).0[0], 0);
        let manifest = read_manifest(&root).expect("manifest");
        assert!(manifest.inverted);
        assert_eq!(manifest.mode, MatteMode::Binary.label());
    }

    #[test]
    fn rgba_output_applies_the_matte_to_the_picture() {
        let root = temp_root("rgba");
        let frames = vec![(0i64, plane(4, 2, 0, 255))];
        let rgb: Vec<u8> = (0..8).flat_map(|_| [10u8, 20, 30]).collect();
        let spec = ExportSpec::rgba(false);
        write_rgba_sequence(&root, &frames, &rgb, &spec, None, Some(1)).expect("write");

        let file = written_frames(&root, FrameNaming::Frame).into_iter().next().expect("a file");
        let saved = image::open(&file).expect("png").to_rgba8();
        assert_eq!(saved.get_pixel(0, 0).0, [10, 20, 30, 0], "the transparent half keeps its colour with alpha 0");
        assert_eq!(saved.get_pixel(3, 0).0, [10, 20, 30, 255]);

        let premultiplied = ExportSpec::rgba(true);
        let root2 = temp_root("rgba-premultiplied");
        write_rgba_sequence(&root2, &frames, &rgb, &premultiplied, None, None).expect("write");
        let file = written_frames(&root2, FrameNaming::Frame).into_iter().next().expect("a file");
        let saved = image::open(&file).expect("png").to_rgba8();
        assert_eq!(saved.get_pixel(0, 0).0, [0, 0, 0, 0], "premultiplied transparent pixels carry no colour");
        assert!(read_manifest(&root2).expect("manifest").premultiplied);
    }

    #[test]
    fn export_refuses_an_empty_or_mismatched_sequence() {
        let root = temp_root("invalid");
        assert!(write_mask_sequence(&root, &[], &ExportSpec::default(), None, None).is_err());
        let frames = vec![(0i64, plane(8, 4, 0, 255)), (1i64, plane(4, 2, 0, 255))];
        let error = write_mask_sequence(&root, &frames, &ExportSpec::default(), None, None).expect_err("a mismatched frame is refused");
        assert!(error.to_string().contains("4x2"), "{error}");
        assert!(
            write_rgba_sequence(&root, &frames[..1], &[0u8; 3], &ExportSpec::rgba(false), None, None).is_err(),
            "an rgb buffer of the wrong length is refused"
        );
    }
}
