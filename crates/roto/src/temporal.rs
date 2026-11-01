//! Temporal quality control: the model-independent pass that decides whether a
//! propagated mask is trustworthy.
//!
//! This is *inspired by* the SENTRY-style idea of scoring masks over time, and
//! deliberately general: it works on any backend's output, because it only ever
//! looks at successive masks. It is **not** SENTRY and does not claim to be an
//! adapter for it — the SENTRY reference could not be verified from this
//! environment, so no attachment is asserted anywhere in this crate. A real
//! SENTRY adapter would plug in here behind [`QcBackend`] without touching the
//! rest of the pipeline.
//!
//! Every metric is arithmetic on mask summaries, so it is deterministic, cheap
//! and testable with no model.

use serde::{Deserialize, Serialize};

/// What the QC pass can see about one frame's mask. Summaries are computed once
/// per frame by [`MaskSummary::from_alpha`] and then compared.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskSummary {
    pub frame: i64,
    pub width: u32,
    pub height: u32,
    /// Pixels at or above the coverage threshold.
    pub covered: u64,
    /// Mean coverage of the whole plane, `0.0..=1.0`.
    pub mean_coverage: f32,
    /// Centroid in pixels, `None` when nothing is covered.
    pub centroid: Option<(f32, f32)>,
    /// Bounding box of the covered pixels: `(x0, y0, x1, y1)`, inclusive.
    pub bbox: Option<(u32, u32, u32, u32)>,
    /// Border pixels of the covered region: a cheap edge-length proxy.
    pub border: u64,
}

impl MaskSummary {
    /// Summarise an 8-bit alpha plane. `threshold` is the coverage cut-off
    /// (128 for a binary reading of a soft matte).
    pub fn from_alpha(frame: i64, alpha: &[u8], width: u32, height: u32, threshold: u8) -> Option<Self> {
        let expected = (width as usize).checked_mul(height as usize)?;
        if alpha.len() != expected || width == 0 || height == 0 {
            return None;
        }
        let mut covered = 0u64;
        let mut sum = 0u64;
        let mut cx = 0f64;
        let mut cy = 0f64;
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..height {
            for x in 0..width {
                let value = alpha[(y as usize) * (width as usize) + (x as usize)];
                sum += u64::from(value);
                if value >= threshold {
                    covered += 1;
                    cx += f64::from(x);
                    cy += f64::from(y);
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        // Border pixels: covered pixels with at least one uncovered 4-neighbour
        // (or at the plane edge). Rows are iterated as slices, so this stays a
        // linear scan without index arithmetic.
        let row_width = width as usize;
        let uncovered = |row: Option<&[u8]>, x: usize| -> bool {
            match row {
                None => true,
                Some(row) => row.get(x).is_none_or(|value| *value < threshold),
            }
        };
        let mut border = 0u64;
        if covered > 0 {
            for (y, row) in alpha.chunks_exact(row_width).enumerate() {
                let above = (y > 0).then(|| &alpha[(y - 1) * row_width..y * row_width]);
                let below = alpha.get((y + 1) * row_width..(y + 2) * row_width);
                for (x, value) in row.iter().enumerate() {
                    if *value < threshold {
                        continue;
                    }
                    let up = uncovered(above, x);
                    let down = uncovered(below, x);
                    let left = uncovered(Some(row), x.wrapping_sub(1));
                    let right = uncovered(Some(row), x + 1);
                    if up || down || left || right {
                        border += 1;
                    }
                }
            }
        }
        let centroid = (covered > 0).then(|| ((cx / covered as f64) as f32, (cy / covered as f64) as f32));
        let bbox = (covered > 0).then_some((x0, y0, x1, y1));
        Some(Self { frame, width, height, covered, mean_coverage: sum as f32 / (expected as f32 * 255.0), centroid, bbox, border })
    }

    /// Intersection over union against another summary of the same size.
    /// `None` when the two masks do not share a plane.
    pub fn iou(&self, other: &MaskSummary) -> Option<f32> {
        if self.width != other.width || self.height != other.height {
            return None;
        }
        // Bounding boxes are enough for the QC verdict and cost nothing extra,
        // but the union of two boxes can hide a hollow mask; the rest of the
        // metrics catch that, so this stays a box IoU and says so.
        match (self.bbox, other.bbox) {
            (Some((ax0, ay0, ax1, ay1)), Some((bx0, by0, bx1, by1))) => {
                let ix = (ax1.min(bx1).saturating_sub(ax0.max(bx0))) as f32;
                let iy = (ay1.min(by1).saturating_sub(ay0.max(by0))) as f32;
                let intersection = (ix + 1.0) * (iy + 1.0);
                let area_a = ((ax1 - ax0) + 1) as f32 * ((ay1 - ay0) + 1) as f32;
                let area_b = ((bx1 - bx0) + 1) as f32 * ((by1 - by0) + 1) as f32;
                let union = area_a + area_b - intersection;
                (union > 0.0).then(|| (intersection / union).clamp(0.0, 1.0))
            }
            _ => Some(0.0),
        }
    }

    /// The area ratio against another summary (`1.0` = same area).
    pub fn area_ratio(&self, other: &MaskSummary) -> Option<f32> {
        (other.covered > 0).then(|| self.covered as f32 / other.covered as f32)
    }
}

/// What the QC pass can complain about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SignalKind {
    /// The mask vanished (occlusion or tracking loss).
    Disappeared,
    /// The mask came back after vanishing.
    Reappeared,
    /// The area jumped.
    SuddenAreaChange,
    /// The centroid moved further than plausible in one frame.
    CentroidJump,
    /// The mask barely overlaps its predecessor.
    IdentityDrift,
    /// The mask flickers: it grows then shrinks (or back) in three frames.
    Flicker,
    /// The mask is almost entirely covered or almost empty.
    Degenerate,
    /// The boundary is very long for its area: a noisy, poorly defined edge.
    RaggedEdge,
}

impl SignalKind {
    pub fn label(self) -> &'static str {
        match self {
            SignalKind::Disappeared => "mask disappeared",
            SignalKind::Reappeared => "mask reappeared",
            SignalKind::SuddenAreaChange => "sudden size change",
            SignalKind::CentroidJump => "position jump",
            SignalKind::IdentityDrift => "identity drift",
            SignalKind::Flicker => "flicker",
            SignalKind::Degenerate => "empty or full mask",
            SignalKind::RaggedEdge => "ragged boundary",
        }
    }

    /// What the workspace should offer for this kind of trouble.
    pub fn suggested_action(self) -> RecoveryAction {
        match self {
            SignalKind::Disappeared | SignalKind::Reappeared => RecoveryAction::ReDetect,
            SignalKind::IdentityDrift | SignalKind::CentroidJump => RecoveryAction::ReinitializeFromBox,
            SignalKind::SuddenAreaChange | SignalKind::Flicker | SignalKind::RaggedEdge => RecoveryAction::BrushCorrect,
            SignalKind::Degenerate => RecoveryAction::BrushCorrect,
        }
    }
}

/// What the user can do about a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// Ask the selection engine for a fresh mask on this frame.
    ReDetect,
    /// Give the tracker a new box and restart from here.
    ReinitializeFromBox,
    /// Correct by hand, then propagate again from the corrected frame.
    BrushCorrect,
}

