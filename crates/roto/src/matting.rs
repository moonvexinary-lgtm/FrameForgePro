//! Matting and alpha: the deterministic edge work that turns a hard mask into a
//! usable matte, plus the seam where a learned matting model would plug in.
//!
//! Everything implemented here is FrameForge's own arithmetic — feather,
//! expand/contract, edge smoothing, inversion, combination and premultiply — so
//! a professional-looking matte exists with **no model, no weights and no
//! runtime**. The learned matting backends in the catalogue (RVM, MatAnyone) are
//! declared but disabled: RVM is GPL-3.0 and MatAnyone is non-commercial, so
//! neither may be bundled or linked, and both are `Redistribution::Prohibited`.

use serde::{Deserialize, Serialize};

/// A single-channel 8-bit plane: masks, alpha and coverage all use this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plane {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Plane {
    pub fn new(width: u32, height: u32, fill: u8) -> Option<Self> {
        let len = (width as usize).checked_mul(height as usize)?;
        if width == 0 || height == 0 {
            return None;
        }
        Some(Self { width, height, data: vec![fill; len] })
    }

    pub fn from_data(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 || data.len() != (width as usize) * (height as usize) {
            return None;
        }
        Some(Self { width, height, data })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }

    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.data.get((y as usize) * (self.width as usize) + (x as usize)).copied()
    }

    pub fn set(&mut self, x: u32, y: u32, value: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = (y as usize) * (self.width as usize) + (x as usize);
        if let Some(slot) = self.data.get_mut(index) {
            *slot = value;
        }
    }

    pub fn covered(&self, threshold: u8) -> u64 {
        self.data.iter().filter(|value| **value >= threshold).count() as u64
    }

    /// Binary view of this plane.
    pub fn thresholded(&self, threshold: u8) -> Self {
        let data = self.data.iter().map(|value| if *value >= threshold { 255 } else { 0 }).collect();
        Self { width: self.width, height: self.height, data }
    }

    /// Invert coverage.
    pub fn inverted(&self) -> Self {
        let data = self.data.iter().map(|value| 255 - *value).collect();
        Self { width: self.width, height: self.height, data }
    }

    /// A separable box blur, repeated: the classic way to feather an edge
    /// without pulling in an image-processing dependency. `radius` is in pixels.
    pub fn blurred(&self, radius: u32) -> Self {
        if radius == 0 {
            return self.clone();
        }
        let mut current = self.clone();
        // Three box passes approximate a Gaussian closely enough for an edge.
        for _ in 0..3 {
            current = current.box_pass(radius);
        }
        current
    }

    fn box_pass(&self, radius: u32) -> Self {
        let w = self.width as usize;
        let h = self.height as usize;
        let mut horizontal = vec![0u16; self.data.len()];
        let mut out = vec![0u8; self.data.len()];
        let window = (radius as usize) * 2 + 1;
        for y in 0..h {
            let row = &self.data[y * w..(y + 1) * w];
            let mut sum: u32 = 0;
            for offset in 0..window {
                let x = offset as i64 - radius as i64;
                sum += u32::from(row[x.clamp(0, (w - 1) as i64) as usize]);
            }
            for x in 0..w {
                horizontal[y * w + x] = (sum / window as u32) as u16;
                let leaving = (x as i64 - radius as i64).clamp(0, (w - 1) as i64) as usize;
                let entering = (x as i64 + radius as i64 + 1).clamp(0, (w - 1) as i64) as usize;
                sum = sum + u32::from(row[entering]) - u32::from(row[leaving]);
            }
        }
        for x in 0..w {
            let mut sum: u32 = 0;
            for offset in 0..window {
                let y = offset as i64 - radius as i64;
                sum += u32::from(horizontal[y.clamp(0, (h - 1) as i64) as usize * w + x]);
            }
            for y in 0..h {
                out[y * w + x] = (sum / window as u32) as u8;
                let leaving = (y as i64 - radius as i64).clamp(0, (h - 1) as i64) as usize;
                let entering = (y as i64 + radius as i64 + 1).clamp(0, (h - 1) as i64) as usize;
                sum = sum + u32::from(horizontal[entering * w + x]) - u32::from(horizontal[leaving * w + x]);
            }
        }
        Self { width: self.width, height: self.height, data: out }
    }

    /// Grow coverage by `radius` pixels (a square structuring element, applied
    /// separably — cheap and predictable).
    pub fn expanded(&self, radius: u32) -> Self {
        self.morph(radius, true)
    }

    /// Shrink coverage by `radius` pixels.
    pub fn contracted(&self, radius: u32) -> Self {
        self.morph(radius, false)
    }

    fn morph(&self, radius: u32, grow: bool) -> Self {
        if radius == 0 {
            return self.clone();
        }
        let w = self.width as usize;
        let h = self.height as usize;
        let mut horizontal = self.data.clone();
        for y in 0..h {
            for x in 0..w {
                let mut best = self.data[y * w + x];
                for offset in 0..=(radius as usize * 2) {
                    let sx = (x as i64 + offset as i64 - radius as i64).clamp(0, (w - 1) as i64) as usize;
                    let value = self.data[y * w + sx];
                    best = if grow { best.max(value) } else { best.min(value) };
                }
                horizontal[y * w + x] = best;
            }
        }
        let mut out = horizontal.clone();
        for y in 0..h {
            for x in 0..w {
                let mut best = horizontal[y * w + x];
                for offset in 0..=(radius as usize * 2) {
                    let sy = (y as i64 + offset as i64 - radius as i64).clamp(0, (h - 1) as i64) as usize;
                    let value = horizontal[sy * w + x];
                    best = if grow { best.max(value) } else { best.min(value) };
                }
                out[y * w + x] = best;
            }
        }
        Self { width: self.width, height: self.height, data: out }
    }
}

