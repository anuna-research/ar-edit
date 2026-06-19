//! The edit document as a Loro CRDT (SPEC-003 REQ-079, REQ-080, REQ-081,
//! REQ-082; ADR-007).
//!
//! ## Modelling decision (flat containers, ADR-007)
//!
//! Rather than nesting a container per shot, the document is a small set of
//! top-level Loro containers keyed by stable id. This satisfies every pure-core
//! requirement while avoiding nested-container API risk:
//!
//! | Container | Type | Holds | Requirement |
//! |-----------|------|-------|-------------|
//! | `order`        | `MovableList` | shot ids, in order | REQ-080 (identity-preserving move) |
//! | `shot_source`  | `Map`         | shot-id → source-id (LWW) | REQ-081 |
//! | `shot_range`   | `Map`         | shot-id → range JSON (LWW) | REQ-081 |
//! | `notes`        | `Map`         | note-id → note JSON | REQ-082 (grow-only) |
//! | `markers`      | `Map`         | marker-id → marker JSON | REQ-082 (OR-set) |
//! | `pois`         | `Map`         | poi-id → poi JSON | REQ-082 (OR-set) |
//!
//! A shot **move** is a `MovableList::mov` — the id stays put, so concurrent
//! field edits to that shot (kept in the keyed maps) are never lost (REQ-080 +
//! REQ-081). A shot's `range` is one whole value in an LWW register, so two
//! peers' concurrent trims converge to exactly one writer's intact `{from,to}`
//! — never an interleaved `from`/`to` (REQ-081, TEST-098). Notes are keyed by a
//! globally-unique `actor:counter` id, so concurrent appends all survive
//! (REQ-082, grow-only).

use crate::ids::ActorId;
use ar_edit_core::models::{Shot, ShotNote, ShotRange};
use loro::{ExportMode, LoroDoc, LoroValue, ValueOrContainer, VersionVector};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const ORDER: &str = "order";
pub(crate) const SHOT_SOURCE: &str = "shot_source";
pub(crate) const SHOT_RANGE: &str = "shot_range";
pub(crate) const NOTES: &str = "notes";
pub(crate) const MARKERS: &str = "markers";
pub(crate) const POIS: &str = "pois";

/// A note as stored in the `notes` map value (JSON).
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct NoteRec {
    pub shot_id: String,
    pub text: String,
    pub created: chrono::DateTime<chrono::Utc>,
}

/// The collaborative edit document. A thin, mutating facade over a [`LoroDoc`];
/// the materialised [`ar_edit_core::models::EditSnapshot`] is derived in
/// [`crate::materialise`].
pub struct CollabDoc {
    doc: LoroDoc,
    actor: ActorId,
    note_counter: AtomicU64,
}

