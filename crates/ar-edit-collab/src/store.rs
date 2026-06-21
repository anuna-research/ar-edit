//! The canonical edit store (SPEC-003 ADR-011 / REQ-079, REQ-086, REQ-088).
//!
//! Completes ADR-011: the [`Loro`](crate) CRDT is the **single** source of truth
//! for an edit, persisted on disk, with the JSON `snapshot`/shots as a *derived*
//! read surface (consumed unchanged by `show`/`validate`/`render`/`--json`).
//!
//! This dissolves the two-store divergence the daemon-coupled guards were built
//! to paper over: there is one store. Durable single-writer undo/redo runs over
//! the oplog via [`CollabDoc::revert_to`] (Phase-0 spike `spike/oq-8-undo`), so
//! `undo`/`redo` work across one-shot CLI invocations with no daemon and no
//! in-memory undo manager. A live session (daemon/peers) still uses the
//! collaborative `UndoManager`; both are attached and durable in the snapshot.
//!
//! ## Daemon op-id passthrough
//!
//! While the daemon still keeps its own live session (until it shares this file,
//! a later phase), each user operation may carry the daemon op-id it was
//! forwarded under. The id is recorded *on the cursor checkpoint* — so a later
//! `undo`/`redo` can name the exact live op to revert, surviving across one-shot
//! invocations without the per-op `EditOp` field the event-sourced path needed.

use crate::crdt::CollabDoc;
use crate::ids::ActorId;
use crate::materialise::materialise;
use crate::migrate::MIGRATION_ACTOR;
use ar_edit_core::edit::{validate_range, EditError};
use ar_edit_core::models::{EditDocument, EditOpKind, EditSnapshot, ShotNote, ShotRange};
use base64::Engine;
use chrono::{DateTime, Utc};

/// One user-visible state on the undo cursor: the CRDT checkpoint plus the
/// daemon op-id the operation that produced it was forwarded under (if any).
#[derive(Clone)]
struct Checkpoint {
    frontier: Vec<u8>,
    op_id: Option<String>,
}

/// On-disk checkpoint (base64 frontier + optional op-id).
#[derive(serde::Serialize, serde::Deserialize)]
struct DiskCheckpoint {
    frontier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    op_id: Option<String>,
}

