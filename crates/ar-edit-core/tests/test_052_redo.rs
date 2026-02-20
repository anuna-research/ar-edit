//! TEST-052: Redo re-applies after undo
//!
//! Verifies that `redo()` increments head, recomputes snapshot to include
//! the re-applied op, and that full undo→redo roundtrips restore state.

use ar_edit_core::edit::EditError;
use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

fn three_shot_doc() -> EditDocument {
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 }).unwrap();
    doc
}

// -- Basic redo ---------------------------------------------------------------

#[test]
fn redo_increments_head() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    assert_eq!(doc.head, 1);

    doc.redo().unwrap();
    assert_eq!(doc.head, 2);
}

#[test]
fn redo_returns_redone_op() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    let redone = doc.redo().unwrap();
    assert!(matches!(&redone.op, EditOpKind::AddShot { shot } if shot.id == "shot-003"));
}

#[test]
fn redo_restores_snapshot() {
    let mut doc = three_shot_doc();
    let original = doc.snapshot.clone();
    doc.undo().unwrap();
    doc.redo().unwrap();

    assert_eq!(doc.snapshot, original);
}

// -- Redo without undo --------------------------------------------------------

#[test]
fn redo_without_undo_errors() {
    let mut doc = three_shot_doc();
    let err = doc.redo().unwrap_err();
    assert!(matches!(err, EditError::NothingToRedo));
}

#[test]
fn redo_empty_document_errors() {
    let mut doc = EditDocument::create("empty");
    let err = doc.redo().unwrap_err();
    assert!(matches!(err, EditError::NothingToRedo));
}

// -- Full undo→redo roundtrip -------------------------------------------------

#[test]
fn undo_all_then_redo_all() {
    let mut doc = three_shot_doc();
    let original = doc.snapshot.clone();

    // Undo all
    for _ in 0..3 {
        doc.undo().unwrap();
    }
    assert_eq!(doc.head, -1);
    assert!(doc.snapshot.shots.is_empty());

    // Redo all
    for _ in 0..3 {
        doc.redo().unwrap();
    }
    assert_eq!(doc.head, 2);
    assert_eq!(doc.snapshot, original);
}

#[test]
fn redo_all_then_further_redo_errors() {
    let mut doc = three_shot_doc();
    doc.undo().unwrap();
    doc.redo().unwrap();

    let err = doc.redo().unwrap_err();
    assert!(matches!(err, EditError::NothingToRedo));
}

// -- Partial undo→redo --------------------------------------------------------

#[test]
fn partial_undo_redo() {
    let mut doc = three_shot_doc();

    // Undo twice
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.head, 0);
    assert_eq!(doc.snapshot.shots.len(), 1);

    // Redo once
    doc.redo().unwrap();
    assert_eq!(doc.head, 1);
    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
}

// -- Redo complex operations --------------------------------------------------

#[test]
fn redo_move_op() {
    let mut doc = three_shot_doc();
    doc.move_shot("shot-003", 0).unwrap();

    doc.undo().unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");

    doc.redo().unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-003");
}

#[test]
fn redo_trim_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();

    doc.undo().unwrap();
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 0, to: 100 }
    );

    doc.redo().unwrap();
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 10, to: 90 }
    );
}

#[test]
fn redo_remove_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.remove_shot("shot-001").unwrap();

    doc.undo().unwrap();
    assert_eq!(doc.snapshot.shots.len(), 2);

    doc.redo().unwrap();
    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-002");
}

#[test]
fn redo_note_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_note("shot-001", "A note").unwrap();

    doc.undo().unwrap();
    assert!(doc.snapshot.shots[0].notes.is_empty());

    doc.redo().unwrap();
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "A note");
}

// -- Persistence roundtrip after redo -----------------------------------------

#[test]
fn save_load_after_undo_redo() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("redo.edit.json");

    let mut doc = three_shot_doc();
    let original = doc.snapshot.clone();
    doc.undo().unwrap();
    doc.redo().unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.head, 2);
    assert_eq!(loaded.snapshot, original);
    assert_eq!(loaded.ops.len(), 3);

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}