pub(crate) fn as_string(v: Option<ValueOrContainer>) -> Option<String> {
    match v {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

impl CollabDoc {
    /// New empty document owned by `actor`. The actor id is the Loro peer id,
    /// so all changes this peer originates carry its site identifier (REQ-072).
    pub fn new(actor: ActorId) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(actor.0).expect("fresh doc accepts peer id");
        Self {
            doc,
            actor,
            note_counter: AtomicU64::new(0),
        }
    }

    pub fn actor(&self) -> ActorId {
        self.actor
    }

    pub(crate) fn doc(&self) -> &LoroDoc {
        &self.doc
    }

    fn order_index_of(&self, shot_id: &str) -> Option<usize> {
        let order = self.doc.get_movable_list(ORDER);
        (0..order.len()).find(|&i| as_string(order.get(i)).as_deref() == Some(shot_id))
    }

    /// Append a shot at the end of the order (mirrors `edit add-segment`).
    pub fn add_shot(&self, shot: &Shot) {
        let order = self.doc.get_movable_list(ORDER);
        let pos = order.len();
        self.add_shot_at(pos, shot);
    }

    /// Insert a shot at `pos` in the order.
    pub fn add_shot_at(&self, pos: usize, shot: &Shot) {
        let order = self.doc.get_movable_list(ORDER);
        order
            .insert(pos.min(order.len()), shot.id.as_str())
            .expect("insert id");
        self.doc
            .get_map(SHOT_SOURCE)
            .insert(shot.id.as_str(), shot.source.as_str())
            .expect("set source");
        self.doc
            .get_map(SHOT_RANGE)
            .insert(shot.id.as_str(), range_to_json(&shot.range).as_str())
            .expect("set range");
        for note in &shot.notes {
            self.add_note(&shot.id, note);
        }
        self.doc.commit();
    }

    /// Remove a shot, retiring its id (REQ — id never reused by the caller).
    pub fn remove_shot(&self, shot_id: &str) {
        if let Some(idx) = self.order_index_of(shot_id) {
            let order = self.doc.get_movable_list(ORDER);
            order.delete(idx, 1).expect("delete from order");
        }
        let _ = self.doc.get_map(SHOT_SOURCE).delete(shot_id);
        let _ = self.doc.get_map(SHOT_RANGE).delete(shot_id);
        // Drop this shot's notes (orphans would be ignored by materialise, but
        // keep the doc tidy).
        for note_id in self.note_ids_for(shot_id) {
            let _ = self.doc.get_map(NOTES).delete(&note_id);
        }
        self.doc.commit();
    }

    /// Relocate a shot to `to_pos` (REQ-080: move, not delete+insert).
    pub fn move_shot(&self, shot_id: &str, to_pos: usize) {
        if let Some(idx) = self.order_index_of(shot_id) {
            let order = self.doc.get_movable_list(ORDER);
            let to = to_pos.min(order.len().saturating_sub(1));
            order.mov(idx, to).expect("move");
            self.doc.commit();
        }
    }

    /// Replace a shot's range (REQ-081: whole-value LWW register).
    pub fn trim_shot(&self, shot_id: &str, new_range: &ShotRange) {
        self.doc
            .get_map(SHOT_RANGE)
            .insert(shot_id, range_to_json(new_range).as_str())
            .expect("set range");
        self.doc.commit();
    }

    /// Change a shot's source (REQ-081: LWW register).
    pub fn set_source(&self, shot_id: &str, source: &str) {
        self.doc
            .get_map(SHOT_SOURCE)
            .insert(shot_id, source)
            .expect("set source");
        self.doc.commit();
    }

    /// Append a note to a shot (REQ-082: grow-only; never lost on merge).
    pub fn add_note(&self, shot_id: &str, note: &ShotNote) {
        let counter = self.note_counter.fetch_add(1, Ordering::Relaxed);
        let note_id = format!("{:016x}:{:08x}", self.actor.0, counter);
        let rec = NoteRec {
            shot_id: shot_id.to_string(),
            text: note.text.clone(),
            created: note.created,
        };
        let json = serde_json::to_string(&rec).expect("note json");
        self.doc
            .get_map(NOTES)
            .insert(note_id.as_str(), json.as_str())
            .expect("add note");
        self.doc.commit();
    }

    fn note_ids_for(&self, shot_id: &str) -> Vec<String> {
        let notes = self.doc.get_map(NOTES);
        let mut ids = Vec::new();
        let value = notes.get_value();
        if let LoroValue::Map(m) = value {
            for (k, v) in m.iter() {
                if let LoroValue::String(s) = v {
                    if let Ok(rec) = serde_json::from_str::<NoteRec>(s) {
                        if rec.shot_id == shot_id {
                            ids.push(k.to_string());
                        }
                    }
                }
            }
        }
        ids
    }

    // ---- markers / POIs (observed-remove sets, REQ-082) ----

    /// Add or replace a marker by id; `json` is the serialised marker.
    pub fn put_marker(&self, marker_id: &str, json: &str) {
        self.doc
            .get_map(MARKERS)
            .insert(marker_id, json)
            .expect("put marker");
        self.doc.commit();
    }

    pub fn remove_marker(&self, marker_id: &str) {
        let _ = self.doc.get_map(MARKERS).delete(marker_id);
        self.doc.commit();
    }

    pub fn put_poi(&self, poi_id: &str, json: &str) {
        self.doc.get_map(POIS).insert(poi_id, json).expect("put poi");
        self.doc.commit();
    }

    pub fn remove_poi(&self, poi_id: &str) {
        let _ = self.doc.get_map(POIS).delete(poi_id);
        self.doc.commit();
    }

    fn map_keys(&self, name: &str) -> Vec<String> {
        let mut keys = Vec::new();
        if let LoroValue::Map(m) = self.doc.get_map(name).get_value() {
            for (k, _) in m.iter() {
                keys.push(k.to_string());
            }
        }
        keys.sort();
        keys
    }

    /// Live marker ids (observed-remove set, REQ-082).
    pub fn marker_ids(&self) -> Vec<String> {
        self.map_keys(MARKERS)
    }

    /// Live POI ids (observed-remove set, REQ-082).
    pub fn poi_ids(&self) -> Vec<String> {
        self.map_keys(POIS)
    }

    // ---- sync ----

    /// Full snapshot for first-contact sync or persistence.
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.doc
            .export(ExportMode::Snapshot)
            .expect("export snapshot")
    }

    /// Merge another peer's snapshot or delta into this document. After import,
    /// the local note counter is advanced past any of this actor's note ids
    /// already present, so a reloaded doc (same ActorId) never reuses an id and
    /// overwrites an existing note (grow-only guarantee across restarts).
    pub fn import(&self, bytes: &[u8]) -> Result<(), loro::LoroError> {
        self.doc.import(bytes)?;
        self.sync_note_counter();
        Ok(())
    }

    /// Advance `note_counter` past the highest `actor:counter` note id already
    /// in the document for this actor.
    fn sync_note_counter(&self) {
        let prefix = format!("{:016x}:", self.actor.0);
        let mut highest: Option<u64> = None;
        if let LoroValue::Map(m) = self.doc.get_map(NOTES).get_value() {
            for (k, _) in m.iter() {
                if let Some(rest) = k.strip_prefix(prefix.as_str()) {
                    if let Ok(n) = u64::from_str_radix(rest, 16) {
                        highest = Some(highest.map_or(n, |h| h.max(n)));
                    }
                }
            }
        }
        if let Some(h) = highest {
            let next = h + 1;
            // Only ever move the counter forward.
            let _ = self
                .note_counter
                .fetch_max(next, Ordering::Relaxed);
        }
    }

    /// This document's current version vector (for delta sync, REQ-087).
    pub fn version(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Encode only the changes this document has that `remote` is missing.
    pub fn export_from(&self, remote: &VersionVector) -> Vec<u8> {
        self.doc
            .export(ExportMode::updates(remote))
            .expect("export updates")
    }
}

/// Serialise a [`ShotRange`] to the externally-tagged JSON used by
/// `ar-edit-core` (e.g. `{"words":{"from":0,"to":52}}`).
pub(crate) fn range_to_json(range: &ShotRange) -> String {
    serde_json::to_string(range).expect("range json")
}

pub(crate) fn range_from_json(s: &str) -> Option<ShotRange> {
    serde_json::from_str(s).ok()
}
