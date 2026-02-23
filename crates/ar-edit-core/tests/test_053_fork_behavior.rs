//! TEST-053: Fork behavior
//!
//! Verifies that performing a new edit after undo truncates the redo history
//! (forking the timeline), and that redo fails after a fork.

use ar_edit_core::edit::EditError;
use ar_edit_core::models::{EditDocument, ShotRange};
use tempfile::TempDir;

fn three_shot_doc() -> EditDocument {
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();
    doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
        .unwrap();
    doc
}

// -- Basic fork ---------------------------------------------------------------

#[test]
fn new_op_after_undo_truncates_redo_history() {
    let mut doc = three_shot_doc();

    // Undo two ops (head = 0)
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.head, 0);
    assert_eq!(doc.ops.len(), 3);

    // New edit forks: ops[1] and ops[2] are discarded
    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();

    assert_eq!(doc.ops.len(), 2); // op[0] kept + new op
    assert_eq!(doc.head, 1);
    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-004");
}

#[test]
fn redo_fails_after_fork() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();

    let err = doc.redo().unwrap_err();
    assert!(matches!(err, EditError::NothingToRedo));
}

// -- Fork with different op types ---------------------------------------------

#[test]
fn fork_with_move_op() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.undo().unwrap();
    // head=0, only shot-001 in snapshot

    // Cannot move with only 1 shot, so add another first
    // next_shot_id=4 persists, so new shot is shot-004
    doc.add_shot("src-005", ShotRange::Words { from: 50, to: 60 })
        .unwrap();
    // ops=[add shot-001, add shot-004], head=1

    doc.move_shot("shot-004", 0).unwrap();
    // ops=[add shot-001, add shot-004, move shot-004], head=2

    assert_eq!(doc.ops.len(), 3);
    assert_eq!(doc.snapshot.shots[0].id, "shot-004");
    assert_eq!(doc.snapshot.shots[1].id, "shot-001");
}

#[test]
fn fork_with_remove_op() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap(); // head=1, ops still has 3

    doc.remove_shot("shot-002").unwrap();
    // Fork: ops[2] (add shot-003) discarded, replaced by remove shot-002
    assert_eq!(doc.ops.len(), 3); // add, add, remove
    assert_eq!(doc.head, 2);
    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
}

#[test]
fn fork_with_trim_op() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap(); // head=1

    doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
        .unwrap();

    assert_eq!(doc.ops.len(), 3); // add, add, trim
    assert_eq!(doc.head, 2);
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 5, to: 45 }
    );
}

// -- Multiple forks -----------------------------------------------------------

#[test]
fn double_fork() {
    let mut doc = three_shot_doc();

    // First fork: undo 1 + add new
    doc.undo().unwrap();
    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();
    assert_eq!(doc.ops.len(), 3); // add, add, add(new)

    // Second fork: undo 2 + add new
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.add_shot("src-005", ShotRange::Scenes { from: 0, to: 1 })
        .unwrap();
    assert_eq!(doc.ops.len(), 2); // add(shot-001), add(shot-005)
    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-005");
}

// -- Op ID assignment after fork ----------------------------------------------

#[test]
fn op_ids_reset_correctly_after_fork() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.undo().unwrap();

    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();

    let ids: Vec<u32> = doc.ops.iter().map(|op| op.id).collect();
    assert_eq!(ids, vec![0, 1]); // sequential from 0
}

// -- Persistence roundtrip after fork -----------------------------------------

#[test]
fn save_load_after_fork() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("forked.edit.json");

    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.ops.len(), 2);
    assert_eq!(loaded.head, 1);
    assert_eq!(loaded.snapshot.shots.len(), 2);
    assert_eq!(loaded.snapshot.shots[0].id, "shot-001");
    assert_eq!(loaded.snapshot.shots[1].id, "shot-004");

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}

#[test]
fn redo_fails_after_load_of_forked_doc() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("forked2.edit.json");

    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();
    doc.save(&path).unwrap();

    let mut loaded = EditDocument::load(&path).unwrap();
    let err = loaded.redo().unwrap_err();
    assert!(matches!(err, EditError::NothingToRedo));
}