/// On-disk envelope: the canonical CRDT payload plus the durable undo cursor,
/// alongside a materialised `snapshot` mirror so a plain reader still sees the
/// shot list. base64 keeps the binary CRDT/frontiers readable in the JSON file.
#[derive(serde::Serialize, serde::Deserialize)]
struct OnDisk {
    name: String,
    created: DateTime<Utc>,
    actor: u64,
    /// base64 Loro snapshot (canonical state).
    crdt: String,
    /// Cursor checkpoints, newest last; `[0]` is the empty baseline.
    undo_history: Vec<DiskCheckpoint>,
    /// Index into `undo_history` of the current visible state.
    undo_head: usize,
    /// Minted-id counter high-water marks (shot, note, orset). Persisted so a
    /// reload never re-mints a tombstoned id even when undo (`revert_to`) rolled
    /// back the in-document counter map (REQ-080). Defaulted for old files.
    #[serde(default)]
    counters: (u64, u64, u64),
    /// Materialised read view (regenerated on every save).
    snapshot: EditSnapshot,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// The canonical, CRDT-backed edit. Holds the live [`CollabDoc`] and a durable
/// head-over-oplog undo cursor.
pub struct PersistentEdit {
    name: String,
    created: DateTime<Utc>,
    doc: CollabDoc,
    /// One checkpoint per user-visible state, newest last; `[0]` empty baseline.
    history: Vec<Checkpoint>,
    /// Index of the current visible state in `history`.
    head: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(String),
    #[error("corrupt edit store: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Edit(#[from] EditError),
}

impl PersistentEdit {
    /// A new, empty edit owned by `actor`.
    pub fn create(name: impl Into<String>, actor: ActorId) -> Self {
        let doc = CollabDoc::new(actor);
        let baseline = Checkpoint {
            frontier: doc.checkpoint(),
            op_id: None,
        };
        Self {
            name: name.into(),
            created: Utc::now(),
            doc,
            history: vec![baseline],
            head: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The materialised shot list (REQ-079).
    pub fn snapshot(&self) -> EditSnapshot {
        materialise(&self.doc)
    }

    /// The current CRDT version frontier (opaque bytes). Equal before and after a
    /// no-op mutation — used to persist only on a real change.
    pub fn frontier(&self) -> Vec<u8> {
        self.doc.checkpoint()
    }

    /// A snapshot-only [`EditDocument`] view for the read-side commands
    /// (`show`/`validate`/`render`/`--json`) — they consume `snapshot.shots`.
    pub fn to_edit_document(&self) -> EditDocument {
        let mut ed = crate::materialise::to_edit_document(&self.doc, self.name.clone());
        ed.created = self.created;
        ed
    }

    pub fn can_undo(&self) -> bool {
        self.head > 0
    }

    pub fn can_redo(&self) -> bool {
        self.head + 1 < self.history.len()
    }

    /// The daemon op-id of the operation that produced the current state — i.e.
    /// the op an `undo` would revert (forwarded to the live daemon as the tag).
    pub fn current_op_id(&self) -> Option<&str> {
        self.history[self.head].op_id.as_deref()
    }

    /// Record a checkpoint after a mutation, truncating any redo branch. A no-op
    /// mutation (e.g. move/remove of a missing shot) leaves the document frontier
    /// unchanged — it records nothing, so undo never has to step over a phantom.
    fn checkpoint(&mut self, op_id: Option<String>) {
        let frontier = self.doc.checkpoint();
        if frontier == self.history[self.head].frontier {
            return;
        }
        self.history.truncate(self.head + 1);
        self.history.push(Checkpoint { frontier, op_id });
        self.head += 1;
    }

    /// Append a shot with a freshly-minted, globally-unique id (REQ-080); returns
    /// the id. `op_id` is the daemon tag to record for undo, if forwarded.
    pub fn add_shot(
        &mut self,
        source: &str,
        range: ShotRange,
        op_id: Option<String>,
        author: &str,
    ) -> Result<String, StoreError> {
        validate_range(&range)?;
        let id = self.doc.add_new_shot(source, &range, author);
        self.checkpoint(op_id);
        Ok(id)
    }

    /// Relocate a shot (REQ-080: identity-preserving move).
    pub fn move_shot(&mut self, shot_id: &str, to: usize, op_id: Option<String>) {
        self.doc.move_shot(shot_id, to);
        self.checkpoint(op_id);
    }

    /// Replace a shot's range (REQ-081), validated first.
    pub fn trim_shot(
        &mut self,
        shot_id: &str,
        range: ShotRange,
        op_id: Option<String>,
    ) -> Result<(), StoreError> {
        validate_range(&range)?;
        self.doc.trim_shot(shot_id, &range);
        self.checkpoint(op_id);
        Ok(())
    }

    /// Remove a shot (REQ-082 observed-remove).
    pub fn remove_shot(&mut self, shot_id: &str, op_id: Option<String>) {
        self.doc.remove_shot(shot_id);
        self.checkpoint(op_id);
    }

    /// Append a note (REQ-082 grow-only).
    pub fn add_note(&mut self, shot_id: &str, note: &ShotNote, op_id: Option<String>) {
        self.doc.add_note(shot_id, note);
        self.checkpoint(op_id);
    }

    /// Whether a shot with `shot_id` is currently present.
    pub fn has_shot(&self, shot_id: &str) -> bool {
        self.snapshot().shots.iter().any(|s| s.id == shot_id)
    }

    /// Append a note from `text` (timestamped now); returns it. Keeps `chrono`
    /// out of one-shot callers.
    pub fn add_note_text(
        &mut self,
        shot_id: &str,
        text: &str,
        op_id: Option<String>,
        author: &str,
    ) -> ShotNote {
        let note = ShotNote {
            text: text.to_string(),
            author: author.to_string(),
            created: Utc::now(),
        };
        self.doc.add_note(shot_id, &note);
        self.checkpoint(op_id);
        note
    }

    /// Durably undo the most recent local change; returns whether anything was
    /// undone. Survives a save/reload (the revert is recorded in the snapshot).
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        // A checkpoint frontier that is not in this doc (corrupt/hand-edited
        // file) makes revert fail — return false rather than panic the CLI.
        if self
            .doc
            .revert_to(&self.history[self.head - 1].frontier)
            .is_err()
        {
            return false;
        }
        self.head -= 1;
        true
    }

    /// Durably redo the most recently undone change.
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        if self
            .doc
            .revert_to(&self.history[self.head + 1].frontier)
            .is_err()
        {
            return false;
        }
        self.head += 1;
        true
    }

