//! TEST-012: Segment (shot) addition for all 3 range types
//!
//! Verifies that `add_shot()` correctly appends shots with Words, Scenes, and
//! Time ranges, assigns sequential IDs, and persists through save/load.

use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

// -- Words range --------------------------------------------------------------

#[test]
fn add_shot_words_range() {
    let mut doc = EditDocument::create("test");
    let shot = doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();

    assert_eq!(shot.id, "shot-001");
    assert_eq!(shot.source, "src-001");
    assert_eq!(shot.range, ShotRange::Words { from: 0, to: 52 });
    assert!(shot.notes.is_empty());

    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.ops.len(), 1);
    assert_eq!(doc.head, 0);
}

// -- Scenes range -------------------------------------------------------------

#[test]
fn add_shot_scenes_range() {
    let mut doc = EditDocument::create("test");
    let shot = doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 3 }).unwrap();

    assert_eq!(shot.id, "shot-001");
    assert_eq!(shot.source, "src-002");
    assert_eq!(shot.range, ShotRange::Scenes { from: 0, to: 3 });
}

// -- Time range ---------------------------------------------------------------

#[test]
fn add_shot_time_range() {
    let mut doc = EditDocument::create("test");
    let shot = doc.add_shot("src-003", ShotRange::Time { from_ms: 15000, to_ms: 22000 }).unwrap();

    assert_eq!(shot.id, "shot-001");
    assert_eq!(shot.source, "src-003");
    assert_eq!(shot.range, ShotRange::Time { from_ms: 15000, to_ms: 22000 });
}

// -- Mixed types in one document ----------------------------------------------

#[test]
fn add_shots_all_three_types() {
    let mut doc = EditDocument::create("mixed");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 10000 }).unwrap();

    assert_eq!(doc.snapshot.shots.len(), 3);
    assert_eq!(doc.snapshot.shots[0].range, ShotRange::Words { from: 0, to: 52 });
    assert_eq!(doc.snapshot.shots[1].range, ShotRange::Scenes { from: 0, to: 2 });
    assert_eq!(doc.snapshot.shots[2].range, ShotRange::Time { from_ms: 5000, to_ms: 10000 });
}

// -- Sequential ID assignment -------------------------------------------------

#[test]
fn shot_ids_increment_sequentially() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 11, to: 20 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 21, to: 30 }).unwrap();

    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    assert_eq!(doc.snapshot.shots[2].id, "shot-003");
    assert_eq!(doc.next_shot_id, 4);
}

// -- Op record ----------------------------------------------------------------

#[test]
fn add_shot_creates_add_shot_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();

    assert_eq!(doc.ops.len(), 1);
    assert_eq!(doc.ops[0].id, 0);
    match &doc.ops[0].op {
        EditOpKind::AddShot { shot } => {
            assert_eq!(shot.id, "shot-001");
            assert_eq!(shot.source, "src-001");
        }
        other => panic!("expected AddShot op, got {other:?}"),
    }
}

#[test]
fn op_ids_are_sequential_across_adds() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 }).unwrap();
    doc.add_shot("src-003", ShotRange::Time { from_ms: 0, to_ms: 5000 }).unwrap();

    let ids: Vec<u32> = doc.ops.iter().map(|op| op.id).collect();
    assert_eq!(ids, vec![0, 1, 2]);
}

// -- Head tracking ------------------------------------------------------------

#[test]
fn head_advances_with_each_add() {
    let mut doc = EditDocument::create("test");
    assert_eq!(doc.head, -1);

    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 10 }).unwrap();
    assert_eq!(doc.head, 0);

    doc.add_shot("src-001", ShotRange::Words { from: 11, to: 20 }).unwrap();
    assert_eq!(doc.head, 1);

    doc.add_shot("src-001", ShotRange::Words { from: 21, to: 30 }).unwrap();
    assert_eq!(doc.head, 2);
}

// -- Persistence roundtrip ----------------------------------------------------

#[test]
fn save_load_roundtrip_all_range_types() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("mixed.edit.json");

    let mut doc = EditDocument::create("mixed");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-003", ShotRange::Time { from_ms: 15000, to_ms: 22000 }).unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.name, "mixed");
    assert_eq!(loaded.head, 2);
    assert_eq!(loaded.ops.len(), 3);
    assert_eq!(loaded.next_shot_id, 4);
    assert_eq!(loaded.snapshot.shots.len(), 3);
    assert_eq!(loaded.snapshot, doc.snapshot);
}

#[test]
fn recompute_matches_snapshot_after_adds() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-003", ShotRange::Time { from_ms: 5000, to_ms: 10000 }).unwrap();

    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}
