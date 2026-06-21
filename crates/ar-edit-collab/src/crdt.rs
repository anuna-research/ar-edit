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
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const ORDER: &str = "order";
pub(crate) const SHOT_SOURCE: &str = "shot_source";
pub(crate) const SHOT_RANGE: &str = "shot_range";
pub(crate) const NOTES: &str = "notes";
pub(crate) const MARKERS: &str = "markers";
pub(crate) const POIS: &str = "pois";
/// Per-actor high-water marks for the minted-id counters (shot / note / marker
/// tag). Persisted in the document so a reload after a deletion never re-mints a
/// tombstoned id (the live containers alone would reset the counter).
pub(crate) const COUNTERS: &str = "id_counters";

/// Commit origin for id high-water-mark writes. [`crate::undo::LocalUndo`]
/// excludes this prefix from undo history, so undoing an insert can never roll
/// the counter back (which would let the next insert reuse a tombstoned id and
/// let a delayed peer op target the wrong shot — REQ-080/REQ-086).
pub(crate) const COUNTER_ORIGIN: &str = "ar-edit:hwm";

/// Separates an observed-remove-set logical id from its unique instance tag in
/// the markers/POIs map keys. A unit separator never appears in an id.
const TAG_SEP: char = '\u{1f}';

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
    /// The local actor / Loro peer id. Held in an atomic so a migrated document
    /// can be re-keyed to the node-derived actor after deterministic migration.
    actor: AtomicU64,
    note_counter: AtomicU64,
    shot_counter: AtomicU64,
    /// Counter for observed-remove-set instance tags (markers / POIs).
    orset_counter: AtomicU64,
}

