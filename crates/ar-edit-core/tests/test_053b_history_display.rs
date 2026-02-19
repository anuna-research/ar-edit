//! TEST-053b: History display
//!
//! Verifies that the operation history (ops list + head position) is correctly
//! maintained, that the snapshot matches recomputed state at any head position,
//! and that persistence preserves the full history.

use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

fn three_shot_doc() -> EditDocument {
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });
    doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 });
    doc
}

// -- Full ops listing ---------------------------------------------------------

#[test]
fn history_lists_all_ops() {
    let doc = three_shot_doc();
    assert_eq!(doc.ops.len(), 3);
    assert!(matches!(&doc.ops[0].op, EditOpKind::AddShot { .. }));
    assert!(matches!(&doc.ops[1].op, EditOpKind::AddShot { .. }));
    assert!(matches!(&doc.ops[2].op, EditOpKind::AddShot { .. }));
}

#[test]
fn history_has_sequential_ids() {
    let doc = three_shot_doc();
    let ids: Vec<u32> = doc.ops.iter().map(|op| op.id).collect();
    assert_eq!(ids, vec![0, 1, 2]);
}

#[test]
fn history_has_timestamps() {
    let doc = three_shot_doc();
    for op in &doc.ops {
        // All timestamps should be valid and non-zero
        assert!(op.ts.timestamp() > 0);
    }
}

// -- Head position reflects undo state ----------------------------------------

#[test]
fn head_at_end_means_all_ops_applied() {
    let doc = three_shot_doc();
    assert_eq!(doc.head, 2);
    assert_eq!(doc.head as usize, doc.ops.len() - 1);
}

#[test]
fn head_after_undo_indicates_current_position() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();

    assert_eq!(doc.head, 1);
    // ops[0..=1] are "applied", ops[2] is "undone"
    assert_eq!(doc.ops.len(), 3);
}

#[test]
fn head_at_negative_one_means_empty() {
    let doc = EditDocument::create("empty");
    assert_eq!(doc.head, -1);
    assert!(doc.ops.is_empty());
}

// -- Mixed op types in history ------------------------------------------------

#[test]
fn history_shows_mixed_op_types() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
        .unwrap();
    doc.remove_shot("shot-002").unwrap();
    doc.add_note("shot-001", "Keep this").unwrap();

    assert_eq!(doc.ops.len(), 7);

    let op_types: Vec<&str> = doc
        .ops
        .iter()
        .map(|op| match &op.op {
            EditOpKind::AddShot { .. } => "add_shot",
            EditOpKind::RemoveShot { .. } => "remove_shot",
            EditOpKind::MoveShot { .. } => "move_shot",
            EditOpKind::TrimShot { .. } => "trim_shot",
            EditOpKind::ReplaceRangeType { .. } => "replace_range_type",
            EditOpKind::AddNote { .. } => "add_note",
        })
        .collect();

    assert_eq!(
        op_types,
        vec![
            "add_shot",
            "add_shot",
            "add_shot",
            "move_shot",
            "trim_shot",
            "remove_shot",
            "add_note",
        ]
    );
}

// -- Snapshot matches at various head positions --------------------------------

#[test]
fn snapshot_at_each_head_position() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
        .unwrap();

    // Verify snapshot at each head position
    for h in 0..=doc.head {
        let snapshot = EditDocument::recompute_snapshot(&doc.ops, h);
        // Just verify it doesn't panic and produces a reasonable result
        if h == 0 {
            assert_eq!(snapshot.shots.len(), 1);
        }
    }

    // Verify current snapshot matches
    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}

// -- Ops list with undone ops -------------------------------------------------

#[test]
fn undone_ops_remain_in_list() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();

    assert_eq!(doc.ops.len(), 3);
    assert_eq!(doc.head, 1);

    // Ops beyond head are "undone" but still present
    assert!(matches!(&doc.ops[2].op, EditOpKind::AddShot { shot } if shot.id == "shot-003"));
}

#[test]
fn undone_ops_distinguishable_by_head() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.undo().unwrap();
    // head=0

    let applied_ops = &doc.ops[..=(doc.head as usize)];
    let undone_ops = &doc.ops[((doc.head + 1) as usize)..];

    assert_eq!(applied_ops.len(), 1);
    assert_eq!(undone_ops.len(), 2);
}

// -- Persistence preserves full history ---------------------------------------

#[test]
fn save_load_preserves_all_ops_with_undo() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("history.edit.json");

    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();
    doc.undo().unwrap(); // head=2, ops has 4
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.head, 2);
    assert_eq!(loaded.ops.len(), 4);

    // Verify all op types present
    assert!(matches!(&loaded.ops[3].op, EditOpKind::MoveShot { .. }));

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}

#[test]
fn save_load_preserves_mixed_history() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("mixed-history.edit.json");

    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
        .unwrap();
    doc.remove_shot("shot-002").unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.ops.len(), 6);
    assert_eq!(loaded.head, 5);

    // Verify snapshot is consistent
    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}

#[test]
fn history_json_structure() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();

    let json = serde_json::to_value(&doc).unwrap();
    assert_eq!(json["head"], 1);

    let ops = json["ops"].as_array().unwrap();
    assert_eq!(ops.len(), 3);

    // Each op has id, ts, and op fields
    for op in ops {
        assert!(op.get("id").is_some());
        assert!(op.get("ts").is_some());
        assert!(op.get("op").is_some());
    }

    // Verify op tags
    assert_eq!(ops[0]["op"], "add_shot");
    assert_eq!(ops[1]["op"], "add_shot");
    assert_eq!(ops[2]["op"], "add_shot");
}
