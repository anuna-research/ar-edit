//! TEST-014: Removal (remove_shot)
//!
//! Verifies that `remove_shot()` removes the shot from the snapshot, returns
//! the full shot data, stores it in the op for undo, and persists correctly.

use ar_edit_core::edit::EditError;
use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

// -- Basic removal ------------------------------------------------------------

#[test]
fn remove_shot_decreases_snapshot_count() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();

    doc.remove_shot("shot-001").unwrap();

    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-002");
}

#[test]
fn remove_shot_returns_full_shot_data() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();

    let removed = doc.remove_shot("shot-001").unwrap();
    assert_eq!(removed.id, "shot-001");
    assert_eq!(removed.source, "src-001");
    assert_eq!(removed.range, ShotRange::Words { from: 0, to: 52 });
}

#[test]
fn remove_preserves_shot_data_in_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.remove_shot("shot-001").unwrap();

    match &doc.ops[1].op {
        EditOpKind::RemoveShot { shot_id, shot } => {
            assert_eq!(shot_id, "shot-001");
            assert_eq!(shot.source, "src-001");
            assert_eq!(shot.range, ShotRange::Words { from: 0, to: 52 });
        }
        other => panic!("expected RemoveShot, got {other:?}"),
    }
}

// -- Multiple removals --------------------------------------------------------

#[test]
fn remove_all_shots_leaves_empty_snapshot() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();
    doc.add_shot(
        "src-003",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();

    doc.remove_shot("shot-001").unwrap();
    doc.remove_shot("shot-002").unwrap();
    doc.remove_shot("shot-003").unwrap();

    assert!(doc.snapshot.shots.is_empty());
    assert_eq!(doc.ops.len(), 6); // 3 adds + 3 removes
}

#[test]
fn remove_middle_preserves_order_of_remaining() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Words { from: 11, to: 20 })
        .unwrap();
    doc.add_shot("src-003", ShotRange::Words { from: 21, to: 30 })
        .unwrap();

    doc.remove_shot("shot-002").unwrap();

    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-003");
}

// -- Error cases --------------------------------------------------------------

#[test]
fn remove_nonexistent_shot_errors() {
    let mut doc = EditDocument::create("test");
    let err = doc.remove_shot("shot-999").unwrap_err();
    assert!(matches!(err, EditError::ShotNotFound(_)));
}

#[test]
fn remove_same_shot_twice_errors() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();

    doc.remove_shot("shot-001").unwrap();
    let err = doc.remove_shot("shot-001").unwrap_err();
    assert!(matches!(err, EditError::ShotNotFound(_)));
}

// -- Head tracking ------------------------------------------------------------

#[test]
fn head_advances_after_remove() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    assert_eq!(doc.head, 0);

    doc.remove_shot("shot-001").unwrap();
    assert_eq!(doc.head, 1);
}

// -- Persistence roundtrip ----------------------------------------------------

#[test]
fn save_load_roundtrip_after_removal() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("removed.edit.json");

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();
    doc.remove_shot("shot-001").unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.snapshot.shots.len(), 1);
    assert_eq!(loaded.snapshot.shots[0].id, "shot-002");
    assert_eq!(loaded.ops.len(), 3);

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}

#[test]
fn removed_shot_data_survives_persistence() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("removed-data.edit.json");

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.remove_shot("shot-001").unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    match &loaded.ops[1].op {
        EditOpKind::RemoveShot { shot_id, shot } => {
            assert_eq!(shot_id, "shot-001");
            assert_eq!(shot.source, "src-001");
            assert_eq!(shot.range, ShotRange::Words { from: 0, to: 52 });
        }
        other => panic!("expected RemoveShot, got {other:?}"),
    }
}
