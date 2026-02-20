//! TEST-015: Trimming (trim_shot)
//!
//! Verifies that `trim_shot()` updates a shot's range, stores the old and new
//! range in the op, and persists correctly for all range types.

use ar_edit_core::edit::EditError;
use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

// -- Words trimming -----------------------------------------------------------

#[test]
fn trim_words_range() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();

    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();

    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 10, to: 90 }
    );
}

#[test]
fn trim_words_op_stores_old_and_new() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();

    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();

    match &doc.ops[1].op {
        EditOpKind::TrimShot {
            shot_id,
            old_range,
            new_range,
        } => {
            assert_eq!(shot_id, "shot-001");
            assert_eq!(*old_range, ShotRange::Words { from: 0, to: 100 });
            assert_eq!(*new_range, ShotRange::Words { from: 10, to: 90 });
        }
        other => panic!("expected TrimShot, got {other:?}"),
    }
}

// -- Scenes trimming ----------------------------------------------------------

#[test]
fn trim_scenes_range() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Scenes { from: 0, to: 5 }).unwrap();

    doc.trim_shot("shot-001", ShotRange::Scenes { from: 1, to: 4 })
        .unwrap();

    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Scenes { from: 1, to: 4 }
    );
}

// -- Time trimming ------------------------------------------------------------

#[test]
fn trim_time_range() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 60000 }).unwrap();

    doc.trim_shot(
        "shot-001",
        ShotRange::Time {
            from_ms: 5000,
            to_ms: 55000,
        },
    )
    .unwrap();

    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Time {
            from_ms: 5000,
            to_ms: 55000
        }
    );
}

// -- Multiple trims -----------------------------------------------------------

#[test]
fn trim_same_shot_twice() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();

    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 20, to: 80 })
        .unwrap();

    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 20, to: 80 }
    );

    // Second trim should record the intermediate range as old
    match &doc.ops[2].op {
        EditOpKind::TrimShot {
            old_range,
            new_range,
            ..
        } => {
            assert_eq!(*old_range, ShotRange::Words { from: 10, to: 90 });
            assert_eq!(*new_range, ShotRange::Words { from: 20, to: 80 });
        }
        other => panic!("expected TrimShot, got {other:?}"),
    }
}

// -- Error cases --------------------------------------------------------------

#[test]
fn trim_nonexistent_shot_errors() {
    let mut doc = EditDocument::create("test");
    let err = doc
        .trim_shot("shot-999", ShotRange::Words { from: 0, to: 10 })
        .unwrap_err();
    assert!(matches!(err, EditError::ShotNotFound(_)));
}

// -- Head tracking ------------------------------------------------------------

#[test]
fn head_advances_after_trim() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();
    assert_eq!(doc.head, 0);

    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();
    assert_eq!(doc.head, 1);
}

// -- Persistence roundtrip ----------------------------------------------------

#[test]
fn save_load_roundtrip_after_trim() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("trimmed.edit.json");

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 100 }).unwrap();
    doc.add_shot("src-002", ShotRange::Time { from_ms: 0, to_ms: 60000 }).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 10, to: 90 })
        .unwrap();
    doc.trim_shot(
        "shot-002",
        ShotRange::Time {
            from_ms: 5000,
            to_ms: 55000,
        },
    )
    .unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(
        loaded.snapshot.shots[0].range,
        ShotRange::Words { from: 10, to: 90 }
    );
    assert_eq!(
        loaded.snapshot.shots[1].range,
        ShotRange::Time {
            from_ms: 5000,
            to_ms: 55000
        }
    );
    assert_eq!(loaded.ops.len(), 4);

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}