impl RecoveryAction {
    pub fn label(self) -> &'static str {
        match self {
            RecoveryAction::ReDetect => "re-detect the object on this frame",
            RecoveryAction::ReinitializeFromBox => "re-initialise tracking from a box",
            RecoveryAction::BrushCorrect => "correct the mask by hand and re-propagate",
        }
    }
}

/// One observation on one frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameSignal {
    pub frame: i64,
    pub kind: SignalKind,
    /// `0.0..=1.0`, larger is worse. Drives the review order, nothing else.
    pub severity: f32,
    pub detail: String,
}

/// A contiguous run of frames that need attention.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlaggedRange {
    pub first: i64,
    pub last: i64,
    pub worst_severity: f32,
    pub kinds: Vec<SignalKind>,
}

impl FlaggedRange {
    pub fn frames(&self) -> i64 {
        self.last.saturating_sub(self.first).saturating_add(1)
    }
}

/// Thresholds for the QC pass. Defaults are deliberately loose: the point is to
/// catch real breakage, not to nag.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct QcLimits {
    /// Area ratio outside `1/ratio .. ratio` is a problem.
    pub area_ratio: f32,
    /// Centroid movement allowed in one frame, as a fraction of the mask's own
    /// diagonal.
    pub centroid_jump: f32,
    /// Minimum box IoU against the previous frame.
    pub min_iou: f32,
    /// Covered fraction below which the mask counts as gone.
    pub min_coverage: f32,
    /// Border-to-area ratio above which the edge counts as ragged.
    pub max_raggedness: f32,
}

impl Default for QcLimits {
    fn default() -> Self {
        Self { area_ratio: 1.6, centroid_jump: 0.75, min_iou: 0.35, min_coverage: 0.0002, max_raggedness: 0.8 }
    }
}

