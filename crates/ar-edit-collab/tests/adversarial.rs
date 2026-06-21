//! Adversarial tests (PROTO-001 "Adversarial Testing — Post-Acceptance"):
//! cases aimed at BREAKING the spec — concurrent conflicts, boundary ranges,
//! hostile/corrupt input, and undo at the edges — rather than the happy path.
//! Each probes a requirement's unstated corners; survivors become regressions.
//!
//! Validates (adversarially): [[SPEC-003-realtime-collaborative-editing#REQ-080]],
//! [[SPEC-003-realtime-collaborative-editing#REQ-082]],
//! [[SPEC-003-realtime-collaborative-editing#REQ-083]],
//! [[SPEC-003-realtime-collaborative-editing#REQ-088]].

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::materialise::materialise;
use ar_edit_collab::store::PersistentEdit;
use ar_edit_core::models::ShotRange;

fn words(from: u32, to: u32) -> ShotRange {
    ShotRange::Words { from, to }
}

fn ids(e: &PersistentEdit) -> Vec<String> {
    e.snapshot().shots.into_iter().map(|s| s.id).collect()
}

/// REQ-083 adversarial: two peers concurrently REMOVE the same shot. The
/// observed-remove must converge to "gone" on both — never resurrect it, never
/// panic, never leave a half-deleted ghost.
#[test]
fn concurrent_remove_of_the_same_shot_converges_to_gone() {
    let a = CollabDoc::new(ActorId(1));
    let id = a.add_new_shot("src-1", &words(0, 10), "");
    let b = CollabDoc::new(ActorId(2));
    b.import(&a.export_snapshot()).unwrap();

    // Both remove the same shot independently.
    a.remove_shot(&id);
    b.remove_shot(&id);
    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    assert!(materialise(&a).shots.is_empty());
    assert_eq!(
        serde_json::to_value(materialise(&a)).unwrap(),
        serde_json::to_value(materialise(&b)).unwrap(),
    );
}

/// REQ-083 adversarial: two peers concurrently MOVE the same shot to different
/// positions. The result must converge deterministically and keep exactly one
/// copy of the shot (a move is not a delete+insert — REQ-080).
#[test]
fn concurrent_move_of_the_same_shot_converges_without_duplication() {
    let a = CollabDoc::new(ActorId(1));
    let id1 = a.add_new_shot("s1", &words(0, 5), "");
    let _id2 = a.add_new_shot("s2", &words(0, 5), "");
    let _id3 = a.add_new_shot("s3", &words(0, 5), "");
    let b = CollabDoc::new(ActorId(2));
    b.import(&a.export_snapshot()).unwrap();

    a.move_shot(&id1, 2); // peer A moves it to the end
    b.move_shot(&id1, 0); // peer B keeps it at the front
    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    let ma = materialise(&a);
    assert_eq!(ma.shots.len(), 3, "no duplication from concurrent move");
    assert_eq!(ma.shots.iter().filter(|s| s.id == id1).count(), 1);
    assert_eq!(
        serde_json::to_value(&ma).unwrap(),
        serde_json::to_value(materialise(&b)).unwrap(),
    );
}

/// REQ-082 adversarial: a note added concurrently with the shot's removal is
/// grow-only — removing the shot must NOT silently swallow a peer's note merge
/// into a panic or divergence.
#[test]
fn concurrent_note_and_remove_converges() {
    let a = CollabDoc::new(ActorId(1));
    let id = a.add_new_shot("src-1", &words(0, 10), "");
    let b = CollabDoc::new(ActorId(2));
    b.import(&a.export_snapshot()).unwrap();

    a.remove_shot(&id);
    b.add_note(
        &id,
        &ar_edit_core::models::ShotNote {
            author: String::new(),
            text: "late note".into(),
            created: chrono::Utc::now(),
        },
    );
    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    // Whatever the merge resolves to, both peers agree (SEC).
    assert_eq!(
        serde_json::to_value(materialise(&a)).unwrap(),
        serde_json::to_value(materialise(&b)).unwrap(),
    );
}

/// Boundary adversarial: zero-width and inverted ranges are rejected BEFORE any
/// mutation (the store stays unchanged), not silently accepted (REQ-081/CON).
#[test]
fn degenerate_ranges_are_rejected_without_mutating() {
    let mut e = PersistentEdit::create("p", ActorId(1));
    let good = e.add_shot("src-1", words(0, 10), None, "").unwrap();

    assert!(
        e.add_shot("src-2", words(5, 5), None, "").is_err(),
        "zero-width rejected"
    );
    assert!(
        e.add_shot("src-2", words(10, 3), None, "").is_err(),
        "inverted rejected"
    );
    assert!(
        e.trim_shot(&good, words(7, 7), None).is_err(),
        "zero-width trim rejected"
    );
    assert!(
        e.trim_shot(&good, words(9, 2), None).is_err(),
        "inverted trim rejected"
    );

    assert_eq!(ids(&e), vec![good], "no degenerate range mutated the store");
}

/// Edge adversarial: undo at the baseline and redo at the tip are no-ops that
/// return false — never panic, never move the cursor off the ends.
#[test]
fn undo_redo_at_the_edges_are_safe_no_ops() {
    let mut e = PersistentEdit::create("p", ActorId(1));
    assert!(!e.undo(), "undo on an empty store is a no-op");
    assert!(!e.redo(), "redo with no future is a no-op");

    e.add_shot("src-1", words(0, 10), None, "").unwrap();
    assert!(e.undo());
    assert!(!e.undo(), "cannot undo past the baseline");
    assert!(e.redo());
    assert!(!e.redo(), "cannot redo past the tip");
}

/// Hostile-input adversarial: a corrupt/truncated store fails CLOSED with an
/// error, never a panic (LangSec fail-closed at the persistence boundary).
#[test]
fn corrupt_store_bytes_fail_closed() {
    assert!(PersistentEdit::from_bytes(b"", ActorId(1)).is_err());
    assert!(PersistentEdit::from_bytes(b"{not json", ActorId(1)).is_err());
    // Valid OnDisk shape but a garbage base64 CRDT blob.
    let poisoned = br#"{"name":"x","created":"2020-01-01T00:00:00Z","actor":1,"crdt":"!!!not-base64!!!","undo_history":[],"undo_head":0,"snapshot":{"shots":[]}}"#;
    assert!(PersistentEdit::from_bytes(poisoned, ActorId(1)).is_err());
}

/// Migration adversarial: a legacy doc whose `head` points past the op log must
/// not panic or over-read — it clamps to what exists.
#[test]
fn legacy_head_beyond_oplog_does_not_panic() {
    let json = serde_json::json!({
        "name": "e",
        "created": "2020-01-01T00:00:00Z",
        "ops": [],
        "head": 9999,
        "snapshot": { "shots": [] }
    });
    let bytes = serde_json::to_vec(&json).unwrap();
    // Either migrates to an empty edit or rejects — but never panics.
    let _ = PersistentEdit::from_bytes(&bytes, ActorId(1));
}

/// REQ-080 adversarial: adding the SAME source repeatedly yields distinct shots
/// with distinct ids — content is not identity (a common CRDT modelling trap).
#[test]
fn repeated_identical_adds_are_distinct_shots() {
    let mut e = PersistentEdit::create("p", ActorId(1));
    let a = e.add_shot("src-1", words(0, 10), None, "").unwrap();
    let b = e.add_shot("src-1", words(0, 10), None, "").unwrap();
    let c = e.add_shot("src-1", words(0, 10), None, "").unwrap();
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
    assert_eq!(
        ids(&e).len(),
        3,
        "identical content does not collapse to one shot"
    );
}
