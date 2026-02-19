//! TEST-013: Reordering (move_shot)
//!
//! Verifies that `move_shot()` correctly reorders shots in the snapshot,
//! records the operation with from/to positions, and persists correctly.

use ar_edit_core::edit::EditError;
use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

/// Helper: create a document with three shots.
fn three_shot_doc() -> EditDocument {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });
    doc.add_shot("src-003", ShotRange::Time { from_ms: 5000, to_ms: 10000 });
    doc
}

// -- Basic reordering ---------------------------------------------------------

#[test]
fn move_last_to_front() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();

    assert_eq!(doc.snapshot.shots[0].id, "shot-003");
    assert_eq!(doc.snapshot.shots[1].id, "shot-001");
    assert_eq!(doc.snapshot.shots[2].id, "shot-002");
}

#[test]
fn move_first_to_end() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-001", 2).unwrap();

    assert_eq!(doc.snapshot.shots[0].id, "shot-002");
    assert_eq!(doc.snapshot.shots[1].id, "shot-003");
    assert_eq!(doc.snapshot.shots[2].id, "shot-001");
}

#[test]
fn move_middle_to_front() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-002", 0).unwrap();

    assert_eq!(doc.snapshot.shots[0].id, "shot-002");
    assert_eq!(doc.snapshot.shots[1].id, "shot-001");
    assert_eq!(doc.snapshot.shots[2].id, "shot-003");
}

#[test]
fn move_to_same_position_is_noop() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-002", 1).unwrap();

    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    assert_eq!(doc.snapshot.shots[2].id, "shot-003");
}

// -- Op record ----------------------------------------------------------------

#[test]
fn move_records_from_and_to_positions() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();

    let move_op = &doc.ops[3]; // ops 0,1,2 are adds, 3 is the move
    match &move_op.op {
        EditOpKind::MoveShot {
            shot_id,
            from_position,
            to_position,
        } => {
            assert_eq!(shot_id, "shot-003");
            assert_eq!(*from_position, 2);
            assert_eq!(*to_position, 0);
        }
        other => panic!("expected MoveShot, got {other:?}"),
    }
}

#[test]
fn head_advances_after_move() {
    let mut doc = three_shot_doc();
    assert_eq!(doc.head, 2);

    doc.move_shot("shot-003", 0).unwrap();
    assert_eq!(doc.head, 3);
    assert_eq!(doc.ops.len(), 4);
}

// -- Multiple moves -----------------------------------------------------------

#[test]
fn consecutive_moves() {
    let mut doc = three_shot_doc();

    // Move shot-003 to front
    doc.move_shot("shot-003", 0).unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-003");

    // Then move shot-001 to end
    doc.move_shot("shot-001", 2).unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-003");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    assert_eq!(doc.snapshot.shots[2].id, "shot-001");
}

// -- Error cases --------------------------------------------------------------

#[test]
fn move_nonexistent_shot_errors() {
    let mut doc = three_shot_doc();
    let err = doc.move_shot("shot-999", 0).unwrap_err();
    assert!(matches!(err, EditError::ShotNotFound(id) if id == "shot-999"));
}

#[test]
fn move_position_out_of_bounds_errors() {
    let mut doc = three_shot_doc();
    let err = doc.move_shot("shot-001", 10).unwrap_err();
    assert!(matches!(
        err,
        EditError::PositionOutOfBounds {
            position: 10,
            max: 2
        }
    ));
}

// -- Persistence roundtrip ----------------------------------------------------

#[test]
fn save_load_roundtrip_after_move() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("reordered.edit.json");

    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.snapshot.shots[0].id, "shot-003");
    assert_eq!(loaded.snapshot.shots[1].id, "shot-001");
    assert_eq!(loaded.snapshot.shots[2].id, "shot-002");
    assert_eq!(loaded.head, 3);
    assert_eq!(loaded.ops.len(), 4);

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}