    /// Serialise the canonical store.
    pub fn to_bytes(&self) -> Vec<u8> {
        let on_disk = OnDisk {
            name: self.name.clone(),
            created: self.created,
            actor: self.doc.actor().0,
            crdt: b64().encode(self.doc.export_snapshot()),
            undo_history: self
                .history
                .iter()
                .map(|c| DiskCheckpoint {
                    frontier: b64().encode(&c.frontier),
                    op_id: c.op_id.clone(),
                })
                .collect(),
            undo_head: self.head,
            counters: self.doc.counter_hwm(),
            snapshot: self.snapshot(),
        };
        serde_json::to_vec_pretty(&on_disk).expect("edit store serialises")
    }

    /// Parse a canonical store; or transparently migrate a legacy event-sourced
    /// [`EditDocument`] JSON (REQ-088) into the CRDT under `actor`.
    pub fn from_bytes(bytes: &[u8], actor: ActorId) -> Result<Self, StoreError> {
        // New canonical format first.
        if let Ok(on_disk) = serde_json::from_slice::<OnDisk>(bytes) {
            if !on_disk.crdt.is_empty() {
                let _ = actor; // the on-disk format carries its own actor
                return Self::from_on_disk(on_disk);
            }
        }
        // Legacy event-sourced edit document → migrate (REQ-088).
        let legacy: EditDocument = serde_json::from_slice(bytes)
            .map_err(|e| StoreError::Corrupt(format!("not a known edit format: {e}")))?;
        Ok(Self::migrate_legacy(legacy, actor))
    }

