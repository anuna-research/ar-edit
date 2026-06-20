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
//! op" can therefore revert an operation the caller did not intend (e.g. a
//! direct-IPC mutation sitting on top of the stack), diverging the live CRDT
//! from the caller's durable, event-sourced edit. To prevent that, every
//! recorded change carries an **op-identity tag** and undo/redo are *guarded*:
//! they only fire when the top of the stack matches the caller's expected tag,
//! returning `false` otherwise so the caller can refuse to commit one side.
//!
//! The tag stack is kept 1:1 with Loro's own undo stack by wrapping each
//! recorded change in a Loro undo group (so a multi-commit request is a single
//! undoable step) and only pushing a tag when a step was actually recorded.

use crate::crdt::{CollabDoc, COUNTER_ORIGIN};
use loro::UndoManager;

/// Retained undo depth. Loro's default is 100; a collaborative editing session
/// can easily exceed that, and silently dropping the oldest steps would let an
/// `undo` succeed on disk while doing nothing live. Kept generous but bounded;
/// the tag stack below is capped to match so the two never drift (REQ-086).
const MAX_UNDO_STEPS: usize = 1000;

/// Local-actor undo/redo over a [`CollabDoc`]. Must be created before the
/// changes it should be able to undo (it tracks the doc from construction).
pub struct LocalUndo {
    mgr: UndoManager,
    /// Op-identity tags, one per Loro undo step, newest last. Kept aligned with
    /// `mgr`'s undo stack so [`Self::undo_if`] can verify the top before acting.
    undo_tags: Vec<String>,
    /// Tags for undone steps available to redo, newest last (mirrors `mgr`'s
    /// redo stack; cleared when a new change is recorded, as Loro does).
    redo_tags: Vec<String>,
    /// `undo_count()` captured at [`Self::begin`], to detect whether the grouped
    /// change actually produced an undoable step.
    pending_pre_count: usize,
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
        Self {
            mgr,
            undo_tags: Vec::new(),
            redo_tags: Vec::new(),
            pending_pre_count: 0,
        }
    }

    /// Open a recording group before a mutation. Pair with [`Self::commit`].
    /// Grouping collapses a multi-commit request into ONE undoable step so the
    /// tag stack stays 1:1 with Loro's undo stack.
    pub fn begin(&mut self) {
        self.pending_pre_count = self.mgr.undo_count();
        let _ = self.mgr.group_start();
    }

    /// Close the group opened by [`Self::begin`] and, if it produced a new
    /// undoable step, record `tag` for it (clearing the redo stack, as any new
    /// change does). A no-op mutation records nothing, keeping the stacks aligned.
    pub fn commit(&mut self, tag: String) {
        self.mgr.group_end();
        if self.mgr.undo_count() > self.pending_pre_count {
            self.undo_tags.push(tag);
            // Match Loro's own eviction so the tag stack never outgrows it.
            if self.undo_tags.len() > MAX_UNDO_STEPS {
                self.undo_tags.remove(0);
            }
            self.redo_tags.clear();
        }
    }

    /// Undo the most recent change **iff** its tag equals `expected`. Returns
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

    /// Redo the most recently undone change **iff** its tag equals `expected`.
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

    /// The tag of the change a guard-free undo would revert (newest), if any.
    pub fn top_undo_tag(&self) -> Option<&str> {
        self.undo_tags.last().map(String::as_str)
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
