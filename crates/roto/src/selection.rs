//! Interactive selection: the prompts the user draws, in image coordinates, and
//! the session that collects them before a backend is asked for a mask.
//!
//! This module is pure geometry. It never talks to a model: it produces the
//! prompt set a selection engine consumes, which is what lets SAM 2.1, EdgeTAM
//! or any future selector share one workspace.

use serde::{Deserialize, Serialize};

/// An axis-aligned box in image pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl BoxRect {
    pub fn from_corners(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }

    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }

    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// Clamp into an image of the given size; a degenerate box becomes a point
    /// of zero area rather than a negative rectangle.
    pub fn clamp_to(&self, width: u32, height: u32) -> Self {
        let max_x = width.saturating_sub(1) as f32;
        let max_y = height.saturating_sub(1) as f32;
        Self { x0: self.x0.clamp(0.0, max_x), y0: self.y0.clamp(0.0, max_y), x1: self.x1.clamp(0.0, max_x), y1: self.y1.clamp(0.0, max_y) }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

/// One prompt the user placed, tagged with the frame it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Prompt {
    Point {
        x: f32,
        y: f32,
        positive: bool,
        frame: i64,
    },
    Box {
        rect: BoxRect,
        frame: i64,
    },
    /// A mask the user corrected by hand; the backend propagates from it.
    Mask {
        frame: i64,
    },
}

impl Prompt {
    pub fn frame(&self) -> i64 {
        match self {
            Prompt::Point { frame, .. } | Prompt::Box { frame, .. } | Prompt::Mask { frame } => *frame,
        }
    }

    pub fn is_negative(&self) -> bool {
        matches!(self, Prompt::Point { positive: false, .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Prompt::Point { positive: true, .. } => "add point",
            Prompt::Point { positive: false, .. } => "remove point",
            Prompt::Box { .. } => "box",
            Prompt::Mask { .. } => "mask",
        }
    }
}

/// The prompt set the user has built for one object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SelectionSession {
    prompts: Vec<Prompt>,
}

impl SelectionSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prompts(&self) -> &[Prompt] {
        &self.prompts
    }

    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
    }

    pub fn push(&mut self, prompt: Prompt) {
        self.prompts.push(prompt);
    }

    /// Add a click. A click on the same pixel toggles nothing: positive and
    /// negative points are separate, exactly as the backends expect.
    pub fn add_point(&mut self, x: f32, y: f32, positive: bool, frame: i64) {
        self.prompts.push(Prompt::Point { x, y, positive, frame });
    }

    /// Keep only the single most recent box: a box is a global statement about
    /// the object, and sending several confuses every backend.
    pub fn set_box(&mut self, rect: BoxRect, frame: i64) {
        self.prompts.retain(|prompt| !matches!(prompt, Prompt::Box { .. }));
        self.prompts.push(Prompt::Box { rect, frame });
    }

    pub fn set_mask_prompt(&mut self, frame: i64) {
        self.prompts.retain(|prompt| !matches!(prompt, Prompt::Mask { .. }));
        self.prompts.push(Prompt::Mask { frame });
    }

    /// Drop the last prompt the user placed (the workspace's Undo prompt).
    pub fn pop(&mut self) -> Option<Prompt> {
        self.prompts.pop()
    }

    pub fn clear(&mut self) {
        self.prompts.clear();
    }

    /// The prompts for one frame, which is what a per-frame selection request
    /// carries.
    pub fn for_frame(&self, frame: i64) -> Vec<Prompt> {
        self.prompts.iter().copied().filter(|prompt| prompt.frame() == frame).collect()
    }

    pub fn positives(&self) -> usize {
        self.prompts.iter().filter(|prompt| matches!(prompt, Prompt::Point { positive: true, .. })).count()
    }

    pub fn negatives(&self) -> usize {
        self.prompts.iter().filter(|prompt| prompt.is_negative()).count()
    }

    /// A one-line description for the status bar.
    pub fn summary(&self) -> String {
        let boxes = self.prompts.iter().filter(|prompt| matches!(prompt, Prompt::Box { .. })).count();
        let masks = self.prompts.iter().filter(|prompt| matches!(prompt, Prompt::Mask { .. })).count();
        format!("{} positive, {} negative, {boxes} box, {masks} mask prompt(s)", self.positives(), self.negatives())
    }

    /// The frames that carry a prompt, in ascending order.
    pub fn prompt_frames(&self) -> Vec<i64> {
        let mut frames: Vec<i64> = self.prompts.iter().map(Prompt::frame).collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_and_boxes_accumulate_the_way_the_backends_expect() {
        let mut session = SelectionSession::new();
        assert!(session.is_empty());
        session.add_point(10.0, 20.0, true, 0);
        session.add_point(30.0, 20.0, false, 0);
        assert_eq!(session.positives(), 1);
        assert_eq!(session.negatives(), 1);

        session.set_box(BoxRect::from_corners(5.0, 5.0, 40.0, 30.0), 0);
        session.set_box(BoxRect::from_corners(6.0, 6.0, 41.0, 31.0), 0);
        let boxes: Vec<&Prompt> = session.prompts().iter().filter(|prompt| matches!(prompt, Prompt::Box { .. })).collect();
        assert_eq!(boxes.len(), 1, "only the newest box survives: {boxes:?}");

        session.set_mask_prompt(0);
        assert_eq!(session.for_frame(0).len(), 4, "two points, one box, one mask");
        assert!(session.for_frame(3).is_empty());
        assert_eq!(session.prompt_frames(), vec![0]);
        assert_eq!(session.pop().map(|prompt| prompt.label()), Some("mask"));
        assert!(session.summary().contains("1 positive"));
    }

    #[test]
    fn a_box_normalises_its_corners_and_clamps_into_the_image() {
        // (`box` is a reserved word, so the fixture is `rect`.)
        let rect = BoxRect::from_corners(40.0, 30.0, 10.0, 5.0);
        assert_eq!((rect.x0, rect.y0, rect.x1, rect.y1), (10.0, 5.0, 40.0, 30.0));
        assert_eq!(rect.width(), 30.0);
        assert_eq!(rect.height(), 25.0);
        assert_eq!(rect.area(), 750.0);
        assert!(rect.contains(20.0, 20.0));
        assert!(!rect.contains(9.0, 20.0));

        let clamped = BoxRect::from_corners(-5.0, -5.0, 500.0, 500.0).clamp_to(64, 36);
        assert_eq!((clamped.x0, clamped.y0, clamped.x1, clamped.y1), (0.0, 0.0, 63.0, 35.0));
        assert!(clamped.contains(63.0, 35.0));
    }

    #[test]
    fn clearing_a_session_leaves_nothing_behind() {
        let mut session = SelectionSession::new();
        session.add_point(1.0, 1.0, true, 0);
        session.clear();
        assert!(session.is_empty());
        assert_eq!(session.summary(), "0 positive, 0 negative, 0 box, 0 mask prompt(s)");
    }
}