/// How the matte should be produced.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MattingParams {
    /// Soften the edge by this many pixels.
    pub feather: f32,
    /// Grow (`> 0`) or shrink (`< 0`) the mask before softening.
    pub expand: f32,
    /// Extra smoothing of the boundary after feathering.
    pub smooth: f32,
    /// Invert coverage (matte the background instead).
    pub invert: bool,
}

impl Default for MattingParams {
    fn default() -> Self {
        Self { feather: 1.0, expand: 0.0, smooth: 0.0, invert: false }
    }
}

impl MattingParams {
    /// Whether these parameters change anything at all.
    pub fn is_identity(&self) -> bool {
        self.feather <= 0.0 && self.expand == 0.0 && self.smooth <= 0.0 && !self.invert
    }
}

/// The matte output the export stage can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatteMode {
    /// 0 or 255 only.
    Binary,
    /// 8-bit coverage.
    Grayscale,
    /// 8-bit alpha, ready to be an alpha channel.
    Alpha,
}

impl MatteMode {
    pub fn label(self) -> &'static str {
        match self {
            MatteMode::Binary => "binary mask",
            MatteMode::Grayscale => "grayscale mask",
            MatteMode::Alpha => "alpha matte",
        }
    }

    /// The threshold each mode reads the source plane with.
    pub fn threshold(self) -> u8 {
        match self {
            MatteMode::Binary => 128,
            MatteMode::Grayscale | MatteMode::Alpha => 1,
        }
    }
}

/// Run the deterministic matting pipeline.
pub fn matte(mask: &Plane, params: &MattingParams, mode: MatteMode) -> Plane {
    let mut plane = mask.thresholded(128);
    if params.expand > 0.0 {
        plane = plane.expanded(params.expand.round() as u32);
    } else if params.expand < 0.0 {
        plane = plane.contracted((-params.expand).round() as u32);
    }
    if params.feather > 0.0 {
        plane = plane.blurred(params.feather.round().max(1.0) as u32);
    }
    if params.smooth > 0.0 {
        plane = plane.blurred(params.smooth.round().max(1.0) as u32);
    }
    if params.invert {
        plane = plane.inverted();
    }
    match mode {
        MatteMode::Binary => plane.thresholded(128),
        MatteMode::Grayscale | MatteMode::Alpha => plane,
    }
}