/// The verdict for a whole run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QualityReport {
    pub frames: usize,
    pub signals: Vec<FrameSignal>,
    pub flagged: Vec<FlaggedRange>,
    /// The frame to restart from, when a restart is the suggested fix.
    pub reinit_from: Option<i64>,
}

impl QualityReport {
    /// Whether the run can be trusted as-is.
    pub fn is_clean(&self) -> bool {
        self.signals.is_empty()
    }

    /// The frames the workspace should show the user, ascending.
    pub fn review_frames(&self) -> Vec<i64> {
        let mut frames: Vec<i64> = self.flagged.iter().map(|range| range.first).collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    pub fn summary(&self) -> String {
        if self.is_clean() {
            return format!("{} frames checked, no problems found", self.frames);
        }
        format!("{} frames checked, {} signal(s) in {} range(s)", self.frames, self.signals.len(), self.flagged.len())
    }
}

/// Score a whole propagated run. Frames must be in ascending order.
pub fn analyze(frames: &[MaskSummary], limits: &QcLimits) -> QualityReport {
    let mut signals: Vec<FrameSignal> = Vec::new();
    let mut vanished_at: Option<i64> = None;

    for (index, current) in frames.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|i| frames.get(i));

        // Disappearance and reappearance.
        let gone = current.covered == 0 || current.mean_coverage < limits.min_coverage;
        if gone {
            if vanished_at.is_none() {
                vanished_at = Some(current.frame);
                signals.push(FrameSignal { frame: current.frame, kind: SignalKind::Disappeared, severity: 1.0, detail: "the mask covers nothing".to_string() });
            }
            continue;
        }
        if let Some(from) = vanished_at.take() {
            signals.push(FrameSignal {
                frame: current.frame,
                kind: SignalKind::Reappeared,
                severity: 0.8,
                detail: format!("the mask returns after vanishing at frame {from}; check the object identity"),
            });
        }

        // Degenerate coverage: everything or almost nothing on a big plane.
        if current.mean_coverage > 0.98 {
            signals.push(FrameSignal {
                frame: current.frame,
                kind: SignalKind::Degenerate,
                severity: 0.6,
                detail: format!("the mask covers {:.1}% of the frame", current.mean_coverage * 100.0),
            });
        }

        let Some(previous) = previous else { continue };
        let Some(iou) = current.iou(previous) else { continue };

        if let Some(ratio) = current.area_ratio(previous) {
            let limit = limits.area_ratio.max(1.0);
            if ratio > limit || ratio < 1.0 / limit {
                signals.push(FrameSignal {
                    frame: current.frame,
                    kind: SignalKind::SuddenAreaChange,
                    severity: ((ratio.max(1.0 / ratio) - 1.0) / 2.0).clamp(0.0, 1.0),
                    detail: format!("the area changed by {:.0}% in one frame", (ratio - 1.0).abs() * 100.0),
                });
            }
        }

        if let (Some((cx, cy)), Some((px, py))) = (current.centroid, previous.centroid) {
            let (w, h) = match current.bbox {
                Some((x0, y0, x1, y1)) => (((x1 - x0) + 1) as f32, ((y1 - y0) + 1) as f32),
                None => (1.0, 1.0),
            };
            let diagonal = (w * w + h * h).sqrt().max(1.0);
            let moved = ((cx - px).powi(2) + (cy - py).powi(2)).sqrt();
            if moved > limits.centroid_jump * diagonal {
                signals.push(FrameSignal {
                    frame: current.frame,
                    kind: SignalKind::CentroidJump,
                    severity: (moved / (diagonal * 2.0)).clamp(0.0, 1.0),
                    detail: format!("the mask moved {moved:.1} px in one frame"),
                });
            }
        }

        if iou < limits.min_iou {
            signals.push(FrameSignal {
                frame: current.frame,
                kind: SignalKind::IdentityDrift,
                severity: (1.0 - iou).clamp(0.0, 1.0),
                detail: format!("only {:.0}% overlap with the previous frame", iou * 100.0),
            });
        }

        if let Some(before) = index.checked_sub(2).and_then(|i| frames.get(i))
            && before.covered > 0
            && previous.covered > 0
        {
            let up = previous.covered as f32 / before.covered as f32;
            let down = current.covered as f32 / previous.covered as f32;
            if (up > 1.2 && down < 1.0) || (up < 1.0 && down > 1.2) {
                signals.push(FrameSignal {
                    frame: current.frame,
                    kind: SignalKind::Flicker,
                    severity: 0.4,
                    detail: "the mask oscillates in size across three frames".to_string(),
                });
            }
        }

