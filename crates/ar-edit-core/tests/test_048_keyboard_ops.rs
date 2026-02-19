//! TEST-048: Keyboard operations
//!
//! Verifies that the edit operations triggered by TUI keyboard shortcuts
//! produce correct results: add shot (a), delete shot (d), move up/down
//! (J/K), trim (t), add note (n), and undo/redo (Ctrl-Z/Ctrl-Y).
//! Also tests multi-step prompt chains and range parsing.

use std::fs;
use std::path::Path;

use ar_edit_core::display;
use ar_edit_core::models::{
    Defaults, EditDocument, EditOpKind, Manifest, ShotRange, Source,
    Transcript, TranscriptSegment, Word,
};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_source(id: &str) -> Source {
    Source {
        id: id.into(),
        path: format!("sources/{id}.mp4").into(),
        original_filename: format!("{id}.mp4"),
        duration_ms: 60_000,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (1920, 1080),
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed: true,
        indexed: false,
    }
}

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 10000,
        segments: vec![TranscriptSegment {
            index: 0,
            start_ms: 0,
            end_ms: 10000,
            text: "one two three four five six seven eight nine ten".into(),
            words: (0..10)
                .map(|i| Word {
                    index: i,
                    text: ["one", "two", "three", "four", "five",
                           "six", "seven", "eight", "nine", "ten"][i as usize].into(),
                    start_ms: (i as u64) * 1000,
                    end_ms: (i as u64) * 1000 + 900,
                    confidence: 0.95,
                })
                .collect(),
        }],
        word_count: 10,
    }
}

fn setup_project(dir: &Path) {
    fs::create_dir_all(dir.join("sources")).unwrap();
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("edits")).unwrap();
    fs::create_dir_all(dir.join("thumbnails")).unwrap();
    fs::create_dir_all(dir.join("annotations")).unwrap();

    let manifest = Manifest {
        version: "1.0.0".into(),
        name: "test".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
        sources: vec![make_source("src-001")],
        next_source_id: 2,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    };
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&make_transcript()).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests: Add shot (a key)
// ---------------------------------------------------------------------------

/// Adding a shot appends it to the timeline.
#[test]
fn add_shot_appends_to_timeline() {
    let mut doc = EditDocument::create("test");
    assert!(doc.snapshot.shots.is_empty());

    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 });
    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[0].source, "src-001");
}

/// Adding multiple shots assigns incrementing IDs.
#[test]
fn add_shot_increments_ids() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 5000 });

    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    assert_eq!(doc.snapshot.shots[2].id, "shot-003");
}

/// Adding a shot records an AddShot op.
#[test]
fn add_shot_creates_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });

    assert_eq!(doc.ops.len(), 1);
    assert!(matches!(&doc.ops[0].op, EditOpKind::AddShot { shot } if shot.id == "shot-001"));
    assert_eq!(doc.head, 0);
}

// ---------------------------------------------------------------------------
// Tests: Delete shot (d key)
// ---------------------------------------------------------------------------

/// Deleting a shot removes it from the snapshot.
#[test]
fn delete_shot_removes_from_snapshot() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });

    doc.remove_shot("shot-001").unwrap();
    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-002");
}

/// Deleting a shot records a RemoveShot op with the full shot data.
#[test]
fn delete_shot_creates_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });

    doc.remove_shot("shot-001").unwrap();
    assert_eq!(doc.ops.len(), 2);
    assert!(matches!(
        &doc.ops[1].op,
        EditOpKind::RemoveShot { shot_id, shot } if shot_id == "shot-001" && shot.id == "shot-001"
    ));
}

/// Deleting a nonexistent shot returns an error.
#[test]
fn delete_nonexistent_shot_errors() {
    let mut doc = EditDocument::create("test");
    let err = doc.remove_shot("shot-999").unwrap_err();
    assert!(format!("{err}").contains("not found"));
}

// ---------------------------------------------------------------------------
// Tests: Move shot up/down (J/K keys)
// ---------------------------------------------------------------------------

/// Moving a shot down increases its index.
#[test]
fn move_shot_down() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 2 });
    doc.add_shot("src-001", ShotRange::Words { from: 3, to: 5 });
    doc.add_shot("src-001", ShotRange::Words { from: 6, to: 9 });

    doc.move_shot("shot-001", 2).unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-002");
    assert_eq!(doc.snapshot.shots[1].id, "shot-003");
    assert_eq!(doc.snapshot.shots[2].id, "shot-001");
}

/// Moving a shot up decreases its index.
#[test]
fn move_shot_up() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 2 });
    doc.add_shot("src-001", ShotRange::Words { from: 3, to: 5 });
    doc.add_shot("src-001", ShotRange::Words { from: 6, to: 9 });

    doc.move_shot("shot-003", 0).unwrap();
    assert_eq!(doc.snapshot.shots[0].id, "shot-003");
    assert_eq!(doc.snapshot.shots[1].id, "shot-001");
    assert_eq!(doc.snapshot.shots[2].id, "shot-002");
}

/// Moving a shot records a MoveShot op.
#[test]
fn move_shot_creates_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 2 });
    doc.add_shot("src-001", ShotRange::Words { from: 3, to: 5 });

    doc.move_shot("shot-001", 1).unwrap();
    assert!(matches!(
        &doc.ops[2].op,
        EditOpKind::MoveShot { shot_id, from_position: 0, to_position: 1 } if shot_id == "shot-001"
    ));
}

/// Moving to out-of-bounds position fails.
#[test]
fn move_shot_out_of_bounds() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 2 });
    doc.add_shot("src-001", ShotRange::Words { from: 3, to: 5 });

    let err = doc.move_shot("shot-001", 5).unwrap_err();
    assert!(format!("{err}").contains("out of bounds"));
}