/// Combine several objects' mattes into one, keeping every visible layer.
pub fn combine(layers: &[Plane], mode: MatteMode) -> Option<Plane> {
    let first = layers.first()?;
    let mut out = first.clone();
    for layer in layers.iter().skip(1) {
        if layer.width != out.width || layer.height != out.height {
            return None;
        }
        for (slot, value) in out.data.iter_mut().zip(layer.data.iter()) {
            *slot = (*slot).max(*value);
        }
    }
    Some(match mode {
        MatteMode::Binary => out.thresholded(128),
        MatteMode::Grayscale | MatteMode::Alpha => out,
    })
}

/// Apply an alpha plane to RGB pixels: `rgb` is interleaved `[r, g, b]`, the
/// result is interleaved `[r, g, b, a]`. Premultiplied when `premultiply`.
pub fn apply_alpha(rgb: &[u8], alpha: &Plane, premultiply: bool) -> Option<Vec<u8>> {
    let pixels = (alpha.width as usize).checked_mul(alpha.height as usize)?;
    if rgb.len() != pixels * 3 {
        return None;
    }
    let mut out = vec![0u8; pixels * 4];
    for index in 0..pixels {
        let a = alpha.data.get(index).copied().unwrap_or(0);
        let (r, g, b) = (rgb[index * 3], rgb[index * 3 + 1], rgb[index * 3 + 2]);
        let scale = |value: u8| -> u8 { if premultiply { ((u32::from(value) * u32::from(a)) / 255) as u8 } else { value } };
        out[index * 4] = scale(r);
        out[index * 4 + 1] = scale(g);
        out[index * 4 + 2] = scale(b);
        out[index * 4 + 3] = a;
    }
    Some(out)
}

/// The seam for a learned matting model. A backend may only be used when its
/// licence allows it and the user installed it themselves — the catalogue marks
/// RVM and MatAnyone as [`crate::models::Redistribution::Prohibited`], so no
/// implementation of this trait ships enabled.
pub trait MatteBackend {
    fn name(&self) -> &'static str;
    /// Whether this backend is allowed in this build at all.
    fn enabled(&self) -> bool;
    /// Refine a mask into an alpha plane. Never called during development.
    fn refine(&self, mask: &Plane, params: &MattingParams) -> Option<Plane>;
}

/// The built-in, always-available backend (no weights, no runtime).
pub struct BuiltInMatte;

