//! Per-actor undo (SPEC-003 REQ-086, TEST-105/106).

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::materialise::materialise;
use ar_edit_collab::undo::LocalUndo;
use ar_edit_core::models::{Shot, ShotRange};

fn shot(id: &str) -> Shot {
    Shot {
        id: id.into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 10 },
        notes: vec![],
    }
}

fn ids(c: &CollabDoc) -> Vec<String> {
    materialise(c).shots.into_iter().map(|s| s.id).collect()
}

/// Regression (P2, REQ-086): the guarded op-id stack stays aligned with Loro's
/// undo stack even past the retention bound, where the bounded stack evicts its
/// oldest item as it adds a new one and `undo_count()` stops growing. The NEWEST
/// op must remain undoable by its op id (a count-growth check would miss the
/// final push and the newest op would fail to undo or target the wrong step).
#[test]
fn guarded_undo_aligned_past_retention_bound() {
    // Exceed the manager's retained depth (1000) by one.
    const N: usize = 1001;
    let doc = CollabDoc::new(ActorId(77));
    let mut undo = LocalUndo::new(&doc);

    for i in 0..N {
        undo.begin();
        doc.add_shot(&shot(&format!("u-{i:04}")));
        assert!(undo.commit(format!("op-{i}")), "each add records a step");
    }

    // The oldest op was evicted, so it is not on top and must not undo.
    assert!(!undo.undo_if("op-0"), "evicted op id must not match the top");
    // The newest op is still on top and undoable — the push at the bound was
    // detected despite the constant undo_count.
    assert!(undo.undo_if(&format!("op-{}", N - 1)), "newest op stays undoable");
    assert!(
        !ids(&doc).contains(&"u-1000".to_string()),
        "undo of the newest op removed its shot"
    );
}

/// TEST-105/106: peer A's undo reverts only A's own change; peer B's
/// concurrent change is untouched.
#[test]
fn undo_is_per_actor() {
    let a = CollabDoc::new(ActorId(10));
    let mut undo_a = LocalUndo::new(&a);

    a.add_shot(&shot("shot-001")); // A's change (undoable by A)

    let b = CollabDoc::new(ActorId(20));
    b.import(&a.export_snapshot()).unwrap();
    b.add_shot(&shot("shot-002")); // B's change
    a.import(&b.export_snapshot()).unwrap();

    // Precondition: A sees both shots.
    assert!(ids(&a).contains(&"shot-001".to_string()));
    assert!(ids(&a).contains(&"shot-002".to_string()));

    // A undoes — only A's shot-001 should disappear.
    assert!(undo_a.undo(), "A had an undoable change");
    let after = ids(&a);
    assert!(
        !after.contains(&"shot-001".to_string()),
        "A's own change should be undone: {after:?}"
    );
    assert!(
        after.contains(&"shot-002".to_string()),
        "B's change must survive A's undo (TEST-106): {after:?}"
    );
}

/// Regression (P2): updating an OR-set member (marker/POI) is an
/// observed-replace — tombstone the old instance, insert a fresh one. Those two
/// steps must commit as a SINGLE undo unit, so one `undo()` of an update
/// restores the previous instance rather than deleting the marker outright.
/// Previously the id-counter bump committed between the delete and the insert,
/// splitting them across two undo steps, so undoing an update lost the marker.
#[test]
fn undo_of_marker_update_restores_marker() {
    let a = CollabDoc::new(ActorId(10));
    a.put_marker("m1", r#"{"v":1}"#); // initial add (committed)

    // Track undo from here: only the update below should be undoable.
    let mut undo = LocalUndo::new(&a);
    a.put_marker("m1", r#"{"v":2}"#); // update == observed-replace
    assert_eq!(a.marker_ids(), vec!["m1".to_string()]);

    assert!(undo.undo(), "the update is one undoable unit");
    assert_eq!(
        a.marker_ids(),
        vec!["m1".to_string()],
        "undoing a marker update must restore the previous instance, not delete the marker"
    );
}

/// Single-actor undo/redo behaves like the classic head-pointer model.
#[test]
fn single_actor_undo_redo() {
    let a = CollabDoc::new(ActorId(1));
    let mut undo = LocalUndo::new(&a);
    a.add_shot(&shot("shot-001"));
    a.add_shot(&shot("shot-002"));
    assert_eq!(ids(&a).len(), 2);

    assert!(undo.undo());
    assert_eq!(ids(&a), vec!["shot-001".to_string()]);
    assert!(undo.redo());
    assert_eq!(ids(&a).len(), 2);
}
