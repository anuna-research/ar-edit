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