impl MatteBackend for BuiltInMatte {
    fn name(&self) -> &'static str {
        "FrameForge edge matting"
    }

    fn enabled(&self) -> bool {
        true
    }

    fn refine(&self, mask: &Plane, params: &MattingParams) -> Option<Plane> {
        Some(matte(mask, params, MatteMode::Alpha))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_plane(width: u32, height: u32, x0: u32, y0: u32, size: u32) -> Plane {
        let mut plane = Plane::new(width, height, 0).expect("plane");
        for y in y0..(y0 + size).min(height) {
            for x in x0..(x0 + size).min(width) {
                plane.set(x, y, 255);
            }
        }
        plane
    }

    #[test]
    fn a_plane_measures_itself_and_refuses_a_mismatched_buffer() {
        let plane = square_plane(16, 16, 4, 4, 8);
        assert_eq!(plane.covered(128), 64);
        assert_eq!(plane.get(4, 4), Some(255));
        assert_eq!(plane.get(3, 3), Some(0));
        assert_eq!(plane.get(99, 99), None, "out-of-range reads are None, not a panic");
        assert!(Plane::from_data(4, 4, vec![0u8; 3]).is_none());
        assert!(Plane::new(0, 4, 0).is_none());
    }

    #[test]
    fn expanding_and_contracting_change_the_covered_area_by_the_expected_border() {
        let plane = square_plane(32, 32, 10, 10, 10);
        assert_eq!(plane.covered(128), 100);

        let grown = plane.expanded(2);
        assert_eq!(grown.covered(128), 196, "two pixels on each side of a 10x10 square is 14x14");

        let shrunk = plane.contracted(2);
        assert_eq!(shrunk.covered(128), 36, "and 6x6 the other way");
    }

    #[test]
    fn feathering_softens_the_edge_without_moving_the_centre() {
        let plane = square_plane(32, 32, 8, 8, 16);
        let soft = plane.blurred(2);
        assert_eq!(soft.get(16, 16), Some(255), "the middle stays solid");
        let edge = soft.get(8, 16).expect("edge pixel");
        assert!(edge > 0 && edge < 255, "the boundary becomes partial coverage: {edge}");
        assert!(soft.covered(128) <= plane.covered(128) + 4, "and the binary reading stays close");
    }

    #[test]
    fn matting_honours_expand_feather_inversion_and_mode() {
        let plane = square_plane(32, 32, 10, 10, 12);
        let params = MattingParams { feather: 2.0, expand: 2.0, smooth: 0.0, invert: false };
        let alpha = matte(&plane, &params, MatteMode::Alpha);
        assert!(alpha.covered(128) > plane.covered(128), "expand grows the matte");
        assert!(alpha.data().iter().any(|value| *value > 0 && *value < 255), "feather produces partial coverage");

        let binary = matte(&plane, &params, MatteMode::Binary);
        assert!(binary.data().iter().all(|value| *value == 0 || *value == 255), "binary mode has no partial values");

        let inverted = matte(&plane, &MattingParams { invert: true, ..MattingParams::default() }, MatteMode::Alpha);
        assert!(inverted.covered(128) > plane.covered(128), "inverting a small subject covers most of the frame");
        assert!(MattingParams::default().is_identity() == false);
        assert!(MattingParams { feather: 0.0, expand: 0.0, smooth: 0.0, invert: false }.is_identity());
    }

    #[test]
    fn layers_combine_by_union_and_refuse_to_mix_sizes() {
        let a = square_plane(16, 16, 0, 0, 4);
        let b = square_plane(16, 16, 12, 12, 4);
        let merged = combine(&[a.clone(), b.clone()], MatteMode::Binary).expect("same size");
        assert_eq!(merged.covered(128), 32, "two disjoint squares");
        assert_eq!(merged.get(2, 2), Some(255));
        assert_eq!(merged.get(13, 13), Some(255));

        let wrong = square_plane(32, 32, 0, 0, 4);
        assert!(combine(&[a, wrong], MatteMode::Binary).is_none());
    }

    #[test]
    fn alpha_is_applied_to_rgb_with_and_without_premultiplication() {
        let alpha = Plane::from_data(2, 1, vec![0, 255]).expect("plane");
        let rgb = vec![10u8, 20, 30, 10, 20, 30];
        let straight = apply_alpha(&rgb, &alpha, false).expect("rgba");
        assert_eq!(straight, vec![10, 20, 30, 0, 10, 20, 30, 255]);
        let premultiplied = apply_alpha(&rgb, &alpha, true).expect("rgba");
        assert_eq!(premultiplied, vec![0, 0, 0, 0, 10, 20, 30, 255]);

        assert!(apply_alpha(&[1, 2, 3], &alpha, false).is_none(), "a mismatched rgb buffer is refused");
    }

    #[test]
    fn the_built_in_backend_is_enabled_and_the_learned_ones_are_not_shipped() {
        let backend = BuiltInMatte;
        assert!(backend.enabled());
        assert_eq!(backend.name(), "FrameForge edge matting");
        let plane = square_plane(8, 8, 2, 2, 4);
        assert!(backend.refine(&plane, &MattingParams::default()).is_some());

        // The catalogue's restricted matting backends must never be presented as
        // usable: their licence blocks bundling.
        for id in ["rvm", "matanyone"] {
            let model = crate::models::find(id).expect("catalogue entry");
            assert!(!model.license.redistribution.allows_download(), "{id} must not be offered as a download");
            assert!(!crate::models::may_download(id), "{id} must not be downloadable");
        }
    }
}
