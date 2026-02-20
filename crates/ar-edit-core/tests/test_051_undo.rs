//! TEST-051: Undo reverts last operation
//!
//! Verifies that `undo()` decrements head, recomputes snapshot, preserves
//! ops for redo, and persists correctly.

use ar_edit_core::edit::EditError;
use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

/// Helper: create a document with three shots.
fn three_shot_doc() -> EditDocument {
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 }).unwrap();
    doc
}

// -- Basic undo ---------------------------------------------------------------

#[test]
fn undo_decrements_head() {
    let mut doc = three_shot_doc();
    assert_eq!(doc.head, 2);

    doc.undo().unwrap();
    assert_eq!(doc.head, 1);
}

#[test]
fn undo_returns_undone_op() {
    let mut doc = three_shot_doc();
    let undone = doc.undo().unwrap();
    assert!(matches!(&undone.op, EditOpKind::AddShot { shot } if shot.id == "shot-003"));
}

#[test]
fn undo_removes_last_shot_from_snapshot() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();

    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
}

#[test]
fn undo_preserves_ops_for_redo() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();

    assert_eq!(doc.ops.len(), 3); // all ops preserved
}

// -- Multiple undos -----------------------------------------------------------

#[test]
fn undo_all_ops() {
    let mut doc = three_shot_doc();

    doc.undo().unwrap();
    assert_eq!(doc.head, 1);
    assert_eq!(doc.snapshot.shots.len(), 2);

    doc.undo().unwrap();
    assert_eq!(doc.head, 0);
    assert_eq!(doc.snapshot.shots.len(), 1);

    doc.undo().unwrap();
    assert_eq!(doc.head, -1);
    assert!(doc.snapshot.shots.is_empty());
}

#[test]
fn undo_past_beginning_errors() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();

    let err = doc.undo().unwrap_err();
    assert!(matches!(err, EditError::NothingToUndo));
}

#[test]
fn undo_empty_document_errors() {
    let mut doc = EditDocument::create("empty");
    let err = doc.undo().unwrap_err();
    assert!(matches!(err, EditError::NothingToUndo));
}

// -- Undo complex operations --------------------------------------------------

#[test]
fn undo_move_restores_original_order() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();

    // After move: [shot-003, shot-001, shot-002]
    assert_eq!(doc.snapshot.shots[0].id, "shot-003");

    // Undo move
    doc.undo().unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    assert_eq!(doc.snapshot.shots[2].id, "shot-003");
}

#[test]
fn undo_trim_restores_original_range() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();

    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 10, to: 90 }
    );

    doc.undo().unwrap();
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 0, to: 100 }
    );
}

#[test]
fn undo_remove_restores_shot() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.remove_shot("shot-001").unwrap();

    assert_eq!(doc.snapshot.shots.len(), 1);

    doc.undo().unwrap();
    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
}

#[test]
fn undo_note_removes_note() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_note("shot-001", "A note").unwrap();
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);

    doc.undo().unwrap();
    assert!(doc.snapshot.shots[0].notes.is_empty());
}

// -- Persistence roundtrip after undo -----------------------------------------

#[test]
fn save_load_after_undo() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("undone.edit.json");

    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.head, 1);
    assert_eq!(loaded.ops.len(), 3);
    assert_eq!(loaded.snapshot.shots.len(), 2);

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}

#[test]
fn save_load_after_full_undo() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("full-undo.edit.json");

    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.head, -1);
    assert_eq!(loaded.ops.len(), 3);
    assert!(loaded.snapshot.shots.is_empty());
}