    fn from_on_disk(on_disk: OnDisk) -> Result<Self, StoreError> {
        let doc = CollabDoc::new(ActorId(on_disk.actor));
        let crdt = b64()
            .decode(on_disk.crdt.as_bytes())
            .map_err(|e| StoreError::Corrupt(format!("crdt base64: {e}")))?;
        doc.import(&crdt)
            .map_err(|e| StoreError::Corrupt(format!("crdt import: {e}")))?;
        // Restore the true minted-id counter marks: `import` reseeds them from
        // the snapshot, which undo may have reverted below the high-water mark
        // (REQ-080). This must run AFTER import.
        doc.restore_counter_hwm(on_disk.counters);
        let mut history = on_disk
            .undo_history
            .into_iter()
            .map(|d| {
                Ok(Checkpoint {
                    frontier: b64()
                        .decode(d.frontier.as_bytes())
                        .map_err(|e| StoreError::Corrupt(format!("history base64: {e}")))?,
                    op_id: d.op_id,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        if history.is_empty() {
            // Corrupt/hand-edited file with a valid crdt but no cursor: baseline
            // at THIS doc's current frontier (not a fresh foreign doc's), so the
            // baseline is a real, in-oplog version.
            history.push(Checkpoint {
                frontier: doc.checkpoint(),
                op_id: None,
            });
        }
        let head = on_disk.undo_head.min(history.len() - 1);
        Ok(Self {
            name: on_disk.name,
            created: on_disk.created,
            doc,
            history,
            head,
        })
    }

    /// Migrate a legacy event-sourced document into the CRDT, reconstructing the
    /// undo cursor by replaying `ops[0..=head]` (so `undo` keeps working after
    /// migration; the redo'd tail beyond `head` is dropped). Snapshot-identical
    /// for the visible state (REQ-088).
    ///
    /// The migration ops are written under the fixed [`MIGRATION_ACTOR`] so two
    /// peers migrating the same legacy edit independently produce *identical*
    /// Loro ops (idempotent on merge — no duplicated shots, REQ-088); the doc is
    /// then re-keyed to the caller's `actor` so subsequent edits carry a unique
    /// peer id. Recorded checkpoint frontiers stay valid across the rekey (their
    /// ops remain in the oplog).
    fn migrate_legacy(legacy: EditDocument, actor: ActorId) -> Self {
        let doc = CollabDoc::new(MIGRATION_ACTOR);
        let mut history = vec![Checkpoint {
            frontier: doc.checkpoint(),
            op_id: None,
        }];
        if legacy.head >= 0 && !legacy.ops.is_empty() {
            // Replay the visible op log to reconstruct the undo cursor. Skip a
            // replayed op that doesn't change the document (e.g. a legacy
            // remove/trim of an absent id) so migration records no phantom step
            // — same no-op contract as `checkpoint`.
            for op in legacy.ops.iter().take((legacy.head + 1) as usize) {
                replay(&doc, &op.op);
                let frontier = doc.checkpoint();
                if frontier != history.last().expect("baseline present").frontier {
                    history.push(Checkpoint {
                        frontier,
                        op_id: None,
                    });
                }
            }
        } else if !legacy.snapshot.shots.is_empty() {
            // Snapshot-only document (no op log): seed the materialised state.
            // No undo history to reconstruct — re-baseline at the seeded state.
            for shot in &legacy.snapshot.shots {
                doc.add_shot(shot);
            }
            history = vec![Checkpoint {
                frontier: doc.checkpoint(),
                op_id: None,
            }];
        }
        // Re-key to the caller's actor for subsequent (collaborative) edits.
        let _ = doc.rekey_actor(actor);
        let head = history.len() - 1;
        Self {
            name: legacy.name,
            created: legacy.created,
            doc,
            history,
            head,
        }
    }
}

/// Apply one event-sourced op to the CRDT (migration replay). Ids are preserved
/// (`CollabDoc::add_shot` keeps a free id), so move/trim/remove still target them.
fn replay(doc: &CollabDoc, op: &EditOpKind) {
    match op {
        EditOpKind::AddShot { shot } => doc.add_shot(shot),
        EditOpKind::RemoveShot { shot_id, .. } => doc.remove_shot(shot_id),
        EditOpKind::MoveShot {
            shot_id,
            to_position,
            ..
        } => doc.move_shot(shot_id, *to_position as usize),
        EditOpKind::TrimShot {
            shot_id, new_range, ..
        }
        | EditOpKind::ReplaceRangeType {
            shot_id, new_range, ..
        } => doc.trim_shot(shot_id, new_range),
        EditOpKind::AddNote { shot_id, note } => doc.add_note(shot_id, note),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(from: usize, to: usize) -> ShotRange {
        ShotRange::Words {
            from: from as u32,
            to: to as u32,
        }
    }

    fn ids(e: &PersistentEdit) -> Vec<String> {
        e.snapshot().shots.into_iter().map(|s| s.id).collect()
    }

    // --- Behavioural mutation-coverage tests: each method does its job, not a
    // --- shape-preserving no-op (a no-op'd mutation must be observable). ---

    #[test]
    fn move_reorders_the_shot_list() {
        let mut e = PersistentEdit::create("p", ActorId(1));
        let a = e.add_shot("s1", words(0, 5), None, "").unwrap();
        let b = e.add_shot("s2", words(0, 5), None, "").unwrap();
        let c = e.add_shot("s3", words(0, 5), None, "").unwrap();
        assert_eq!(ids(&e), vec![a.clone(), b.clone(), c.clone()]);
        e.move_shot(&a, 2, None);
        assert_eq!(
            ids(&e),
            vec![b, c, a],
            "move reorders, not just preserves the set"
        );
    }

    #[test]
    fn remove_deletes_exactly_the_target() {
        let mut e = PersistentEdit::create("p", ActorId(1));
        let a = e.add_shot("s1", words(0, 5), None, "").unwrap();
        let b = e.add_shot("s2", words(0, 5), None, "").unwrap();
        e.remove_shot(&a, None);
        assert_eq!(ids(&e), vec![b], "remove deletes the target shot");
    }

    #[test]
    fn add_note_appears_on_the_shot() {
        let mut e = PersistentEdit::create("p", ActorId(1));
        let a = e.add_shot("s1", words(0, 5), None, "").unwrap();
        let note = ShotNote {
            author: String::new(),
            text: "hello".into(),
            created: Utc::now(),
        };
        e.add_note(&a, &note, None);
        let shot = e.snapshot().shots.into_iter().find(|s| s.id == a).unwrap();
        assert_eq!(shot.notes.len(), 1);
        assert_eq!(shot.notes[0].text, "hello");
    }

    #[test]
    fn has_shot_distinguishes_present_from_absent() {
        let mut e = PersistentEdit::create("p", ActorId(1));
        let a = e.add_shot("s1", words(0, 5), None, "").unwrap();
        assert!(e.has_shot(&a), "present shot");
        assert!(!e.has_shot("shot-nope"), "absent shot");
    }

    #[test]
    fn redo_restores_the_exact_prior_state() {
        let mut e = PersistentEdit::create("p", ActorId(1));
        e.add_shot("s1", words(0, 5), None, "").unwrap();
        let two = e.add_shot("s2", words(0, 5), None, "").unwrap();
        let full = ids(&e);
        assert!(e.undo());
        assert_eq!(ids(&e).len(), 1, "undo dropped the second shot");
        assert!(e.redo());
        assert_eq!(
            ids(&e),
            full,
            "redo restored the exact prior list (not the current state)"
        );
        assert!(e.has_shot(&two));
    }

    #[test]
    fn snapshot_only_legacy_migrates_its_shots() {
        // A legacy doc with shots in its snapshot but an EMPTY op log (head >= 0)
        // must seed the shots from the snapshot, not the (empty) replay branch
        // — guards migrate_legacy's `head >= 0 && !ops.is_empty()` condition.
        use ar_edit_core::models::Shot;
        let mk = |id: &str, src: &str| Shot {
            author: String::new(),
            id: id.into(),
            source: src.into(),
            range: words(0, 10),
            notes: vec![],
        };
        let mut legacy = EditDocument::create("e");
        legacy.head = 0; // >= 0, so the && short-circuits on empty ops
        legacy.snapshot = EditSnapshot {
            shots: vec![mk("shot-001", "s1"), mk("shot-002", "s2")],
        };

        let e =
            PersistentEdit::from_bytes(&serde_json::to_vec(&legacy).unwrap(), ActorId(1)).unwrap();
        assert_eq!(
            ids(&e).len(),
            2,
            "snapshot-only migration seeded both shots"
        );
    }

    #[test]
    fn oversized_undo_head_is_clamped_on_load() {
        // Poison a valid store's undo_head; from_on_disk must clamp it to the
        // last valid index (history.len() - 1), not overrun the history.
        let mut e = PersistentEdit::create("p", ActorId(1));
        e.add_shot("s1", words(0, 5), None, "").unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&e.to_bytes()).unwrap();
        v["undo_head"] = serde_json::json!(9999);
        let mut e2 =
            PersistentEdit::from_bytes(&serde_json::to_vec(&v).unwrap(), ActorId(1)).unwrap();
        assert!(!e2.can_redo(), "clamped to the tip — nothing to redo");
        assert!(e2.undo(), "head is a valid index, so undo works");
        assert_eq!(
            ids(&e2).len(),
            0,
            "undo from the clamped tip reaches the empty baseline"
        );
    }

    #[test]
    fn frontier_tracks_real_changes_only() {
        let mut e = PersistentEdit::create("p", ActorId(1));
        let f0 = e.frontier();
        e.add_shot("s1", words(0, 5), None, "").unwrap();
        let f1 = e.frontier();
        assert_ne!(f0, f1, "a real add advances the frontier");
        e.remove_shot("shot-absent", None); // no-op
        assert_eq!(e.frontier(), f1, "a no-op leaves the frontier unchanged");
    }

    /// Regression (REQ-080): a cursor undo reverts the in-document counter map,
    /// so without persisting the counter high-water marks a reload re-minted the
    /// tombstoned id. The id must stay unique across undo + reload.
    #[test]
    fn no_id_reuse_after_undo_and_reload() {
        let mut e = PersistentEdit::create("p", ActorId(0x42));
        let id1 = e.add_shot("src-001", words(0, 10), None, "").unwrap();
        assert!(e.undo());
        let mut e = PersistentEdit::from_bytes(&e.to_bytes(), ActorId(0x42)).unwrap();
        let id2 = e.add_shot("src-002", words(0, 10), None, "").unwrap();
        assert_ne!(
            id1, id2,
            "a tombstoned id must never be re-minted after undo + reload"
        );
    }

    /// Regression (REQ-088): two peers migrating the SAME legacy edit
    /// independently must converge — the migration ops are written under the
    /// fixed MIGRATION_ACTOR, so a CRDT merge dedups them instead of duplicating
    /// every shot.
    #[test]
    fn migration_is_idempotent_across_peers() {
        let mut legacy = EditDocument::create("e");
        legacy.add_shot("src-001", words(0, 10)).unwrap();
        legacy.add_shot("src-002", words(0, 10)).unwrap();
        let bytes = serde_json::to_vec(&legacy).unwrap();

        let a = PersistentEdit::from_bytes(&bytes, ActorId(10)).unwrap();
        let b = PersistentEdit::from_bytes(&bytes, ActorId(20)).unwrap();
        // Merge b's state into a, as a CRDT sync would.
        a.doc.import(&b.doc.export_snapshot()).unwrap();
        assert_eq!(
            ids(&a).len(),
            2,
            "independent migrations converge — no duplicated shots"
        );
    }

    /// Regression: a no-op mutation (move/remove of a missing shot) must not
    /// record a phantom undo step — one undo reverts the one real edit.
    #[test]
    fn no_op_mutation_records_no_undo_step() {
        let mut e = PersistentEdit::create("p", ActorId(7));
        e.add_shot("src-001", words(0, 10), None, "").unwrap();
        e.remove_shot("does-not-exist", None);
        e.move_shot("does-not-exist", 0, None);
        assert!(e.undo(), "one undo available");
        assert_eq!(
            ids(&e).len(),
            0,
            "a single undo reverts the real add (no phantom steps)"
        );
        assert!(!e.undo(), "nothing more to undo");
    }

    #[test]
    fn durable_undo_redo_round_trips_through_persistence() {
        let mut e = PersistentEdit::create("rough-cut", ActorId(3));
        e.add_shot("src-001", words(0, 10), None, "").unwrap();
        let b = e.add_shot("src-002", words(0, 10), None, "").unwrap();
        e.add_shot("src-003", words(0, 10), None, "").unwrap();
        assert_eq!(ids(&e).len(), 3);

        assert!(e.undo());
        assert!(e.undo());
        assert_eq!(ids(&e).len(), 1);

        // Persist + reload (a one-shot CLI "restart").
        let bytes = e.to_bytes();
        let mut e = PersistentEdit::from_bytes(&bytes, ActorId(3)).unwrap();
        assert_eq!(ids(&e).len(), 1, "undo survived the round-trip");

        assert!(e.redo());
        assert!(e.redo());
        assert!(ids(&e).contains(&b), "redo restores the same shot id");

        // Edit after undo truncates redo.
        assert!(e.undo());
        e.add_shot("src-004", words(0, 10), None, "").unwrap();
        assert!(!e.redo(), "a new edit after undo truncates redo");
    }

    #[test]
    fn op_ids_recorded_on_cursor_survive_persistence() {
        let mut e = PersistentEdit::create("rc", ActorId(4));
        e.add_shot("src-001", words(0, 10), Some("op-1".into()), "")
            .unwrap();
        let id = e
            .add_shot("src-002", words(0, 10), Some("op-2".into()), "")
            .unwrap();
        // The op an undo would revert reports its daemon tag for forwarding.
        assert_eq!(e.current_op_id(), Some("op-2"));

        let e = PersistentEdit::from_bytes(&e.to_bytes(), ActorId(4)).unwrap();
        assert_eq!(e.current_op_id(), Some("op-2"), "op-id survived reload");
        let _ = id;
    }

    #[test]
    fn rejects_invalid_range() {
        let mut e = PersistentEdit::create("rc", ActorId(1));
        assert!(matches!(
            e.add_shot("src-001", words(5, 5), None, ""),
            Err(StoreError::Edit(_))
        ));
        assert_eq!(ids(&e).len(), 0, "no shot added on invalid range");
        let id = e.add_shot("src-001", words(0, 10), None, "").unwrap();
        assert!(matches!(
            e.trim_shot(&id, words(10, 3), None),
            Err(StoreError::Edit(_))
        ));
    }

    #[test]
    fn migrates_legacy_and_preserves_undo() {
        // A legacy edit with three shots in its event-sourced log.
        let mut legacy = EditDocument::create("legacy");
        legacy.add_shot("src-001", words(0, 10)).unwrap();
        legacy.add_shot("src-002", words(0, 10)).unwrap();
        legacy.add_shot("src-003", words(0, 10)).unwrap();
        let bytes = serde_json::to_vec(&legacy).unwrap();

        let mut e = PersistentEdit::from_bytes(&bytes, ActorId(9)).unwrap();
        assert_eq!(e.name(), "legacy");
        assert_eq!(
            e.snapshot().shots.len(),
            3,
            "migration is snapshot-identical"
        );
        // Undo history was reconstructed from the legacy op log.
        assert!(e.can_undo(), "migrated doc can undo its replayed ops");
        assert!(e.undo());
        assert_eq!(e.snapshot().shots.len(), 2, "undo after migration works");
    }
}
