//! TEST-019: Direct JSON authoring
//!
//! Verifies that an EditDocument can be constructed directly from hand-crafted
//! JSON (as a user or external tool would), deserialised, validated, and that
//! its snapshot matches the recomputed state from its ops.

use ar_edit_core::models::*;
use serde_json::json;

// -- Full document from JSON --------------------------------------------------

#[test]
fn full_edit_document_from_json() {
    let input = json!({
        "name": "rough-cut",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 4,
        "head": 2,
        "ops": [
            {
                "id": 0,
                "ts": "2026-02-19T13:00:01Z",
                "op": "add_shot",
                "shot": {
                    "id": "shot-001",
                    "source": "src-001",
                    "range": { "words": { "from": 0, "to": 52 } }
                }
            },
            {
                "id": 1,
                "ts": "2026-02-19T13:01:00Z",
                "op": "add_shot",
                "shot": {
                    "id": "shot-002",
                    "source": "src-002",
                    "range": { "scenes": { "from": 0, "to": 2 } }
                }
            },
            {
                "id": 2,
                "ts": "2026-02-19T13:02:00Z",
                "op": "add_shot",
                "shot": {
                    "id": "shot-003",
                    "source": "src-001",
                    "range": { "time": { "from_ms": 15000, "to_ms": 22000 } }
                }
            }
        ],
        "snapshot": {
            "shots": [
                { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } },
                { "id": "shot-003", "source": "src-001", "range": { "time": { "from_ms": 15000, "to_ms": 22000 } } }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    assert_eq!(doc.name, "rough-cut");
    assert_eq!(doc.head, 2);
    assert_eq!(doc.next_shot_id, 4);
    assert_eq!(doc.ops.len(), 3);
    assert_eq!(doc.snapshot.shots.len(), 3);
}

#[test]
fn json_snapshot_matches_recomputed() {
    let input = json!({
        "name": "test",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 4,
        "head": 2,
        "ops": [
            {
                "id": 0,
                "ts": "2026-02-19T13:00:01Z",
                "op": "add_shot",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            },
            {
                "id": 1,
                "ts": "2026-02-19T13:01:00Z",
                "op": "add_shot",
                "shot": { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } }
            },
            {
                "id": 2,
                "ts": "2026-02-19T13:02:00Z",
                "op": "add_shot",
                "shot": { "id": "shot-003", "source": "src-001", "range": { "time": { "from_ms": 15000, "to_ms": 22000 } } }
            }
        ],
        "snapshot": {
            "shots": [
                { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } },
                { "id": "shot-003", "source": "src-001", "range": { "time": { "from_ms": 15000, "to_ms": 22000 } } }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}

// -- Document with move and trim ops ------------------------------------------

#[test]
fn json_with_move_and_trim_ops() {
    let input = json!({
        "name": "complex-edit",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 4,
        "head": 4,
        "ops": [
            {
                "id": 0, "ts": "2026-02-19T13:00:01Z", "op": "add_shot",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            },
            {
                "id": 1, "ts": "2026-02-19T13:01:00Z", "op": "add_shot",
                "shot": { "id": "shot-002", "source": "src-002", "range": { "words": { "from": 100, "to": 200 } } }
            },
            {
                "id": 2, "ts": "2026-02-19T13:02:00Z", "op": "add_shot",
                "shot": { "id": "shot-003", "source": "src-001", "range": { "scenes": { "from": 0, "to": 2 } } }
            },
            {
                "id": 3, "ts": "2026-02-19T13:03:00Z", "op": "move_shot",
                "shot_id": "shot-003", "from_position": 2, "to_position": 0
            },
            {
                "id": 4, "ts": "2026-02-19T13:04:00Z", "op": "trim_shot",
                "shot_id": "shot-002",
                "old_range": { "words": { "from": 100, "to": 200 } },
                "new_range": { "words": { "from": 110, "to": 190 } }
            }
        ],
        "snapshot": {
            "shots": [
                { "id": "shot-003", "source": "src-001", "range": { "scenes": { "from": 0, "to": 2 } } },
                { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                { "id": "shot-002", "source": "src-002", "range": { "words": { "from": 110, "to": 190 } } }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    assert_eq!(doc.ops.len(), 5);
    assert_eq!(doc.snapshot.shots[0].id, "shot-003");
    assert_eq!(
        doc.snapshot.shots[2].range,
        ShotRange::Words { from: 110, to: 190 }
    );

    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}

// -- Document with notes ------------------------------------------------------

#[test]
fn json_with_notes() {
    let input = json!({
        "name": "noted-edit",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 2,
        "head": 1,
        "ops": [
            {
                "id": 0, "ts": "2026-02-19T13:00:01Z", "op": "add_shot",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            },
            {
                "id": 1, "ts": "2026-02-19T14:00:00Z", "op": "add_note",
                "shot_id": "shot-001",
                "note": { "text": "Great opening take", "created": "2026-02-19T14:00:00Z" }
            }
        ],
        "snapshot": {
            "shots": [
                {
                    "id": "shot-001",
                    "source": "src-001",
                    "range": { "words": { "from": 0, "to": 52 } },
                    "notes": [{ "text": "Great opening take", "created": "2026-02-19T14:00:00Z" }]
                }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Great opening take");

    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}

// -- Document with remove op --------------------------------------------------

#[test]
fn json_with_remove_op() {
    let input = json!({
        "name": "removed-edit",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 3,
        "head": 2,
        "ops": [
            {
                "id": 0, "ts": "2026-02-19T13:00:01Z", "op": "add_shot",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            },
            {
                "id": 1, "ts": "2026-02-19T13:01:00Z", "op": "add_shot",
                "shot": { "id": "shot-002", "source": "src-002", "range": { "time": { "from_ms": 5000, "to_ms": 10000 } } }
            },
            {
                "id": 2, "ts": "2026-02-19T13:02:00Z", "op": "remove_shot",
                "shot_id": "shot-001",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            }
        ],
        "snapshot": {
            "shots": [
                { "id": "shot-002", "source": "src-002", "range": { "time": { "from_ms": 5000, "to_ms": 10000 } } }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-002");

    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}

// -- JSON roundtrip -----------------------------------------------------------

#[test]
fn json_roundtrip_preserves_all_fields() {
    let input = json!({
        "name": "roundtrip",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 3,
        "head": 1,
        "ops": [
            {
                "id": 0, "ts": "2026-02-19T13:00:01Z", "op": "add_shot",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            },
            {
                "id": 1, "ts": "2026-02-19T13:01:00Z", "op": "add_shot",
                "shot": { "id": "shot-002", "source": "src-002", "range": { "time": { "from_ms": 5000, "to_ms": 10000 } } }
            }
        ],
        "snapshot": {
            "shots": [
                { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                { "id": "shot-002", "source": "src-002", "range": { "time": { "from_ms": 5000, "to_ms": 10000 } } }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    let serialized = serde_json::to_value(&doc).unwrap();
    let deserialized: EditDocument = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized, doc);
}

// -- Disk persistence ---------------------------------------------------------

#[test]
fn json_authored_doc_saves_and_loads() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("authored.edit.json");

    let input = json!({
        "name": "authored",
        "created": "2026-02-19T13:00:00Z",
        "next_shot_id": 3,
        "head": 1,
        "ops": [
            {
                "id": 0, "ts": "2026-02-19T13:00:01Z", "op": "add_shot",
                "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
            },
            {
                "id": 1, "ts": "2026-02-19T13:01:00Z", "op": "add_shot",
                "shot": { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } }
            }
        ],
        "snapshot": {
            "shots": [
                { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                { "id": "shot-002", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } }
            ]
        }
    });

    let doc: EditDocument = serde_json::from_value(input).unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded, doc);
}

// -- All range types in JSON --------------------------------------------------

#[test]
fn all_range_type_json_formats() {
    // Words
    let words: ShotRange = serde_json::from_value(json!({ "words": { "from": 0, "to": 52 } })).unwrap();
    assert_eq!(words, ShotRange::Words { from: 0, to: 52 });

    // Scenes
    let scenes: ShotRange = serde_json::from_value(json!({ "scenes": { "from": 0, "to": 2 } })).unwrap();
    assert_eq!(scenes, ShotRange::Scenes { from: 0, to: 2 });

    // Time
    let time: ShotRange = serde_json::from_value(json!({ "time": { "from_ms": 15000, "to_ms": 22000 } })).unwrap();
    assert_eq!(time, ShotRange::Time { from_ms: 15000, to_ms: 22000 });
}

// -- Op type JSON formats -----------------------------------------------------

#[test]
fn all_op_type_json_formats() {
    // add_shot
    let add: EditOp = serde_json::from_value(json!({
        "id": 0, "ts": "2026-02-19T13:00:00Z", "op": "add_shot",
        "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
    }))
    .unwrap();
    assert!(matches!(add.op, EditOpKind::AddShot { .. }));

    // remove_shot
    let remove: EditOp = serde_json::from_value(json!({
        "id": 1, "ts": "2026-02-19T13:01:00Z", "op": "remove_shot",
        "shot_id": "shot-001",
        "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
    }))
    .unwrap();
    assert!(matches!(remove.op, EditOpKind::RemoveShot { .. }));

    // move_shot
    let mv: EditOp = serde_json::from_value(json!({
        "id": 2, "ts": "2026-02-19T13:02:00Z", "op": "move_shot",
        "shot_id": "shot-001", "from_position": 2, "to_position": 0
    }))
    .unwrap();
    assert!(matches!(mv.op, EditOpKind::MoveShot { .. }));

    // trim_shot
    let trim: EditOp = serde_json::from_value(json!({
        "id": 3, "ts": "2026-02-19T13:03:00Z", "op": "trim_shot",
        "shot_id": "shot-001",
        "old_range": { "words": { "from": 0, "to": 100 } },
        "new_range": { "words": { "from": 10, "to": 90 } }
    }))
    .unwrap();
    assert!(matches!(trim.op, EditOpKind::TrimShot { .. }));

    // replace_range_type
    let replace: EditOp = serde_json::from_value(json!({
        "id": 4, "ts": "2026-02-19T13:04:00Z", "op": "replace_range_type",
        "shot_id": "shot-001",
        "old_range": { "words": { "from": 0, "to": 52 } },
        "new_range": { "time": { "from_ms": 0, "to_ms": 12400 } }
    }))
    .unwrap();
    assert!(matches!(replace.op, EditOpKind::ReplaceRangeType { .. }));

    // add_note
    let note: EditOp = serde_json::from_value(json!({
        "id": 5, "ts": "2026-02-19T13:05:00Z", "op": "add_note",
        "shot_id": "shot-001",
        "note": { "text": "Great take", "created": "2026-02-19T13:05:00Z" }
    }))
    .unwrap();
    assert!(matches!(note.op, EditOpKind::AddNote { .. }));
}
