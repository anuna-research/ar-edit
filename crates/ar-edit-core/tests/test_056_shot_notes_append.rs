//! TEST-056: Shot notes append-only
//!
//! Verifies that shot notes are strictly append-only: new notes are always
//! added at the end, multiple notes accumulate, notes survive persistence
//! roundtrips, and undo/redo correctly manages note state via event replay.

use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn append_single_note() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });

    let note = doc.add_note("shot-001", "Good take").unwrap();
    assert_eq!(note.text, "Good take");

    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Good take");
}

#[test]
fn append_multiple_notes_preserves_order() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });

    doc.add_note("shot-001", "First observation").unwrap();
    doc.add_note("shot-001", "Second thought").unwrap();
    doc.add_note("shot-001", "Final decision: keep").unwrap();

    let notes = &doc.snapshot.shots[0].notes;
    assert_eq!(notes.len(), 3);
    assert_eq!(notes[0].text, "First observation");
    assert_eq!(notes[1].text, "Second thought");
    assert_eq!(notes[2].text, "Final decision: keep");
}

#[test]
fn notes_on_different_shots() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 30 });
    doc.add_shot("src-001", ShotRange::Words { from: 31, to: 60 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

    doc.add_note("shot-001", "Trim opening").unwrap();
    doc.add_note("shot-002", "Great content").unwrap();
    doc.add_note("shot-003", "B-roll candidate").unwrap();
    doc.add_note("shot-001", "Actually keep the opening").unwrap();

    assert_eq!(doc.snapshot.shots[0].notes.len(), 2);
    assert_eq!(doc.snapshot.shots[1].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[2].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[1].text, "Actually keep the opening");
}

#[test]
fn note_generates_add_note_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "Review later").unwrap();

    assert_eq!(doc.ops.len(), 2);
    match &doc.ops[1].op {
        EditOpKind::AddNote { shot_id, note } => {
            assert_eq!(shot_id, "shot-001");
            assert_eq!(note.text, "Review later");
        }
        other => panic!("expected AddNote, got {other:?}"),
    }
}

#[test]
fn note_timestamps_are_monotonic() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });

    doc.add_note("shot-001", "First").unwrap();
    doc.add_note("shot-001", "Second").unwrap();

    let notes = &doc.snapshot.shots[0].notes;
    assert!(notes[0].created <= notes[1].created);
}

#[test]
fn note_on_nonexistent_shot_fails() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });

    let err = doc.add_note("shot-999", "orphan note").unwrap_err();
    assert!(format!("{err}").contains("shot-999"));
}

#[test]
fn undo_removes_last_note() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "Keep this").unwrap();
    doc.add_note("shot-001", "Remove this").unwrap();

    assert_eq!(doc.snapshot.shots[0].notes.len(), 2);

    doc.undo().unwrap(); // undo second note
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Keep this");
}

#[test]
fn undo_all_notes_restores_empty() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "Note A").unwrap();
    doc.add_note("shot-001", "Note B").unwrap();

    doc.undo().unwrap(); // undo Note B
    doc.undo().unwrap(); // undo Note A

    assert!(doc.snapshot.shots[0].notes.is_empty());
}

#[test]
fn redo_restores_note() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "Important note").unwrap();

    doc.undo().unwrap();
    assert!(doc.snapshot.shots[0].notes.is_empty());

    doc.redo().unwrap();
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Important note");
}

#[test]
fn undo_redo_roundtrip_preserves_notes() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "First").unwrap();
    doc.add_note("shot-001", "Second").unwrap();
    doc.add_note("shot-001", "Third").unwrap();

    let original = doc.snapshot.clone();

    // Undo all notes
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert!(doc.snapshot.shots[0].notes.is_empty());

    // Redo all notes
    doc.redo().unwrap();
    doc.redo().unwrap();
    doc.redo().unwrap();
    assert_eq!(doc.snapshot, original);
}

#[test]
fn new_op_after_undo_discards_redo_notes() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "Original note").unwrap();

    doc.undo().unwrap(); // undo note
    doc.add_note("shot-001", "Replacement note").unwrap();

    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Replacement note");

    // Redo should fail — the original note was discarded
    let err = doc.redo().unwrap_err();
    assert!(format!("{err}").contains("redo"));
}

#[test]
fn notes_persist_through_save_load() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.edit.json");

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "Director's note: perfect delivery").unwrap();
    doc.add_note("shot-001", "Color grade needed").unwrap();

    doc.save(&path).unwrap();
    let loaded = EditDocument::load(&path).unwrap();

    assert_eq!(loaded.snapshot.shots[0].notes.len(), 2);
    assert_eq!(
        loaded.snapshot.shots[0].notes[0].text,
        "Director's note: perfect delivery"
    );
    assert_eq!(loaded.snapshot.shots[0].notes[1].text, "Color grade needed");
}

#[test]
fn notes_recompute_matches_snapshot() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 });
    doc.add_note("shot-001", "Note on shot 1").unwrap();
    doc.add_note("shot-002", "Note on shot 2").unwrap();
    doc.add_note("shot-001", "Another note on shot 1").unwrap();

    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
    assert_eq!(recomputed.shots[0].notes.len(), 2);
    assert_eq!(recomputed.shots[1].notes.len(), 1);
}

#[test]
fn notes_in_json_structure() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.edit.json");

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 50 });
    doc.add_note("shot-001", "A note").unwrap();

    doc.save(&path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Verify notes appear in snapshot
    let shot = &json["snapshot"]["shots"][0];
    assert_eq!(shot["notes"][0]["text"], "A note");
    assert!(shot["notes"][0]["created"].is_string());

    // Verify AddNote op in ops array
    let last_op = &json["ops"][1];
    assert_eq!(last_op["op"], "add_note");
    assert_eq!(last_op["shot_id"], "shot-001");
    assert_eq!(last_op["note"]["text"], "A note");
}
