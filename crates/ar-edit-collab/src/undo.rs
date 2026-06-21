//! Per-actor undo / redo (SPEC-003 REQ-086).
//!
//! Wraps Loro's `UndoManager`, which records only the **local** peer's changes.
//! `undo()` therefore reverts the local actor's own most recent change and
//! never a change originated by another peer (TEST-105, TEST-106); the inverse
//! change propagates to peers as an ordinary CRDT delta. In a single-actor
//! document this is observationally identical to the old head-pointer
//! undo/redo of [`SPEC-001`] REQ-046/047.
//!
//! ## Operation identity (REQ-090 divergence guard)
//!
//! The session daemon owns ONE undo manager but serves a stream of one-shot CLI
//! commands, direct IPC clients, and its own rollbacks. A bare "undo the last
//! op" — or a non-unique `"<kind>:<shot>"` tag — can revert an operation the
//! caller did not intend (two trims of the same shot share that tag), diverging
//! the live CRDT from the caller's durable edit. Every recorded change therefore
//! carries a globally-unique **operation id** (minted by the caller) and
//! undo/redo are *guarded*: they only fire when the top of the stack carries the
//! caller's exact op id, returning `false` otherwise.
//!
//! ## Stack alignment under eviction
//!
//! The tag stack is kept 1:1 with Loro's undo stack by (a) wrapping each
//! recorded change in a Loro undo group so a multi-commit request is a single
//! undoable step, and (b) detecting *pushes* via Loro's `on_push` callback
//! rather than watching `undo_count()`. At the retention bound Loro evicts the
//! oldest item as it adds a new one, so the count stays constant; a
//! count-growth test would miss that push and the stacks would drift. Counting
//! pushes directly — and evicting the oldest tag in lockstep — keeps the tops
//! aligned (REQ-086).

use crate::crdt::{CollabDoc, COUNTER_ORIGIN};
use loro::{UndoItemMeta, UndoManager, UndoOrRedo};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Retained undo depth. Loro's default is 100; a collaborative editing session
/// can easily exceed that, and silently dropping the oldest steps would let an
/// `undo` succeed on disk while doing nothing live. Kept generous but bounded;
/// the tag stack below evicts in lockstep so the two never drift (REQ-086).
const MAX_UNDO_STEPS: usize = 1000;

/// Local-actor undo/redo over a [`CollabDoc`]. Must be created before the
/// changes it should be able to undo (it tracks the doc from construction).
pub struct LocalUndo {
    mgr: UndoManager,
    /// Count of new-change pushes Loro has made onto the undo stack, maintained
    /// by the `on_push` callback. Used to detect whether a recorded change
    /// actually produced an undoable step even when the bounded stack is full
    /// (where `undo_count()` no longer grows).
    pushes: Arc<AtomicUsize>,
    /// Op-ids, one per Loro undo step, newest last; kept aligned with `mgr`'s
    /// undo stack (including eviction) so [`Self::undo_if`] can match the top.
    undo_tags: Vec<String>,
    /// Op-ids for undone steps available to redo, newest last.
    redo_tags: Vec<String>,
    /// `pushes` captured at [`Self::begin`], to detect a push during the group.
    pending_pushes: usize,
}

impl LocalUndo {
    pub fn new(doc: &CollabDoc) -> Self {
        let mut mgr = UndoManager::new(doc.doc());
        // Id high-water-mark bumps are committed under this origin; never let an
        // undo roll them back (that would reuse a tombstoned id — REQ-080).
        mgr.add_exclude_origin_prefix(COUNTER_ORIGIN);
        // Preserve more than Loro's default 100 steps so a long session's older
        // ops remain undoable instead of silently no-op'ing.
        mgr.set_max_undo_steps(MAX_UNDO_STEPS);

        // Count only `UndoOrRedo::Undo` pushes — i.e. a *new* undoable change
        // (Loro records these with that direction; see record_checkpoint). This
        // fires even when the full stack evicts its oldest item, which a
        // count-based check would miss.
        let pushes = Arc::new(AtomicUsize::new(0));
        let counter = pushes.clone();
        mgr.set_on_push(Some(Box::new(move |kind, _span, _diff| {
            if matches!(kind, UndoOrRedo::Undo) {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            UndoItemMeta::default()
        })));

        Self {
            mgr,
            pushes,
            undo_tags: Vec::new(),
            redo_tags: Vec::new(),
            pending_pushes: 0,
        }
    }

    /// Open a recording group before a mutation. Pair with [`Self::commit`].
    /// Grouping collapses a multi-commit request into ONE undoable step so the
    /// tag stack stays 1:1 with Loro's undo stack.
    pub fn begin(&mut self) {
        self.pending_pushes = self.pushes.load(Ordering::Relaxed);
        let _ = self.mgr.group_start();
    }

    /// Close the group opened by [`Self::begin`]. If it produced a new undoable
    /// step (a push was observed), record `op_id` for it — evicting the oldest
    /// tag in lockstep with Loro's bounded stack — and clear the redo stack (as
    /// any new change does). Returns whether a step was recorded; a no-op
    /// mutation records nothing, keeping the stacks aligned.
    pub fn commit(&mut self, op_id: String) -> bool {
        self.mgr.group_end();
        let recorded = self.pushes.load(Ordering::Relaxed) > self.pending_pushes;
        if recorded {
            self.undo_tags.push(op_id);
            // Mirror Loro's `while len > max { pop_front }` so the tops align.
            while self.undo_tags.len() > MAX_UNDO_STEPS {
                self.undo_tags.remove(0);
            }
            self.redo_tags.clear();
        }
        recorded
    }

    /// Undo the most recent change **iff** its op-id equals `expected`. Returns
    /// whether it undid. A mismatch (unrelated op on top) or an empty stack
    /// (e.g. after a restart) returns `false` without mutating, so the caller
    /// can refuse to commit the other side (REQ-086/090).
    pub fn undo_if(&mut self, expected: &str) -> bool {
        if self.undo_tags.last().map(String::as_str) != Some(expected) {
            return false;
        }
        match self.mgr.undo() {
            Ok(true) => {
                let tag = self.undo_tags.pop().expect("checked above");
                self.redo_tags.push(tag);
                true
            }
            _ => false,
        }
    }

    /// Redo the most recently undone change **iff** its op-id equals `expected`.
    pub fn redo_if(&mut self, expected: &str) -> bool {
        if self.redo_tags.last().map(String::as_str) != Some(expected) {
            return false;
        }
        match self.mgr.redo() {
            Ok(true) => {
                let tag = self.redo_tags.pop().expect("checked above");
                self.undo_tags.push(tag);
                true
            }
            _ => false,
        }
    }

    /// Unguarded undo of the local actor's most recent change (no op-identity
    /// check, tag stacks untouched). For single-actor / non-daemon callers; the
    /// daemon uses [`Self::undo_if`] to avoid reverting an unrelated op.
    pub fn undo(&mut self) -> bool {
        self.mgr.undo().unwrap_or(false)
    }

    /// Unguarded redo counterpart of [`Self::undo`].
    pub fn redo(&mut self) -> bool {
        self.mgr.redo().unwrap_or(false)
    }

    pub fn can_undo(&self) -> bool {
        self.mgr.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.mgr.can_redo()
    }
}
