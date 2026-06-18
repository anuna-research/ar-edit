//! Per-actor undo / redo (SPEC-003 REQ-086).
//!
//! Wraps Loro's `UndoManager`, which records only the **local** peer's changes.
//! `undo()` therefore reverts the local actor's own most recent change and
//! never a change originated by another peer (TEST-105, TEST-106); the inverse
//! change propagates to peers as an ordinary CRDT delta. In a single-actor
//! document this is observationally identical to the old head-pointer
//! undo/redo of [`SPEC-001`] REQ-046/047.

use crate::crdt::CollabDoc;
use loro::UndoManager;

/// Local-actor undo/redo over a [`CollabDoc`]. Must be created before the
/// changes it should be able to undo (it tracks the doc from construction).
pub struct LocalUndo {
    mgr: UndoManager,
}

impl LocalUndo {
    pub fn new(doc: &CollabDoc) -> Self {
        Self {
            mgr: UndoManager::new(doc.doc()),
        }
    }

    /// Undo the local actor's most recent change. Returns whether anything was
    /// undone.
    pub fn undo(&mut self) -> bool {
        self.mgr.undo().unwrap_or(false)
    }

    /// Redo the most recently undone local change.
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
