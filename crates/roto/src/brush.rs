//! Brush strokes: what the workspace's paint/erase tools send to the engine.
//!
//! Strokes are pure geometry applied to a [`Plane`], so a correction is testable
//! and reproducible without a model. The workspace keeps the interactive canvas;
//! this is the authoritative definition of what a stroke *means*.

use serde::{Deserialize, Serialize};

use crate::matting::Plane;

/// Paint coverage in, or take it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushMode {
    Add,
    Subtract,
}

impl BrushMode {
    pub fn label(self) -> &'static str {
        match self {
            BrushMode::Add => "add to mask",
            BrushMode::Subtract => "remove from mask",
        }
    }
}

/// The brush settings the workspace exposes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushSettings {
    /// Radius in image pixels.
    pub radius: f32,
    /// `1.0` is a hard dab, lower values feather the edge of the dab itself.
    pub hardness: f32,
    pub mode: BrushMode,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self { radius: 12.0, hardness: 1.0, mode: BrushMode::Add }
    }
}

/// One stroke: a polyline of dabs on one frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub frame: i64,
    pub settings: BrushSettings,
    pub points: Vec<(f32, f32)>,
}

impl Stroke {
    pub fn new(frame: i64, settings: BrushSettings) -> Self {
        Self { frame, settings, points: Vec::new() }
    }

    pub fn push(&mut self, x: f32, y: f32) {
        self.points.push((x, y));
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The axis-aligned bounds of the stroke, radius included.
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in &self.points {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        (x0 <= x1).then(|| (x0 - self.settings.radius, y0 - self.settings.radius, x1 + self.settings.radius, y1 + self.settings.radius))
    }

    /// Apply the stroke to a plane; returns the number of pixels touched.
    pub fn apply(&self, plane: &mut Plane) -> usize {
        if self.is_empty() {
            return 0;
        }
        let mut touched = 0usize;
        let mut previous: Option<(f32, f32)> = None;
        for (x, y) in &self.points {
            match previous {
                None => touched += self.dab(plane, *x, *y),
                Some((px, py)) => {
                    // Dab along the segment so a fast drag leaves no gaps.
                    let distance = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                    let step = (self.settings.radius * 0.4).max(1.0);
                    let steps = (distance / step).ceil().max(1.0) as u32;
                    for index in 1..=steps {
                        let t = index as f32 / steps as f32;
                        touched += self.dab(plane, px + (x - px) * t, py + (y - py) * t);
                    }
                }
            }
            previous = Some((*x, *y));
        }
        touched
    }

    fn dab(&self, plane: &mut Plane, x: f32, y: f32) -> usize {
        let radius = self.settings.radius.max(0.5);
        let width = plane.width();
        let height = plane.height();
        let x0 = ((x - radius).floor().max(0.0)) as u32;
        let x1 = ((x + radius).ceil().min((width - 1) as f32).max(0.0)) as u32;
        let y0 = ((y - radius).floor().max(0.0)) as u32;
        let y1 = ((y + radius).ceil().min((height - 1) as f32).max(0.0)) as u32;
        let mut touched = 0usize;
        for py in y0..=y1.min(height.saturating_sub(1)) {
            for px in x0..=x1.min(width.saturating_sub(1)) {
                let dx = px as f32 + 0.5 - x;
                let dy = py as f32 + 0.5 - y;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance > radius {
                    continue;
                }
                // Hardness controls the falloff inside the dab.
                let hardness = self.settings.hardness.clamp(0.05, 1.0);
                let inner = radius * hardness;
                let coverage = if distance <= inner { 1.0 } else { ((radius - distance) / (radius - inner).max(1e-3)).clamp(0.0, 1.0) };
                let value = (coverage * 255.0).round() as u8;
                let current = plane.get(px, py).unwrap_or(0);
                let next = match self.settings.mode {
                    BrushMode::Add => current.max(value),
                    BrushMode::Subtract => current.saturating_sub(value),
                };
                if next != current {
                    touched += 1;
                }
                plane.set(px, py, next);
            }
        }
        touched
    }
}

/// Apply a whole correction pass: every stroke, in order.
pub fn apply_strokes(plane: &mut Plane, strokes: &[Stroke]) -> usize {
    strokes.iter().map(|stroke| stroke.apply(plane)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dab_paints_a_round_area_and_nothing_outside_it() {
        let mut plane = Plane::new(32, 32, 0).expect("plane");
        let mut stroke = Stroke::new(0, BrushSettings { radius: 4.0, hardness: 1.0, mode: BrushMode::Add });
        stroke.push(16.0, 16.0);
        stroke.apply(&mut plane);
        assert_eq!(plane.get(16, 16), Some(255));
        assert_eq!(plane.get(19, 16), Some(255), "inside the radius");
        assert_eq!(plane.get(23, 16), Some(0), "outside the radius stays clear");
        let covered = plane.covered(128);
        assert!((37..=57).contains(&covered), "a radius-4 disc is about 49 pixels, measured {covered}");
        assert_eq!(stroke.bounds(), Some((12.0, 12.0, 20.0, 20.0)));
    }

    #[test]
    fn subtract_erases_only_the_covered_pixels() {
        let mut plane = Plane::new(32, 32, 0).expect("plane");
        let mut add = Stroke::new(0, BrushSettings { radius: 6.0, hardness: 1.0, mode: BrushMode::Add });
        add.push(16.0, 16.0);
        add.apply(&mut plane);
        let painted = plane.covered(128);
        assert!(painted > 0);

        let mut erase = Stroke::new(0, BrushSettings { radius: 2.0, hardness: 1.0, mode: BrushMode::Subtract });
        erase.push(16.0, 16.0);
        erase.apply(&mut plane);
        assert_eq!(plane.get(16, 16), Some(0), "the erased centre is clear");
        assert!(plane.covered(128) < painted, "erasing only removes coverage");
        assert_eq!(erase.settings.mode.label(), "remove from mask");
    }

    #[test]
    fn a_drag_dabs_along_the_path_without_gaps() {
        let mut plane = Plane::new(64, 16, 0).expect("plane");
        let mut stroke = Stroke::new(3, BrushSettings { radius: 3.0, hardness: 1.0, mode: BrushMode::Add });
        stroke.push(4.0, 8.0);
        stroke.push(60.0, 8.0);
        stroke.apply(&mut plane);
        for x in 4..60 {
            assert!(plane.get(x, 8).unwrap_or(0) > 0, "column {x} must be painted");
        }
    }

    #[test]
    fn softness_makes_the_dab_edge_partial_and_strokes_accumulate() {
        let mut plane = Plane::new(32, 32, 0).expect("plane");
        let mut soft = Stroke::new(0, BrushSettings { radius: 8.0, hardness: 0.3, mode: BrushMode::Add });
        soft.push(16.0, 16.0);
        soft.apply(&mut plane);
        assert_eq!(plane.get(16, 16), Some(255), "the centre is solid");
        let edge = plane.get(23, 16).unwrap_or(0);
        assert!(edge > 0 && edge < 255, "the outer edge is partial: {edge}");

        let mut hard = Stroke::new(0, BrushSettings { radius: 1.0, hardness: 1.0, mode: BrushMode::Add });
        hard.push(1.0, 1.0);
        let touched = apply_strokes(&mut plane, &[hard]);
        assert!((1..=4).contains(&touched), "a radius-1 dab touches its immediate pixels, measured {touched}");
        assert!(Stroke::new(0, BrushSettings::default()).is_empty());
        assert_eq!(BrushSettings::default().radius, 12.0);
    }
}
