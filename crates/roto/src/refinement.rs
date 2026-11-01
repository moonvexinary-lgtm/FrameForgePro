//! Mask refinement: boolean combination of objects, morphology, polygon fills
//! and the correction keyframes a re-propagation starts from.
//!
//! All arithmetic on [`Plane`]s, so the refinement stage is deterministic and
//! runs with no model at all.

use serde::{Deserialize, Serialize};

use crate::matting::Plane;
use crate::tracking::{Direction, FrameRange};

/// Boolean combination of two layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    Union,
    Intersect,
    /// `a` without `b`.
    Subtract,
    /// Either, but not both.
    Exclusive,
}

impl BoolOp {
    pub fn label(self) -> &'static str {
        match self {
            BoolOp::Union => "combine",
            BoolOp::Intersect => "intersect",
            BoolOp::Subtract => "subtract",
            BoolOp::Exclusive => "exclusive",
        }
    }

    pub fn symbol(self) -> char {
        match self {
            BoolOp::Union => '+',
            BoolOp::Intersect => '&',
            BoolOp::Subtract => '-',
            BoolOp::Exclusive => '^',
        }
    }
}

/// Apply a boolean operation, refusing layers of different sizes.
pub fn combine(a: &Plane, b: &Plane, op: BoolOp) -> Option<Plane> {
    if a.width() != b.width() || a.height() != b.height() {
        return None;
    }
    let data = a
        .data()
        .iter()
        .zip(b.data().iter())
        .map(|(left, right)| match op {
            BoolOp::Union => (*left).max(*right),
            BoolOp::Intersect => (*left).min(*right),
            BoolOp::Subtract => (*left).saturating_sub(*right),
            BoolOp::Exclusive => (*left).max(*right) - (*left).min(*right),
        })
        .collect();
    Plane::from_data(a.width(), a.height(), data)
}

/// Morphological closing: grow then shrink, which fills pinholes and small gaps
/// without changing the outline much.
pub fn close(plane: &Plane, radius: u32) -> Plane {
    plane.expanded(radius).contracted(radius)
}

/// Morphological opening: shrink then grow, which removes speckle.
pub fn open(plane: &Plane, radius: u32) -> Plane {
    plane.contracted(radius).expanded(radius)
}

/// Remove covered islands smaller than `min_pixels`, and fill holes smaller than
/// `min_pixels` — the two artefacts that most often survive a tracker.
pub fn despeckle(plane: &Plane, min_pixels: u64) -> Plane {
    if min_pixels <= 1 {
        return plane.clone();
    }
    let mut out = plane.clone();
    let width = plane.width();
    let height = plane.height();
    let mut visited = vec![false; (width as usize) * (height as usize)];
    let threshold = 128u8;

    for start_y in 0..height {
        for start_x in 0..width {
            let index = (start_y as usize) * (width as usize) + (start_x as usize);
            if visited.get(index).copied().unwrap_or(true) {
                continue;
            }
            let target = plane.get(start_x, start_y).unwrap_or(0) >= threshold;
            // Flood fill one connected component of the same binary value.
            let mut stack = vec![(start_x, start_y)];
            let mut component: Vec<(u32, u32)> = Vec::new();
            while let Some((x, y)) = stack.pop() {
                let index = (y as usize) * (width as usize) + (x as usize);
                if visited.get(index).copied().unwrap_or(true) {
                    continue;
                }
                let value = plane.get(x, y).unwrap_or(0) >= threshold;
                if value != target {
                    continue;
                }
                if let Some(slot) = visited.get_mut(index) {
                    *slot = true;
                }
                component.push((x, y));
                if x > 0 {
                    stack.push((x - 1, y));
                }
                if y > 0 {
                    stack.push((x, y - 1));
                }
                if x + 1 < width {
                    stack.push((x + 1, y));
                }
                if y + 1 < height {
                    stack.push((x, y + 1));
                }
            }
            if (component.len() as u64) < min_pixels {
                // A small foreground island disappears; a small background hole
                // is filled.
                let fill = if target { 0 } else { 255 };
                for (x, y) in component {
                    out.set(x, y, fill);
                }
            }
        }
    }
    out
}

