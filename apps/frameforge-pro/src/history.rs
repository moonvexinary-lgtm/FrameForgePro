//! Combined undo ordering for the editing project and the compositing
//! compositions.
//!
//! FilmCraft's [`Session`] owns the whole-project undo snapshots. Composition
//! edits live outside that project, so each one records a before/after
//! snapshot of the affected composition and adds a project-neutral step to the
//! engine history. Both kinds of edit therefore share one chronological
//! undo/redo order.
//!
//! The two stacks here stay in lockstep with `session.history.undo` and
//! `session.history.redo`: each engine step has exactly one slot, `None` for a
//! project edit and `Some` for the composition snapshot that rides with a
//! project-neutral step.

use std::sync::Arc;

use filmcraft_engine::{EngineError, Session};
use filmcraft_project::ItemId;
use frameforge_bridge::Composition;

/// The engine-history label for a composition step.
pub const COMPOSITION_LABEL: &str = "Edit composition";

/// A composition edit: the affected item and the states around it.
#[derive(Clone, Debug)]
pub struct CompositionStep {
    pub item: ItemId,
    pub before: Option<Composition>,
    pub after: Option<Composition>,
}

/// One undone or redone step, plus the label the engine history used.
#[derive(Clone, Debug)]
pub struct Undone {
    pub label: String,
    pub step: Option<CompositionStep>,
}

/// A history slot: `None` is a project step the engine already stores,
/// `Some` is the composition snapshot that rides with a project-neutral step.
type Step = Option<CompositionStep>;

/// The combined history stacks, kept in lockstep with the engine's own
/// `session.history.undo` / `session.history.redo` vectors.
#[derive(Default)]
pub struct EditHistory {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Bumped on every pointer press so one drag shares one undo step.
    gesture: u64,
    /// The gesture and item whose composition step is still open.
    open_gesture: Option<(u64, ItemId)>,
}

