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

use crate::crdt::CollabDoc;
use crate::ids::ActorId;
use crate::materialise::materialise;
use crate::migrate;
use ar_edit_core::edit::{validate_range, EditError};
use ar_edit_core::models::{EditDocument, EditSnapshot, ShotNote, ShotRange};
use base64::Engine;
use chrono::{DateTime, Utc};

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
    /// base64 frontier checkpoints, newest last; `[0]` is the empty baseline.
    undo_history: Vec<String>,
    /// Index into `undo_history` of the current visible state.
    undo_head: usize,
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
    history: Vec<Vec<u8>>,
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
        let baseline = doc.checkpoint();
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

    /// Record a checkpoint after a mutation, truncating any redo branch.
    fn checkpoint(&mut self) {
        self.history.truncate(self.head + 1);
        self.history.push(self.doc.checkpoint());
        self.head += 1;
    }

    /// Append a shot with a freshly-minted, globally-unique id (REQ-080); returns
    /// the id. Validates the range first (CON-018 / event-sourced parity).
    pub fn add_shot(&mut self, source: &str, range: ShotRange) -> Result<String, StoreError> {
        validate_range(&range)?;
        let id = self.doc.add_new_shot(source, &range);
        self.checkpoint();
        Ok(id)
    }

    /// Relocate a shot (REQ-080: identity-preserving move).
    pub fn move_shot(&mut self, shot_id: &str, to: usize) {
        self.doc.move_shot(shot_id, to);
        self.checkpoint();
    }

    /// Replace a shot's range (REQ-081), validated first.
    pub fn trim_shot(&mut self, shot_id: &str, range: ShotRange) -> Result<(), StoreError> {
        validate_range(&range)?;
        self.doc.trim_shot(shot_id, &range);
        self.checkpoint();
        Ok(())
    }

    /// Remove a shot (REQ-082 observed-remove).
    pub fn remove_shot(&mut self, shot_id: &str) {
        self.doc.remove_shot(shot_id);
        self.checkpoint();
    }

    /// Append a note (REQ-082 grow-only).
    pub fn add_note(&mut self, shot_id: &str, note: &ShotNote) {
        self.doc.add_note(shot_id, note);
        self.checkpoint();
    }

    /// Durably undo the most recent local change; returns whether anything was
    /// undone. Survives a save/reload (the revert is recorded in the snapshot).
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        self.doc
            .revert_to(&self.history[self.head - 1])
            .expect("revert to a recorded checkpoint");
        self.head -= 1;
        true
    }

    /// Durably redo the most recently undone change.
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        self.doc
            .revert_to(&self.history[self.head + 1])
            .expect("revert to a recorded checkpoint");
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
            undo_history: self.history.iter().map(|f| b64().encode(f)).collect(),
            undo_head: self.head,
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
                let doc = CollabDoc::new(ActorId(on_disk.actor));
                let crdt = b64()
                    .decode(on_disk.crdt.as_bytes())
                    .map_err(|e| StoreError::Corrupt(format!("crdt base64: {e}")))?;
                doc.import(&crdt)
                    .map_err(|e| StoreError::Corrupt(format!("crdt import: {e}")))?;
                let history = on_disk
                    .undo_history
                    .iter()
                    .map(|s| b64().decode(s.as_bytes()))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StoreError::Corrupt(format!("history base64: {e}")))?;
                let head = on_disk.undo_head.min(history.len().saturating_sub(1));
                return Ok(Self {
                    name: on_disk.name,
                    created: on_disk.created,
                    doc,
                    history: if history.is_empty() {
                        vec![CollabDoc::new(actor).checkpoint()]
                    } else {
                        history
                    },
                    head,
                });
            }
        }
        // Legacy event-sourced edit document → migrate (REQ-088). Migration is
        // snapshot-identical; the migrated doc starts with no undo history (you
        // cannot undo past the import), which new edits then extend.
        let legacy: EditDocument = serde_json::from_slice(bytes)
            .map_err(|e| StoreError::Corrupt(format!("not a known edit format: {e}")))?;
        let doc = migrate::from_event_sourced(&legacy, actor);
        let baseline = doc.checkpoint();
        Ok(Self {
            name: legacy.name,
            created: legacy.created,
            doc,
            history: vec![baseline],
            head: 0,
        })
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

    #[test]
    fn durable_undo_redo_round_trips_through_persistence() {
        let mut e = PersistentEdit::create("rough-cut", ActorId(3));
        e.add_shot("src-001", words(0, 10)).unwrap();
        let b = e.add_shot("src-002", words(0, 10)).unwrap();
        e.add_shot("src-003", words(0, 10)).unwrap();
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
        e.add_shot("src-004", words(0, 10)).unwrap();
        assert!(!e.redo(), "a new edit after undo truncates redo");
    }

    #[test]
    fn rejects_invalid_range() {
        let mut e = PersistentEdit::create("rc", ActorId(1));
        assert!(matches!(
            e.add_shot("src-001", words(5, 5)),
            Err(StoreError::Edit(_))
        ));
        assert_eq!(ids(&e).len(), 0, "no shot added on invalid range");
        let id = e.add_shot("src-001", words(0, 10)).unwrap();
        assert!(matches!(e.trim_shot(&id, words(10, 3)), Err(StoreError::Edit(_))));
    }

    #[test]
    fn migrates_legacy_event_sourced_document() {
        // A legacy edit with three shots in its materialised snapshot.
        let mut legacy = EditDocument::create("legacy");
        legacy.add_shot("src-001", words(0, 10)).unwrap();
        legacy.add_shot("src-002", words(0, 10)).unwrap();
        let bytes = serde_json::to_vec(&legacy).unwrap();

        let e = PersistentEdit::from_bytes(&bytes, ActorId(9)).unwrap();
        assert_eq!(e.name(), "legacy");
        assert_eq!(e.snapshot().shots.len(), 2, "migration is snapshot-identical");
        assert!(!e.can_undo(), "migrated doc has no pre-import undo history");
    }
}