        let area = current.covered.max(1) as f32;
        let raggedness = current.border as f32 / area;
        if raggedness > limits.max_raggedness && current.covered > 64 {
            signals.push(FrameSignal {
                frame: current.frame,
                kind: SignalKind::RaggedEdge,
                severity: (raggedness / (limits.max_raggedness * 2.0)).clamp(0.0, 1.0),
                detail: format!("boundary-to-area ratio {raggedness:.2}: the edge is poorly defined"),
            });
        }
    }

    // A run that ends inside an occlusion is still worth flagging.
    if let Some(from) = vanished_at {
        signals.push(FrameSignal { frame: from, kind: SignalKind::Disappeared, severity: 0.9, detail: "the mask never returns".to_string() });
    }

    let flagged = group(&signals);
    let reinit_from = flagged.iter().find(|range| range.worst_severity >= 0.6).map(|range| range.first);
    QualityReport { frames: frames.len(), signals, flagged, reinit_from }
}

/// Group signals into contiguous flagged ranges (frames within 2 of each other).
fn group(signals: &[FrameSignal]) -> Vec<FlaggedRange> {
    let mut sorted: Vec<&FrameSignal> = signals.iter().collect();
    sorted.sort_by_key(|signal| signal.frame);
    let mut ranges: Vec<FlaggedRange> = Vec::new();
    for signal in sorted {
        match ranges.last_mut() {
            Some(range) if signal.frame <= range.last + 2 => {
                range.last = range.last.max(signal.frame);
                range.worst_severity = range.worst_severity.max(signal.severity);
                if !range.kinds.contains(&signal.kind) {
                    range.kinds.push(signal.kind);
                }
            }
            _ => ranges.push(FlaggedRange { first: signal.frame, last: signal.frame, worst_severity: signal.severity, kinds: vec![signal.kind] }),
        }
    }
    ranges
}

/// A seam for a real learned QC model (for example a SENTRY-style scorer). The
/// interface is deliberately the same shape as [`analyze`], so swapping the
/// scorer never touches the pipeline. No such backend is implemented or claimed.
pub trait QcBackend {
    fn name(&self) -> &'static str;
    fn score(&self, frames: &[MaskSummary], limits: &QcLimits) -> QualityReport;
}

/// The built-in arithmetic scorer.
pub struct ArithmeticQc;