impl EditHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Record a project-changing engine command. `top_before` is the identity
    /// of the newest undo snapshot before the command (from [`undo_top`]).
    /// Commands that did not add a history step (selection, targeting, a merged
    /// fader move) do not create an entry.
    pub fn note_project_step(&mut self, session: &Session, top_before: Option<usize>) {
        if undo_top(session) == top_before {
            return;
        }
        // The engine may have trimmed its oldest step at the history limit, so
        // its length can stay the same while the top changes. At least this
        // command's step is present; anything it added beyond the first is
        // filled with project steps (upstream groups multiple edits into one).
        let before_len = self.undo.len();
        let after_len = session.history.undo.len();
        let added = after_len.saturating_sub(before_len).max(1);
        for _ in 0..added {
            self.undo.push(None);
        }
        self.redo.clear();
        self.open_gesture = None;
        self.trim_to_session(session);
    }

    /// Start (or continue) a pointer gesture. A press starts a new gesture;
    /// otherwise the same id is returned. A whole slider drag is one step.
    pub fn begin_gesture(&mut self, pressed: bool) -> u64 {
        if pressed {
            self.gesture = self.gesture.wrapping_add(1);
        }
        self.gesture
    }

    /// Close the current gesture once the pointer is up, so a later keyboard
    /// change is a separate step.
    pub fn end_gesture(&mut self, dragging: bool) {
        if !dragging {
            self.open_gesture = None;
        }
    }

    /// Record a composition edit. The first change of a gesture adds a
    /// project-neutral step to the engine history; later changes in the same
    /// gesture replace the step's `after` state. On failure nothing is recorded
    /// (the caller restores the composition).
    pub fn record_composition(
        &mut self,
        session: &mut Session,
        item: ItemId,
        before: Option<Composition>,
        after: Option<Composition>,
        gesture: u64,
    ) -> Result<(), EngineError> {
        if self.open_gesture == Some((gesture, item))
            && let Some(Some(step)) = self.undo.last_mut()
        {
            step.after = after;
            self.redo.clear();
            return Ok(());
        }
        // The composition lives beside the editing project, so this step does
        // not change the project itself: it exists to give the composition
        // snapshot the same place in the one chronological history.
        session.edit(COMPOSITION_LABEL, |_, _| Ok(()))?;
        self.undo.push(Some(CompositionStep { item, before, after }));
        self.redo.clear();
        self.open_gesture = Some((gesture, item));
        self.trim_to_session(session);
        Ok(())
    }

    /// Undo one step. Returns the engine label and the composition step (when
    /// the undone step was a composition edit) for the caller to apply.
    pub fn undo(&mut self, session: &mut Session) -> Option<Undone> {
        let step = self.undo.pop()?;
        let Some(label) = session.undo() else {
            self.undo.push(step);
            return None;
        };
        self.redo.push(step.clone());
        self.open_gesture = None;
        Some(Undone { label, step })
    }

    /// Redo one step (the mirror of [`EditHistory::undo`]).
    pub fn redo(&mut self, session: &mut Session) -> Option<Undone> {
        let step = self.redo.pop()?;
        let Some(label) = session.redo() else {
            self.redo.push(step);
            return None;
        };
        self.undo.push(step.clone());
        self.open_gesture = None;
        self.trim_to_session(session);
        Some(Undone { label, step })
    }

    /// Drop the oldest slots when the engine trimmed its own history at the
    /// limit. Redo can legitimately push the engine past its limit, so this
    /// mirrors the engine's actual length rather than the limit.
    fn trim_to_session(&mut self, session: &Session) {
        let len = session.history.undo.len();
        if self.undo.len() > len {
            self.undo.drain(0..self.undo.len() - len);
        }
    }

    #[cfg(test)]
    fn depths(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

/// Identity of the newest engine undo snapshot, used to tell whether a command
/// actually added a step. The engine's history stores an `Arc<Project>`; two
/// different states have different allocations.
pub fn undo_top(session: &Session) -> Option<usize> {
    session.history.undo.last().map(|(_, project)| Arc::as_ptr(project) as usize)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{CompositionStep, EditHistory, undo_top};
    use filmcraft_engine::Session;
    use filmcraft_project::ItemId;
    use frameforge_bridge::{Composition, EffectLink};

    fn apply(comps: &mut HashMap<ItemId, Composition>, step: &CompositionStep, forward: bool) {
        match if forward { &step.after } else { &step.before } {
            Some(c) => {
                comps.insert(step.item, c.clone());
            }
            None => {
                comps.remove(&step.item);
            }
        }
    }

    fn project_edit(session: &mut Session, history: &mut EditHistory, title: &str) {
        let top = undo_top(session);
        session
            .edit(title, |project, _| {
                project.name = title.to_string();
                Ok(())
            })
            .unwrap();
        history.note_project_step(session, top);
    }

    fn comp_with(enabled: bool) -> Composition {
        Composition { effects: vec![EffectLink { effect: "test".to_string(), enabled, params: HashMap::new() }], ..Composition::default() }
    }

    #[test]
    fn effect_undo_does_not_undo_the_previous_timeline_edit() {
        let mut session = Session::default();
        let mut history = EditHistory::new();
        let mut comps = HashMap::new();
        let item = ItemId(42);

        project_edit(&mut session, &mut history, "timeline one");
        history.record_composition(&mut session, item, None, Some(Composition::default()), 1).unwrap();
        comps.insert(item, Composition::default());

        let undone = history.undo(&mut session).expect("composition step");
        assert!(undone.step.is_some(), "the newest step is the composition edit");
        let step = undone.step.expect("composition step");
        apply(&mut comps, &step, false);
        assert!(!comps.contains_key(&item), "undo restores the missing composition");
        assert_eq!(session.project.name, "timeline one", "the timeline edit is untouched");

        let undone = history.undo(&mut session).expect("timeline step");
        assert!(undone.step.is_none(), "the next step is the timeline edit");
        assert_eq!(session.project.name, "Untitled", "the timeline edit is undone");

        let redone = history.redo(&mut session).expect("timeline redo");
        assert!(redone.step.is_none());
        assert_eq!(session.project.name, "timeline one");
        let redone = history.redo(&mut session).expect("composition redo");
        let step = redone.step.expect("composition redo");
        apply(&mut comps, &step, true);
        assert!(comps.contains_key(&item), "redo restores the composition");
    }

    #[test]
    fn one_drag_is_one_composition_undo_step() {
        let mut session = Session::default();
        let mut history = EditHistory::new();
        let item = ItemId(7);
        let first = comp_with(true);
        let second = comp_with(false);
        let third = comp_with(true);

        let gesture = history.begin_gesture(true);
        history.record_composition(&mut session, item, None, Some(first.clone()), gesture).unwrap();
        history.record_composition(&mut session, item, Some(first), Some(second.clone()), gesture).unwrap();
        history.end_gesture(true);
        history.record_composition(&mut session, item, Some(second), Some(third.clone()), gesture).unwrap();
        history.end_gesture(false);

        assert_eq!(history.depths(), (1, 0), "the whole drag is one step");
        let undone = history.undo(&mut session).expect("composition step");
        let step = undone.step.expect("composition step");
        assert_eq!(step.before, None);
        assert_eq!(step.after, Some(third));
    }

    #[test]
    fn a_new_edit_after_undo_clears_redo() {
        let mut session = Session::default();
        let mut history = EditHistory::new();
        let item = ItemId(8);

        project_edit(&mut session, &mut history, "timeline one");
        history.record_composition(&mut session, item, None, Some(comp_with(true)), 1).unwrap();
        history.undo(&mut session).expect("composition step");
        assert!(history.can_redo());
        assert!(session.history.can_redo());

        history.record_composition(&mut session, item, None, Some(comp_with(false)), 2).unwrap();
        assert!(!history.can_redo(), "a new composition edit clears redo");
        assert!(!session.history.can_redo(), "the engine redo is cleared too");
    }

    #[test]
    fn a_command_that_does_not_edit_adds_no_step() {
        let mut session = Session::default();
        let mut history = EditHistory::new();
        let top = undo_top(&session);

        history.note_project_step(&session, top);
        assert_eq!(history.depths(), (0, 0));
    }

    #[test]
    fn a_merged_engine_edit_adds_no_second_step() {
        let mut session = Session::default();
        let mut history = EditHistory::new();

        let top = undo_top(&session);
        session
            .edit_merged("Fader", "a1-volume", |project, _| {
                project.name = "one".to_string();
                Ok(())
            })
            .unwrap();
        history.note_project_step(&session, top);

        let top = undo_top(&session);
        session
            .edit_merged("Fader", "a1-volume", |project, _| {
                project.name = "two".to_string();
                Ok(())
            })
            .unwrap();
        history.note_project_step(&session, top);

        assert_eq!(history.depths().0, 1, "the engine merged both fader moves into one step");
    }

    #[test]
    fn history_limit_keeps_composition_steps_aligned() {
        let mut session = Session::default();
        session.history.limit = 3;
        let mut history = EditHistory::new();
        let item = ItemId(9);

        for i in 0..5 {
            project_edit(&mut session, &mut history, &format!("timeline {i}"));
            history.record_composition(&mut session, item, None, Some(Composition::default()), i as u64 + 1).unwrap();
            assert_eq!(history.depths().0, session.history.undo.len());
            assert!(history.depths().0 <= 3);
        }

        while history.can_undo() {
            history.undo(&mut session).expect("undo");
        }
        assert!(!session.history.can_undo());
        assert_eq!(history.depths().0, session.history.undo.len());
    }

    #[test]
    fn a_closed_gesture_does_not_swallow_a_later_change() {
        let mut session = Session::default();
        let mut history = EditHistory::new();
        let item = ItemId(10);

        let gesture = history.begin_gesture(true);
        history.record_composition(&mut session, item, None, Some(comp_with(true)), gesture).unwrap();
        history.end_gesture(false);
        history.record_composition(&mut session, item, Some(comp_with(true)), Some(comp_with(false)), gesture).unwrap();
        history.end_gesture(false);

        assert_eq!(history.depths().0, 2, "a keyboard change after the drag is its own step");
    }
}
