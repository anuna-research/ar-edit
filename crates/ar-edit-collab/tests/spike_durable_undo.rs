//! SPIKE (SPEC-003 OQ-8 / ADR-011, Phase 0): durable single-writer undo/redo
//! over the Loro oplog, WITHOUT a daemon, surviving a persistence round-trip.
//!
//! Question this answers: can the unified single-store on-disk format support
//! `ar-edit undo` / `redo` across one-shot CLI invocations using only what is
//! persisted (CRDT snapshot + an undo cursor), with no in-memory undo manager?
//!
//! Mechanism: model `head` as a cursor over checkpoints (Loro frontiers, stored
//! as opaque bytes) and move between them with `CollabDoc::revert_to`, which
//! generates *local* ops to reach a target version. Because the move is itself
//! recorded in the oplog, the doc stays attached/editable (unlike `checkout`,
//! which detaches), undo is durable in the snapshot, and it would propagate to
//! peers as an ordinary CRDT update (REQ-086). The persisted state is exactly
//! `(snapshot_bytes, Vec<checkpoint_bytes>, head)` — what a unified on-disk edit
//! file would carry.

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::materialise::materialise;
use ar_edit_core::models::ShotRange;

/// A durable edit "session" = the CRDT plus the persisted undo cursor. `history`
/// holds one checkpoint per user-visible state, with `history[0]` the empty
/// baseline; `head` indexes the current state.
struct Session {
    doc: CollabDoc,
    history: Vec<Vec<u8>>,
    head: usize,
}

impl Session {
    fn new(actor: ActorId) -> Self {
        let doc = CollabDoc::new(actor);
        let history = vec![doc.checkpoint()]; // empty baseline
        Self {
            doc,
            history,
            head: 0,
        }
    }

    /// Append a shot and record a checkpoint, truncating any redo branch.
    fn add(&mut self, source: &str) -> String {
        let id = self
            .doc
            .add_new_shot(source, &ShotRange::Words { from: 0, to: 10 }, "");
        self.history.truncate(self.head + 1);
        self.history.push(self.doc.checkpoint());
        self.head += 1;
        id
    }

    fn undo(&mut self) -> bool {
        if self.head == 0 {
            return false;
        }
        self.doc.revert_to(&self.history[self.head - 1]).unwrap();
        self.head -= 1;
        true
    }

    fn redo(&mut self) -> bool {
        if self.head + 1 >= self.history.len() {
            return false;
        }
        self.doc.revert_to(&self.history[self.head + 1]).unwrap();
        self.head += 1;
        true
    }

    fn ids(&self) -> Vec<String> {
        materialise(&self.doc)
            .shots
            .into_iter()
            .map(|s| s.id)
            .collect()
    }

    /// Serialise everything an on-disk edit file would hold.
    fn persist(&self) -> (Vec<u8>, Vec<Vec<u8>>, usize) {
        (self.doc.export_snapshot(), self.history.clone(), self.head)
    }

    /// Reload from persisted bytes — the one-shot "process restart".
    fn restore(actor: ActorId, snapshot: &[u8], history: Vec<Vec<u8>>, head: usize) -> Self {
        let doc = CollabDoc::new(actor);
        doc.import(snapshot).unwrap();
        Self { doc, history, head }
    }
}

/// The whole Phase-0 question in one test: durable undo/redo with no daemon,
/// surviving a persistence round-trip, with edit-after-undo truncating redo and
/// the document staying attached throughout.
#[test]
fn durable_undo_redo_survives_persistence() {
    let actor = ActorId(42);
    let mut s = Session::new(actor);
    let _a = s.add("src-001");
    let b = s.add("src-002");
    let _c = s.add("src-003");
    assert_eq!(s.ids().len(), 3);
    assert!(
        s.doc.is_attached(),
        "revert-based undo keeps the doc attached/editable"
    );

    // Undo twice (→ 1 shot), then persist and "restart" the process.
    assert!(s.undo());
    assert!(s.undo());
    assert_eq!(s.ids().len(), 1);

    let (snap, hist, head) = s.persist();
    drop(s);
    let mut s = Session::restore(actor, &snap, hist, head);

    // The undo SURVIVED the round-trip — durable undo with no in-memory manager.
    assert_eq!(s.ids().len(), 1, "undone state persisted across restart");

    // Redo restores the shots — and the SAME minted ids come back (it is the
    // identical CRDT state, reached by reverting forward).
    assert!(s.redo());
    assert!(s.redo());
    let ids = s.ids();
    assert_eq!(ids.len(), 3);
    assert!(
        ids.contains(&b),
        "redo restores the exact same shot id: {ids:?}"
    );

    // Undo then a NEW edit truncates the redo branch (event-sourced parity with
    // SPEC-001 fork behaviour).
    assert!(s.undo());
    assert_eq!(s.ids().len(), 2);
    s.add("src-004");
    assert_eq!(s.ids().len(), 3);
    assert!(!s.redo(), "a new edit after undo truncates redo");
    assert!(
        s.doc.is_attached(),
        "still attached after the whole sequence"
    );
}

/// A second persistence cycle: undo must remain durable across MULTIPLE
/// save/reload boundaries (the realistic one-shot-CLI cadence: each command is a
/// fresh process), and a checkpoint recorded in one process must be a valid
/// revert target in a later one.
#[test]
fn undo_cursor_stable_across_multiple_restarts() {
    let actor = ActorId(7);

    // Process 1: two adds.
    let mut s = Session::new(actor);
    s.add("src-001");
    s.add("src-002");
    let (snap, hist, head) = s.persist();
    assert_eq!(s.ids().len(), 2);

    // Process 2: load, undo one, save.
    let mut s = Session::restore(actor, &snap, hist, head);
    assert!(s.undo());
    assert_eq!(s.ids().len(), 1);
    let (snap, hist, head) = s.persist();

    // Process 3: load, redo one (a checkpoint from process 1, reverted to in 3).
    let mut s = Session::restore(actor, &snap, hist, head);
    assert_eq!(s.ids().len(), 1, "undo from process 2 persisted");
    assert!(
        s.redo(),
        "a checkpoint from an earlier process is a valid revert target"
    );
    assert_eq!(s.ids().len(), 2);
}