impl QcBackend for ArithmeticQc {
    fn name(&self) -> &'static str {
        "built-in mask metrics"
    }

    fn score(&self, frames: &[MaskSummary], limits: &QcLimits) -> QualityReport {
        analyze(frames, limits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 64;
    const H: u32 = 36;

    /// A mask with a square of the given size at the given corner.
    fn square(frame: i64, x0: u32, y0: u32, size: u32) -> MaskSummary {
        let mut alpha = vec![0u8; (W * H) as usize];
        for y in y0..(y0 + size).min(H) {
            for x in x0..(x0 + size).min(W) {
                alpha[(y * W + x) as usize] = 255;
            }
        }
        MaskSummary::from_alpha(frame, &alpha, W, H, 128).expect("summary")
    }

    fn empty(frame: i64) -> MaskSummary {
        MaskSummary::from_alpha(frame, &vec![0u8; (W * H) as usize], W, H, 128).expect("summary")
    }

    #[test]
    fn a_summary_measures_area_centroid_box_and_edge() {
        let summary = square(0, 10, 5, 10);
        assert_eq!(summary.covered, 100);
        assert_eq!(summary.bbox, Some((10, 5, 19, 14)));
        assert_eq!(summary.centroid, Some((14.5, 9.5)));
        assert_eq!(summary.border, 36, "a 10x10 square has a 36-pixel border");
        assert!(summary.mean_coverage > 0.0 && summary.mean_coverage < 0.2);

        let empty = empty(0);
        assert_eq!(empty.covered, 0);
        assert_eq!(empty.centroid, None);
        assert_eq!(empty.bbox, None);
        // A mismatched plane is refused rather than mis-measured.
        assert!(MaskSummary::from_alpha(0, &[0u8; 4], W, H, 128).is_none());
    }

    #[test]
    fn a_steady_mask_reports_no_problems() {
        let frames: Vec<MaskSummary> = (0..10).map(|frame| square(frame, 20, 10, 12)).collect();
        let report = analyze(&frames, &QcLimits::default());
        assert!(report.is_clean(), "{}", report.summary());
        assert_eq!(report.frames, 10);
        assert!(report.reinit_from.is_none());
        let drifting: Vec<MaskSummary> = (0..10).map(|frame| square(frame, 20 + frame as u32, 10, 12)).collect();
        assert!(analyze(&drifting, &QcLimits::default()).is_clean(), "smooth motion is not drift");
    }

    #[test]
    fn a_vanished_mask_and_its_return_are_both_flagged() {
        let mut frames: Vec<MaskSummary> = (0..4).map(|frame| square(frame, 20, 10, 12)).collect();
        frames.push(empty(4));
        frames.push(empty(5));
        frames.push(square(6, 40, 20, 12));
        let report = analyze(&frames, &QcLimits::default());

        let kinds: Vec<SignalKind> = report.signals.iter().map(|signal| signal.kind).collect();
        assert!(kinds.contains(&SignalKind::Disappeared), "{kinds:?}");
        assert!(kinds.contains(&SignalKind::Reappeared), "{kinds:?}");
        assert!(!report.is_clean());
        assert!(report.reinit_from.is_some(), "a reappearance suggests re-detection");
        assert_eq!(report.signals[0].kind.suggested_action(), RecoveryAction::ReDetect);
        assert!(report.summary().contains("signal"));
    }

    #[test]
    fn a_run_that_ends_occluded_is_flagged_at_the_last_visible_frame() {
        let mut frames: Vec<MaskSummary> = (0..3).map(|frame| square(frame, 20, 10, 12)).collect();
        frames.push(empty(3));
        let report = analyze(&frames, &QcLimits::default());
        assert!(report.signals.iter().any(|signal| signal.kind == SignalKind::Disappeared && signal.detail.contains("never returns")));
    }

    #[test]
    fn a_size_explosion_and_a_shrink_are_reported_with_severity() {
        let mut frames = vec![square(0, 20, 10, 8)];
        frames.push(square(1, 10, 2, 24));
        let report = analyze(&frames, &QcLimits::default());
        let signal = report.signals.iter().find(|signal| signal.kind == SignalKind::SuddenAreaChange).expect("an area jump is flagged");
        assert!(signal.severity > 0.0 && signal.severity <= 1.0);
        assert_eq!(signal.kind.suggested_action(), RecoveryAction::BrushCorrect);
    }

    #[test]
    fn an_identity_swap_is_flagged_as_drift_and_asks_for_a_box() {
        let mut frames = vec![square(0, 2, 2, 10)];
        frames.push(square(1, 52, 24, 10));
        let report = analyze(&frames, &QcLimits::default());
        let kinds: Vec<SignalKind> = report.signals.iter().map(|signal| signal.kind).collect();
        assert!(kinds.contains(&SignalKind::IdentityDrift) || kinds.contains(&SignalKind::CentroidJump), "{kinds:?}");
        assert!(report.flagged.iter().any(|range| range.kinds.iter().any(|kind| kind.suggested_action() == RecoveryAction::ReinitializeFromBox)));
    }

    #[test]
    fn a_flicker_across_three_frames_is_flagged_once() {
        let mut frames = vec![square(0, 20, 10, 10)];
        frames.push(square(1, 12, 6, 26));
        frames.push(square(2, 20, 10, 10));
        let report = analyze(&frames, &QcLimits::default());
        assert!(report.signals.iter().any(|signal| signal.kind == SignalKind::Flicker), "{:?}", report.signals);
        assert!(!report.flagged[0].kinds.is_empty());
    }

    #[test]
    fn a_ragged_boundary_and_a_full_frame_mask_are_flagged() {
        // A checkerboard edge: lots of boundary for very little area.
        let mut alpha = vec![0u8; (W * H) as usize];
        for y in 0..H {
            for x in 0..W {
                if (x + y) % 2 == 0 {
                    alpha[(y * W + x) as usize] = 255;
                }
            }
        }
        let ragged = MaskSummary::from_alpha(0, &alpha, W, H, 128).expect("summary");
        let report = analyze(&[ragged], &QcLimits::default());
        let kinds: Vec<SignalKind> = report.signals.iter().map(|signal| signal.kind).collect();
        assert!(kinds.contains(&SignalKind::RaggedEdge) || kinds.contains(&SignalKind::Degenerate), "{kinds:?}");

        let full = MaskSummary::from_alpha(1, &vec![255u8; (W * H) as usize], W, H, 128).expect("summary");
        let report = analyze(&[full], &QcLimits::default());
        assert!(report.signals.iter().any(|signal| signal.kind == SignalKind::Degenerate));
    }

    #[test]
    fn the_arithmetic_backend_is_named_and_swappable() {
        let frames = vec![square(0, 20, 10, 12)];
        let backend = ArithmeticQc;
        assert_eq!(backend.name(), "built-in mask metrics");
        assert_eq!(backend.score(&frames, &QcLimits::default()).frames, 1);
    }
}