pub(crate) fn as_string(v: Option<ValueOrContainer>) -> Option<String> {
    match v {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

fn as_i64(v: Option<ValueOrContainer>) -> Option<i64> {
    match v {
        Some(ValueOrContainer::Value(LoroValue::I64(n))) => Some(n),
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
            actor: AtomicU64::new(actor.0),
            note_counter: AtomicU64::new(0),
            shot_counter: AtomicU64::new(0),
            orset_counter: AtomicU64::new(0),
        }
    }

    pub fn actor(&self) -> ActorId {
        ActorId(self.actor.load(Ordering::Relaxed))
    }

    /// Current minted-id counter high-water marks (shot, note, orset) — the
    /// *next* value each will mint. These must be persisted alongside the
    /// snapshot: a `revert_to` (cursor undo) rolls back the in-document
    /// [`COUNTERS`] map, so the reverted snapshot alone would let a reload
    /// re-mint a tombstoned id (REQ-080). The atomics are never reverted, so
    /// they hold the true marks.
    pub fn counter_hwm(&self) -> (u64, u64, u64) {
        (
            self.shot_counter.load(Ordering::Relaxed),
            self.note_counter.load(Ordering::Relaxed),
            self.orset_counter.load(Ordering::Relaxed),
        )
    }

    /// Raise the minted-id counter marks (monotonic), so a reload never reuses an
    /// id even if the persisted snapshot's [`COUNTERS`] map was reverted by undo.
    pub fn restore_counter_hwm(&self, (shot, note, orset): (u64, u64, u64)) {
        let _ = self.shot_counter.fetch_max(shot, Ordering::Relaxed);
        let _ = self.note_counter.fetch_max(note, Ordering::Relaxed);
        let _ = self.orset_counter.fetch_max(orset, Ordering::Relaxed);
    }

    /// Re-key this document's actor (Loro peer id) for *subsequent* operations.
    /// Used after a deterministic migration so the migration ops carry a fixed
    /// peer id (idempotent across independent migrations) while later edits
    /// carry the caller's node-derived actor (REQ-072, REQ-088).
    pub fn rekey_actor(&self, actor: ActorId) -> Result<(), loro::LoroError> {
        self.doc.commit();
        self.doc.set_peer_id(actor.0)?;
        self.actor.store(actor.0, Ordering::Relaxed);
        Ok(())
    }

    /// Mint a globally-unique shot id (actor-scoped) for collaborative inserts,
    /// so two replicas never produce a colliding id without coordination
    /// (REQ-080).
    fn mint_shot_id(&self) -> String {
        let c = self.shot_counter.fetch_add(1, Ordering::Relaxed);
        self.record_counter("shot", c);
        format!("shot-{:016x}-{:x}", self.actor.load(Ordering::Relaxed), c)
    }

    /// Persist the *next* value of a minted-id counter under an actor-scoped key
    /// in [`COUNTERS`], so the high-water mark survives deletion of every entry
    /// it was derived from (a reload then never reuses a tombstoned id). Only
    /// this actor writes its own key, so the register never conflicts on merge.
    fn record_counter(&self, kind: &str, used: u64) {
        let key = format!("{kind}:{:016x}", self.actor.load(Ordering::Relaxed));
        let map = self.doc.get_map(COUNTERS);
        let next = used.saturating_add(1) as i64;
        let cur = as_i64(map.get(key.as_str())).unwrap_or(0);
        if next <= cur {
            return;
        }
        // Flush any in-flight insert ops first so they keep the default
        // (undoable) origin, then write the high-water mark in its OWN commit
        // tagged with COUNTER_ORIGIN — which LocalUndo excludes from undo. This
        // keeps an undo of the insert from also reverting the counter bump (which
        // would resurrect the tombstoned id for reuse).
        self.doc.commit();
        map.insert(key.as_str(), next).expect("counter");
        self.doc.set_next_commit_origin(COUNTER_ORIGIN);
        self.doc.commit();
    }

    /// The persisted next-counter floor for `kind` (this actor), if any.
    fn counter_floor(&self, kind: &str) -> Option<u64> {
        let key = format!("{kind}:{:016x}", self.actor.load(Ordering::Relaxed));
        as_i64(self.doc.get_map(COUNTERS).get(key.as_str())).map(|n| n.max(0) as u64)
    }

    /// Use the requested id if it is free in this document, otherwise mint a
    /// unique one (guards against local duplicate ids overwriting fields).
    fn unique_id_for(&self, requested: &str) -> String {
        if self.order_index_of(requested).is_some() {
            self.mint_shot_id()
        } else {
            requested.to_string()
        }
    }

    fn insert_shot(&self, pos: usize, id: &str, source: &str, range: &ShotRange, notes: &[ShotNote]) {
        let order = self.doc.get_movable_list(ORDER);
        order.insert(pos.min(order.len()), id).expect("insert id");
        self.doc
            .get_map(SHOT_SOURCE)
            .insert(id, source)
            .expect("set source");
        self.doc
            .get_map(SHOT_RANGE)
            .insert(id, range_to_json(range).as_str())
            .expect("set range");
        for note in notes {
            self.add_note(id, note);
        }
        self.doc.commit();
    }

    pub(crate) fn doc(&self) -> &LoroDoc {
        &self.doc
    }

    fn order_index_of(&self, shot_id: &str) -> Option<usize> {
        let order = self.doc.get_movable_list(ORDER);
        (0..order.len()).find(|&i| as_string(order.get(i)).as_deref() == Some(shot_id))
    }

    /// Append a shot, preserving its id when free (migration/import) or minting
    /// a unique one on a local collision. For NEW collaborative inserts prefer
    /// [`Self::add_new_shot`], which always mints an actor-scoped id so
    /// concurrent inserts on different replicas never collide (REQ-080).
    pub fn add_shot(&self, shot: &Shot) {
        let pos = self.doc.get_movable_list(ORDER).len();
        self.add_shot_at(pos, shot);
    }

    /// Insert a shot at `pos`, with the same id-uniqueness handling as
    /// [`Self::add_shot`].
    pub fn add_shot_at(&self, pos: usize, shot: &Shot) {
        let id = self.unique_id_for(&shot.id);
        self.insert_shot(pos, &id, &shot.source, &shot.range, &shot.notes);
    }

    /// Append a brand-new shot with a freshly-minted, globally-unique id and
    /// return that id. This is the correct entry point for live collaborative
    /// inserts (CLI/agent/daemon): the id is actor-scoped, so two replicas
    /// inserting concurrently never produce a colliding id (REQ-080).
    pub fn add_new_shot(&self, source: &str, range: &ShotRange) -> String {
        let id = self.mint_shot_id();
        let pos = self.doc.get_movable_list(ORDER).len();
        self.insert_shot(pos, &id, source, range, &[]);
        id
    }

    /// Remove a shot, retiring its id (REQ — id never reused by the caller). A
    /// shot that is not present is a true no-op (no commit, no op) — otherwise
    /// the deletes/commit would advance the version and record a phantom step.
    pub fn remove_shot(&self, shot_id: &str) {
        let Some(idx) = self.order_index_of(shot_id) else {
            return;
        };
        self.doc
            .get_movable_list(ORDER)
            .delete(idx, 1)
            .expect("delete from order");
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

    /// Replace a shot's range (REQ-081: whole-value LWW register). A shot that is
    /// not present is a no-op — otherwise the write would orphan a range key (not
    /// in `order`, so invisible to materialise) and record a phantom undo step.
    pub fn trim_shot(&self, shot_id: &str, new_range: &ShotRange) {
        if self.order_index_of(shot_id).is_none() {
            return;
        }
        self.doc
            .get_map(SHOT_RANGE)
            .insert(shot_id, range_to_json(new_range).as_str())
            .expect("set range");
        self.doc.commit();
    }

    /// Append a note to a shot (REQ-082: grow-only; never lost on merge).
    pub fn add_note(&self, shot_id: &str, note: &ShotNote) {
        let counter = self.note_counter.fetch_add(1, Ordering::Relaxed);
        self.record_counter("note", counter);
        let note_id = format!(
            "{:016x}:{:08x}",
            self.actor.load(Ordering::Relaxed),
            counter
        );
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
    //
    // A Loro map is LWW, so keying a marker directly by its id would let a
    // concurrent remove win over an add it never observed, dropping the marker.
    // Instead each `put` writes a uniquely-*tagged* instance key
    // (`<id>\u{1f}<actor>:<counter>`); a `remove` deletes only the instances it
    // currently observes. A concurrent re-add carries a fresh tag the remove did
    // not observe, so it survives the merge (true OR-set semantics, REQ-082).

    /// Mint a globally-unique, never-reused instance tag for an OR-set add. The
    /// counter is persisted (see [`Self::record_counter`]) so a reload never
    /// reuses a tag a tombstone already covers.
    fn mint_orset_tag(&self) -> String {
        let c = self.orset_counter.fetch_add(1, Ordering::Relaxed);
        self.record_counter("orset", c);
        format!("{:016x}:{:08x}", self.actor.load(Ordering::Relaxed), c)
    }

    /// The map keys that are live instances of `logical_id` (including any legacy
    /// untagged key equal to the id, for documents written before the OR-set).
    fn orset_instance_keys(&self, name: &str, logical_id: &str) -> Vec<String> {
        let prefix = format!("{logical_id}{TAG_SEP}");
        let mut keys = Vec::new();
        if let LoroValue::Map(m) = self.doc.get_map(name).get_value() {
            for (k, _) in m.iter() {
                let key = k.to_string();
                if key == logical_id || key.starts_with(prefix.as_str()) {
                    keys.push(key);
                }
            }
        }
        keys
    }

    /// Live logical ids in an OR-set container (sorted, deduplicated).
    fn orset_live_ids(&self, name: &str) -> Vec<String> {
        let mut ids: BTreeSet<String> = BTreeSet::new();
        if let LoroValue::Map(m) = self.doc.get_map(name).get_value() {
            for (k, _) in m.iter() {
                let key = k.to_string();
                let logical = key.split(TAG_SEP).next().unwrap_or(key.as_str());
                ids.insert(logical.to_string());
            }
        }
        ids.into_iter().collect()
    }

    /// Add (or update) an OR-set member. An update is modelled as an
    /// observed-replace: the instances this replica currently sees are tombstoned
    /// and a fresh tagged instance is written, so concurrent adds elsewhere are
    /// never clobbered.
    fn orset_put(&self, name: &str, logical_id: &str, json: &str) {
        // Mint the instance tag FIRST. `mint_orset_tag` persists the counter
        // high-water mark in its own COUNTER_ORIGIN commit (which flushes any
        // in-flight ops); doing it before we touch the OR-set keeps that commit
        // from landing BETWEEN the tombstones and the insert below. The
        // observed-replace — delete the observed instances, insert the fresh one
        // — therefore commits as a SINGLE undo unit, so one `LocalUndo::undo()`
        // of an update restores the previous instance instead of deleting the
        // marker outright (REQ-082/REQ-086).
        let key = format!("{logical_id}{TAG_SEP}{}", self.mint_orset_tag());
        let map = self.doc.get_map(name);
        for k in self.orset_instance_keys(name, logical_id) {
            let _ = map.delete(&k);
        }
        map.insert(key.as_str(), json).expect("put orset member");
        self.doc.commit();
    }

    /// Remove an OR-set member: tombstone every instance this replica observes.
    fn orset_remove(&self, name: &str, logical_id: &str) {
        let map = self.doc.get_map(name);
        for k in self.orset_instance_keys(name, logical_id) {
            let _ = map.delete(&k);
        }
        self.doc.commit();
    }

    /// Add or replace a marker by id; `json` is the serialised marker.
    pub fn put_marker(&self, marker_id: &str, json: &str) {
        self.orset_put(MARKERS, marker_id, json);
    }

    pub fn remove_marker(&self, marker_id: &str) {
        self.orset_remove(MARKERS, marker_id);
    }

    pub fn put_poi(&self, poi_id: &str, json: &str) {
        self.orset_put(POIS, poi_id, json);
    }

    pub fn remove_poi(&self, poi_id: &str) {
        self.orset_remove(POIS, poi_id);
    }

    /// Live marker ids (observed-remove set, REQ-082).
    pub fn marker_ids(&self) -> Vec<String> {
        self.orset_live_ids(MARKERS)
    }

    /// Live POI ids (observed-remove set, REQ-082).
    pub fn poi_ids(&self) -> Vec<String> {
        self.orset_live_ids(POIS)
    }

    // ---- durable head-over-oplog undo (SPIKE: SPEC-003 OQ-8 / ADR-011) ----
    //
    // Durable single-writer undo without a daemon: model `head` as a cursor over
    // checkpoints (Loro frontiers) and revert between them with `revert_to`,
    // which generates *local* ops to move the state — so the doc stays attached
    // and editable (unlike `checkout`, which detaches), the change is durable in
    // the oplog/snapshot, and it propagates to peers as an ordinary CRDT update
    // (REQ-086). This is the offline analog of the per-actor `UndoManager`.

    /// An encoded checkpoint of the current visible state, to record after each
    /// user edit. Bytes are what a unified on-disk format would persist as the
    /// undo-cursor history (no Loro types leak to callers).
    pub fn checkpoint(&self) -> Vec<u8> {
        self.doc.state_frontiers().encode()
    }

    /// Durably move the visible state to the encoded `checkpoint` by applying
    /// revert ops. Stays attached; the move is itself recorded in the oplog (so
    /// a later revert can undo it — that is "redo").
    pub fn revert_to(&self, checkpoint: &[u8]) -> Result<(), loro::LoroError> {
        let target = loro::Frontiers::decode(checkpoint)?;
        self.doc.revert_to(&target)?;
        // A revert may resurrect or retire minted ids; keep the counters ahead.
        self.sync_shot_counter();
        self.sync_note_counter();
        Ok(())
    }

    /// Whether the document is attached to its latest oplog version (editable).
    /// The `revert_to`-based undo above keeps this true; `checkout` would not.
    pub fn is_attached(&self) -> bool {
        !self.doc.is_detached()
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
        self.sync_shot_counter();
        // OR-set instance tags must also never be reused after a reload.
        if let Some(floor) = self.counter_floor("orset") {
            let _ = self.orset_counter.fetch_max(floor, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Advance the shot counter past any minted shot ids already present for
    /// this actor, so a reloaded doc never re-mints a colliding id. Combines the
    /// persisted high-water mark (which survives deletions, [`COUNTERS`]) with a
    /// scan of live ids (the fallback for documents written before COUNTERS).
    fn sync_shot_counter(&self) {
        let prefix = format!("shot-{:016x}-", self.actor.load(Ordering::Relaxed));
        let mut highest: Option<u64> = None;
        let order = self.doc.get_movable_list(ORDER);
        for i in 0..order.len() {
            if let Some(s) = as_string(order.get(i)) {
                if let Some(rest) = s.strip_prefix(prefix.as_str()) {
                    if let Ok(n) = u64::from_str_radix(rest, 16) {
                        highest = Some(highest.map_or(n, |h| h.max(n)));
                    }
                }
            }
        }
        if let Some(h) = highest {
            let _ = self.shot_counter.fetch_max(h + 1, Ordering::Relaxed);
        }
        if let Some(floor) = self.counter_floor("shot") {
            let _ = self.shot_counter.fetch_max(floor, Ordering::Relaxed);
        }
    }

    /// Advance `note_counter` past the highest `actor:counter` note id already
    /// in the document for this actor.
    fn sync_note_counter(&self) {
        let prefix = format!("{:016x}:", self.actor.load(Ordering::Relaxed));
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
        if let Some(floor) = self.counter_floor("note") {
            let _ = self.note_counter.fetch_max(floor, Ordering::Relaxed);
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