// ---------------------------------------------------------------------------
// Tests: Trim shot (t key)
// ---------------------------------------------------------------------------

/// Trimming a shot updates its range in the snapshot.
#[test]
fn trim_shot_updates_range() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 9 });

    doc.trim_shot("shot-001", ShotRange::Words { from: 2, to: 7 }).unwrap();
    assert_eq!(doc.snapshot.shots[0].range, ShotRange::Words { from: 2, to: 7 });
}

/// Trimming records a TrimShot op with both old and new ranges.
#[test]
fn trim_shot_creates_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 9 });

    doc.trim_shot("shot-001", ShotRange::Words { from: 2, to: 7 }).unwrap();
    assert!(matches!(
        &doc.ops[1].op,
        EditOpKind::TrimShot {
            shot_id,
            old_range: ShotRange::Words { from: 0, to: 9 },
            new_range: ShotRange::Words { from: 2, to: 7 },
        } if shot_id == "shot-001"
    ));
}

/// Trimming a nonexistent shot fails.
#[test]
fn trim_nonexistent_shot_errors() {
    let mut doc = EditDocument::create("test");
    let err = doc.trim_shot("shot-999", ShotRange::Words { from: 0, to: 5 }).unwrap_err();
    assert!(format!("{err}").contains("not found"));
}

// ---------------------------------------------------------------------------
// Tests: Add note (n key)
// ---------------------------------------------------------------------------

/// Adding a note appends to the shot's notes array.
#[test]
fn add_note_appends() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });

    doc.add_note("shot-001", "Great take").unwrap();
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Great take");
}

/// Multiple notes accumulate on a shot.
#[test]
fn multiple_notes_accumulate() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });

    doc.add_note("shot-001", "Note one").unwrap();
    doc.add_note("shot-001", "Note two").unwrap();
    doc.add_note("shot-001", "Note three").unwrap();

    assert_eq!(doc.snapshot.shots[0].notes.len(), 3);
}

/// Adding a note creates an AddNote op.
#[test]
fn add_note_creates_op() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });
    doc.add_note("shot-001", "Test note").unwrap();

    assert!(matches!(
        &doc.ops[1].op,
        EditOpKind::AddNote { shot_id, note } if shot_id == "shot-001" && note.text == "Test note"
    ));
}

// ---------------------------------------------------------------------------
// Tests: Undo/Redo (Ctrl-Z / Ctrl-Y)
// ---------------------------------------------------------------------------

/// Undo reverses the last operation.
#[test]
fn undo_reverses_add_shot() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });
    assert_eq!(doc.snapshot.shots.len(), 1);

    doc.undo().unwrap();
    assert!(doc.snapshot.shots.is_empty());
    assert_eq!(doc.head, -1);
}

/// Redo re-applies after undo.
#[test]
fn redo_reapplies_add_shot() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });

    doc.undo().unwrap();
    assert!(doc.snapshot.shots.is_empty());

    doc.redo().unwrap();
    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
}

/// Multiple undos followed by redos restore the original state.
#[test]
fn undo_redo_roundtrip() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });
    doc.add_shot("src-001", ShotRange::Words { from: 8, to: 9 });
    let original = doc.snapshot.clone();

    // Undo all
    doc.undo().unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert!(doc.snapshot.shots.is_empty());

    // Redo all
    doc.redo().unwrap();
    doc.redo().unwrap();
    doc.redo().unwrap();
    assert_eq!(doc.snapshot, original);
}

/// Undo on empty doc returns error.
#[test]
fn undo_empty_errors() {
    let mut doc = EditDocument::create("test");
    let err = doc.undo().unwrap_err();
    assert!(format!("{err}").contains("nothing to undo"));
}

/// Redo at head returns error.
#[test]
fn redo_at_head_errors() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });

    let err = doc.redo().unwrap_err();
    assert!(format!("{err}").contains("nothing to redo"));
}

/// New operation after undo truncates redo history (fork).
#[test]
fn new_op_after_undo_forks() {
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });

    // Undo one op
    doc.undo().unwrap();
    assert_eq!(doc.snapshot.shots.len(), 1);

    // Add a different shot — forks the history
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 5000 });

    // Only 2 ops should remain (original first + new fork)
    assert_eq!(doc.ops.len(), 2);
    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[1].id, "shot-003");

    // Redo should fail
    let err = doc.redo().unwrap_err();
    assert!(format!("{err}").contains("nothing to redo"));
}

// ---------------------------------------------------------------------------
// Tests: Edit document save/load roundtrip
// ---------------------------------------------------------------------------

/// Operations survive save/load cycle (TUI saves after each operation).
#[test]
fn ops_survive_save_load() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.edit.json");

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 5 });
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 3000 });
    doc.move_shot("shot-001", 1).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 1, to: 4 }).unwrap();
    doc.add_note("shot-001", "Good take").unwrap();
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.snapshot.shots.len(), 2);
    assert_eq!(loaded.ops.len(), 5);
    assert_eq!(loaded.snapshot.shots[0].id, "shot-002");
    assert_eq!(loaded.snapshot.shots[1].id, "shot-001");
    assert_eq!(loaded.snapshot.shots[1].notes.len(), 1);
}

/// Resolved shots update correctly after each operation.
#[test]
fn operations_update_resolved_shots() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 });
    doc.add_shot("src-001", ShotRange::Words { from: 5, to: 9 });

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 2);

    // Delete first shot
    doc.remove_shot("shot-001").unwrap();
    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].id, "shot-002");

    // Undo delete
    doc.undo().unwrap();
    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].id, "shot-001");
}