/// Fill a polygon with a scanline pass. Points are in image coordinates and may
/// be given in either winding order.
pub fn fill_polygon(plane: &mut Plane, points: &[(f32, f32)]) -> usize {
    if points.len() < 3 {
        return 0;
    }
    let height = plane.height();
    let width = plane.width();
    let mut filled = 0usize;
    for y in 0..height {
        let scan = y as f32 + 0.5;
        let mut crossings: Vec<f32> = Vec::new();
        for index in 0..points.len() {
            let (x0, y0) = points[index];
            let (x1, y1) = points[(index + 1) % points.len()];
            let (top, bottom) = (y0.min(y1), y0.max(y1));
            if scan >= top && scan < bottom {
                let t = (scan - y0) / (y1 - y0);
                crossings.push(x0 + t * (x1 - x0));
            }
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in crossings.chunks_exact(2) {
            let from = pair[0].floor().max(0.0) as u32;
            let to = pair[1].ceil().min(width as f32) as u32;
            for x in from..to.min(width) {
                plane.set(x, y, 255);
                filled += 1;
            }
        }
    }
    filled
}

/// A hand correction on one frame: the mask the user painted.
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectionKeyframe {
    pub frame: i64,
    pub mask: Plane,
}

/// The corrections for a frame, if the user made one.
pub fn correction_at(corrections: &[CorrectionKeyframe], frame: i64) -> Option<&Plane> {
    corrections.iter().find(|correction| correction.frame == frame).map(|correction| &correction.mask)
}

/// Where a re-propagation should start, given the corrections the user kept.
/// Corrections are never discarded by a re-run.
pub fn resume_plan(range: FrameRange, direction: Direction, corrections: &[CorrectionKeyframe]) -> Vec<crate::tracking::Segment> {
    let marks: Vec<crate::engine::Correction> = corrections.iter().map(|correction| crate::engine::Correction::at(correction.frame)).collect();
    crate::tracking::plan_segments(range, direction, &marks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(width: u32, height: u32, x0: u32, y0: u32, size: u32) -> Plane {
        let mut plane = Plane::new(width, height, 0).expect("plane");
        for y in y0..(y0 + size).min(height) {
            for x in x0..(x0 + size).min(width) {
                plane.set(x, y, 255);
            }
        }
        plane
    }

    #[test]
    fn boolean_operations_combine_two_layers_by_the_expected_rule() {
        let a = square(16, 16, 0, 0, 8);
        let b = square(16, 16, 4, 0, 8);
        assert_eq!(combine(&a, &b, BoolOp::Union).expect("union").covered(128), 96, "8x8 + 8x8 overlapping by 4x8");
        assert_eq!(combine(&a, &b, BoolOp::Intersect).expect("intersect").covered(128), 32);
        assert_eq!(combine(&a, &b, BoolOp::Subtract).expect("subtract").covered(128), 32);
        assert_eq!(combine(&a, &b, BoolOp::Exclusive).expect("exclusive").covered(128), 64);
        assert!(combine(&a, &square(32, 32, 0, 0, 4), BoolOp::Union).is_none());
        assert_eq!(BoolOp::Union.symbol(), '+');
    }

    #[test]
    fn opening_removes_speckle_and_closing_fills_pinholes() {
        let mut speckled = square(32, 32, 10, 10, 12);
        speckled.set(2, 2, 255);
        speckled.set(3, 3, 255);
        let cleaned = open(&speckled, 1);
        assert_eq!(cleaned.get(2, 2), Some(0), "a lone speckle is removed");
        assert_eq!(cleaned.get(16, 16), Some(255), "the subject survives");

        let mut holed = square(32, 32, 10, 10, 12);
        holed.set(16, 16, 0);
        let closed = close(&holed, 1);
        assert_eq!(closed.get(16, 16), Some(255), "a pinhole is filled");
    }

    #[test]
    fn despeckle_removes_small_islands_and_fills_small_holes_only() {
        let mut plane = square(32, 32, 4, 4, 16);
        // A 3-pixel island and a 3-pixel hole on the subject.
        plane.set(28, 28, 255);
        plane.set(29, 28, 255);
        plane.set(30, 28, 255);
        plane.set(10, 10, 0);
        plane.set(11, 10, 0);

        let cleaned = despeckle(&plane, 5);
        assert_eq!(cleaned.get(28, 28), Some(0), "the tiny island is gone");
        assert_eq!(cleaned.get(10, 10), Some(255), "the tiny hole is filled");
        assert_eq!(cleaned.get(11, 10), Some(255));
        assert_eq!(cleaned.covered(128), 16 * 16, "the subject's area is unchanged");
    }

    #[test]
    fn a_polygon_fills_its_interior_and_nothing_else() {
        let mut plane = Plane::new(32, 32, 0).expect("plane");
        let filled = fill_polygon(&mut plane, &[(4.0, 4.0), (20.0, 4.0), (20.0, 12.0), (4.0, 12.0)]);
        assert_eq!(filled, 16 * 8);
        assert_eq!(plane.get(10, 8), Some(255));
        assert_eq!(plane.get(2, 2), Some(0));
        assert_eq!(plane.get(24, 20), Some(0));
        assert_eq!(fill_polygon(&mut plane, &[(1.0, 1.0), (2.0, 2.0)]), 0, "two points are not a polygon");
    }

    #[test]
    fn a_resume_plan_starts_from_every_correction_and_keeps_them() {
        let range = FrameRange::new(0, 23);
        let corrections = vec![CorrectionKeyframe { frame: 5, mask: square(32, 32, 0, 0, 4) }, CorrectionKeyframe { frame: 15, mask: square(32, 32, 0, 0, 4) }];
        let segments = resume_plan(range, Direction::Forward, &corrections);
        assert_eq!(segments.len(), 2, "{segments:?}");
        assert_eq!(segments[0].range.first, 5, "the first correction starts the first piece");
        assert_eq!(segments[1].range.first, 15);
        assert!(segments.iter().all(|segment| segment.from_correction.is_some()));
        assert!(correction_at(&corrections, 5).is_some());
        assert!(correction_at(&corrections, 6).is_none());
    }
}
