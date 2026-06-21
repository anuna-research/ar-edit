//! Materialise the CRDT into the on-disk views consumed by the existing
//! read-side commands (SPEC-003 REQ-079). Pure and deterministic: the same
//! CRDT state always yields the same [`EditSnapshot`] (REQ-083).

use crate::crdt::{
    as_string, range_from_json, CollabDoc, NoteRec, NOTES, ORDER, SHOT_AUTHOR, SHOT_RANGE,
    SHOT_SOURCE,
};
use ar_edit_core::models::{EditDocument, EditSnapshot, Shot, ShotNote, ShotRange};
use chrono::{TimeZone, Utc};
use loro::LoroValue;
use std::collections::HashMap;

/// Reconstruct the ordered shot list from CRDT state.
pub fn materialise(collab: &CollabDoc) -> EditSnapshot {
    let doc = collab.doc();
    let order = doc.get_movable_list(ORDER);
    let source_map = doc.get_map(SHOT_SOURCE);
    let range_map = doc.get_map(SHOT_RANGE);
    let author_map = doc.get_map(SHOT_AUTHOR);

    // Group notes by shot id (note-id kept for deterministic tie-break).
    let mut notes_by_shot: HashMap<String, Vec<(String, NoteRec)>> = HashMap::new();
    if let LoroValue::Map(m) = doc.get_map(NOTES).get_value() {
        for (k, v) in m.iter() {
            if let LoroValue::String(s) = v {
                if let Ok(rec) = serde_json::from_str::<NoteRec>(&s.to_string()) {
                    notes_by_shot
                        .entry(rec.shot_id.clone())
                        .or_default()
                        .push((k.to_string(), rec));
                }
            }
        }
    }

    let mut shots = Vec::with_capacity(order.len());
    for i in 0..order.len() {
        let id = match as_string(order.get(i)) {
            Some(s) => s,
            None => continue,
        };
        let source = as_string(source_map.get(id.as_str())).unwrap_or_default();
        let range = as_string(range_map.get(id.as_str()))
            .and_then(|s| range_from_json(&s))
            .unwrap_or(ShotRange::Time {
                from_ms: 0,
                to_ms: 0,
            });
        let notes = notes_by_shot
            .remove(&id)
            .map(|mut v| {
                // Order by note id only. A note id is `<actor>:<counter>`, so
                // within a single (e.g. migrated) actor this is exactly the
                // append order — preserving the migration identity guarantee
                // (REQ-083) even when creation timestamps are non-monotonic
                // (clock rollback, hand-edited legacy snapshots). Across actors
                // it stays deterministic (actor-major), so merges still converge.
                v.sort_by(|a, b| a.0.cmp(&b.0));
                v.into_iter()
                    .map(|(_, rec)| ShotNote {
                        text: rec.text,
                        author: rec.author,
                        created: rec.created,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let author = as_string(author_map.get(id.as_str())).unwrap_or_default();
        shots.push(Shot {
            id,
            source,
            range,
            notes,
            author,
        });
    }
    EditSnapshot { shots }
}

/// Build a full on-disk [`EditDocument`] view: a snapshot-only document
/// (`ops: []`, `head: -1`) per [`DATA-MODEL`] conventions, suitable for
/// `edit show` / `validate` / `render` to consume unchanged (REQ-079, TEST-093).
pub fn to_edit_document(collab: &CollabDoc, name: impl Into<String>) -> EditDocument {
    let snapshot = materialise(collab);
    let next_shot_id = snapshot
        .shots
        .iter()
        .filter_map(|s| s.id.strip_prefix("shot-"))
        .filter_map(|n| n.parse::<u32>().ok())
        .max()
        .map(|m| m + 1)
        .unwrap_or(1);
    EditDocument {
        name: name.into(),
        // Epoch as a stable placeholder; persistence overwrites with real ts.
        created: Utc.timestamp_opt(0, 0).single().unwrap_or_else(Utc::now),
        next_shot_id,
        head: -1,
        ops: Vec::new(),
        snapshot,
    }
}
