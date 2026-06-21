//! Behavioural coverage for the CRDT primitives (SPEC-003) — POIs and markers as
//! OR-sets (REQ-082), notes per shot, minted-tag/counter reuse across reload
//! (REQ-080), and the version vector. Written to give the mutation suite teeth
//! on `crdt.rs` (PROTO-001 mandates mutation testing for AI-synthesised code).
//!
//! Validates: [[SPEC-003-realtime-collaborative-editing#REQ-080]],
//! [[SPEC-003-realtime-collaborative-editing#REQ-082]],
//! [[SPEC-003-realtime-collaborative-editing#REQ-083]].

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::materialise::materialise;
use ar_edit_core::models::{ShotNote, ShotRange};

fn words(from: u32, to: u32) -> ShotRange {
    ShotRange::Words { from, to }
}

fn note(text: &str) -> ShotNote {
    ShotNote {
        author: String::new(),
        text: text.into(),
        created: chrono::Utc::now(),
    }
}

/// REQ-091: a shot's author and a note's author flow through the CRDT into the
/// materialised view (collaborative attribution).
#[test]
fn shot_and_note_authors_flow_through_the_crdt() {
    let doc = CollabDoc::new(ActorId(1));
    let id = doc.add_new_shot("src-1", &words(0, 10), "alice");
    doc.add_note(
        &id,
        &ShotNote {
            text: "hi".into(),
            author: "bob".into(),
            created: chrono::Utc::now(),
        },
    );
    let snap = materialise(&doc);
    assert_eq!(snap.shots[0].author, "alice", "shot author materialised");
    assert_eq!(
        snap.shots[0].notes[0].author, "bob",
        "note author materialised"
    );
}

/// REQ-082/083: POIs are an observed-remove set that converges and whose removes
/// propagate — symmetric with markers, and now flowing through the CRDT.
#[test]
fn pois_converge_and_removes_propagate() {
    let a = CollabDoc::new(ActorId(1));
    let b = CollabDoc::new(ActorId(2));
    a.put_poi("poi-1", r#"{"cat":"highlight"}"#);
    a.put_poi("poi-2", r#"{"cat":"issue"}"#);
    b.put_poi("poi-3", r#"{"cat":"highlight"}"#);
    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    assert_eq!(a.poi_ids(), b.poi_ids(), "POI sets converge");
    assert_eq!(a.poi_ids().len(), 3, "all concurrent POI adds survive");

    a.remove_poi("poi-1");
    b.import(&a.export_snapshot()).unwrap();
    assert!(
        !b.poi_ids().contains(&"poi-1".to_string()),
        "POI remove propagated"
    );
    assert_eq!(b.poi_ids().len(), 2);
}

/// REQ-082: a note attaches to its OWN shot, and removing a different shot must
/// NOT delete this shot's notes (guards `note_ids_for`'s shot-id match).
#[test]
fn notes_attach_to_their_shot_and_survive_other_removals() {
    let doc = CollabDoc::new(ActorId(1));
    let s1 = doc.add_new_shot("src-1", &words(0, 10), "");
    let s2 = doc.add_new_shot("src-2", &words(0, 10), "");
    doc.add_note(&s1, &note("a"));
    doc.add_note(&s1, &note("b"));
    doc.add_note(&s2, &note("c"));

    let snap = materialise(&doc);
    let n1 = snap.shots.iter().find(|s| s.id == s1).unwrap().notes.len();
    let n2 = snap.shots.iter().find(|s| s.id == s2).unwrap().notes.len();
    assert_eq!((n1, n2), (2, 1), "notes attach to the correct shot");

    // Removing s2 must delete only s2's notes — s1 keeps both.
    doc.remove_shot(&s2);
    let snap = materialise(&doc);
    assert!(snap.shots.iter().all(|s| s.id != s2), "s2 removed");
    assert_eq!(
        snap.shots.iter().find(|s| s.id == s1).unwrap().notes.len(),
        2,
        "removing another shot did not touch this shot's notes"
    );
}

/// REQ-082 (OR-set): a concurrent re-add carries a fresh instance tag the remove
/// never observed, so the member SURVIVES the merge — only true if minted tags
/// are unique (guards `mint_orset_tag`).
#[test]
fn concurrent_readd_survives_a_remove() {
    let a = CollabDoc::new(ActorId(1));
    let b = CollabDoc::new(ActorId(2));
    a.put_marker("m1", r#"{"v":1}"#);
    b.import(&a.export_snapshot()).unwrap(); // b observes the original instance

    b.remove_marker("m1"); // b tombstones the instance it saw
    a.put_marker("m1", r#"{"v":2}"#); // a re-adds with a FRESH tag (concurrent)

    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    assert!(
        a.marker_ids().contains(&"m1".to_string()),
        "re-add survived the unobserved remove"
    );
    assert_eq!(a.marker_ids(), b.marker_ids(), "and both peers converge");
}

/// The version vector reflects applied operations — a doc with edits is not at
/// the empty version (guards `version`).
#[test]
fn version_reflects_applied_ops() {
    let empty = CollabDoc::new(ActorId(1));
    let active = CollabDoc::new(ActorId(1));
    active.add_new_shot("src-1", &words(0, 10), "");
    assert_ne!(
        active.version(),
        empty.version(),
        "version advances with edits"
    );
}

/// REQ-080: after a reload (import into a fresh doc), the minted-shot counter is
/// synced past the highest existing id, so the next mint cannot collide (guards
/// `sync_shot_counter`'s max+1).
#[test]
fn shot_counter_sync_prevents_reuse_after_reload() {
    let a = CollabDoc::new(ActorId(0xABC));
    let id0 = a.add_new_shot("src", &words(0, 5), "");
    let id1 = a.add_new_shot("src", &words(0, 5), "");

    let b = CollabDoc::new(ActorId(0xABC));
    b.import(&a.export_snapshot()).unwrap();
    let id2 = b.add_new_shot("src", &words(0, 5), "");

    assert_ne!(id2, id0);
    assert_ne!(
        id2, id1,
        "the next mint after reload does not reuse an existing id"
    );
}

/// REQ-080: same for the note counter — a note added after a reload gets a fresh
/// id and does not overwrite an existing note (guards `sync_note_counter`).
#[test]
fn note_counter_sync_prevents_reuse_after_reload() {
    let a = CollabDoc::new(ActorId(0xABC));
    let s = a.add_new_shot("src", &words(0, 10), "");
    a.add_note(&s, &note("n0"));
    a.add_note(&s, &note("n1"));

    let b = CollabDoc::new(ActorId(0xABC));
    b.import(&a.export_snapshot()).unwrap();
    b.add_note(&s, &note("n2"));

    let snap = materialise(&b);
    let notes = &snap.shots.iter().find(|x| x.id == s).unwrap().notes;
    assert_eq!(
        notes.len(),
        3,
        "the post-reload note did not overwrite an existing one"
    );
}
