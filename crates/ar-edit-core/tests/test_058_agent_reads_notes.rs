//! TEST-058: Agent reads shot notes in edit show
//!
//! Verifies that when an edit document is resolved for display (as an agent
//! would see via `edit show --json`), shot notes are included in the resolved
//! output with correct text and timestamps.

use ar_edit_core::display::resolve_edit;
use ar_edit_core::models::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 30000,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 10000,
                text: "Welcome to the interview".into(),
                words: vec![
                    Word { index: 0, text: "Welcome".into(), start_ms: 0, end_ms: 500, confidence: 0.95 },
                    Word { index: 1, text: "to".into(), start_ms: 500, end_ms: 700, confidence: 0.97 },
                    Word { index: 2, text: "the".into(), start_ms: 700, end_ms: 900, confidence: 0.98 },
                    Word { index: 3, text: "interview".into(), start_ms: 900, end_ms: 1500, confidence: 0.96 },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 10000,
                end_ms: 20000,
                text: "Today we discuss climate policy".into(),
                words: vec![
                    Word { index: 4, text: "Today".into(), start_ms: 10000, end_ms: 10500, confidence: 0.94 },
                    Word { index: 5, text: "we".into(), start_ms: 10500, end_ms: 10700, confidence: 0.99 },
                    Word { index: 6, text: "discuss".into(), start_ms: 10700, end_ms: 11200, confidence: 0.93 },
                    Word { index: 7, text: "climate".into(), start_ms: 11200, end_ms: 11800, confidence: 0.91 },
                    Word { index: 8, text: "policy".into(), start_ms: 11800, end_ms: 12300, confidence: 0.92 },
                ],
            },
            TranscriptSegment {
                index: 2,
                start_ms: 20000,
                end_ms: 30000,
                text: "Thank you for watching".into(),
                words: vec![
                    Word { index: 9, text: "Thank".into(), start_ms: 20000, end_ms: 20500, confidence: 0.96 },
                    Word { index: 10, text: "you".into(), start_ms: 20500, end_ms: 20800, confidence: 0.97 },
                    Word { index: 11, text: "for".into(), start_ms: 20800, end_ms: 21000, confidence: 0.98 },
                    Word { index: 12, text: "watching".into(), start_ms: 21000, end_ms: 21600, confidence: 0.95 },
                ],
            },
        ],
        word_count: 13,
    }
}

fn setup_project(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("edits")).unwrap();

    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn resolved_shot_includes_single_note() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_note("shot-001", "Perfect delivery, keep as-is").unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].notes.len(), 1);
    assert_eq!(resolved[0].notes[0].text, "Perfect delivery, keep as-is");
}

#[test]
fn resolved_shot_includes_multiple_notes() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_note("shot-001", "Good energy").unwrap();
    doc.add_note("shot-001", "Trim last 2 seconds").unwrap();
    doc.add_note("shot-001", "Add lower-third graphic").unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].notes.len(), 3);
    assert_eq!(resolved[0].notes[0].text, "Good energy");
    assert_eq!(resolved[0].notes[1].text, "Trim last 2 seconds");
    assert_eq!(resolved[0].notes[2].text, "Add lower-third graphic");
}

#[test]
fn resolved_shot_without_notes_has_empty_vec() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert!(resolved[0].notes.is_empty());
}

#[test]
fn notes_on_multiple_shots_resolved_correctly() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 8 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 9, to: 12 }).unwrap();

    doc.add_note("shot-001", "Opening — strong").unwrap();
    doc.add_note("shot-002", "Core content").unwrap();
    doc.add_note("shot-002", "Needs color correction").unwrap();
    // shot-003 intentionally has no notes

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);
    assert_eq!(resolved[0].notes.len(), 1);
    assert_eq!(resolved[0].notes[0].text, "Opening — strong");
    assert_eq!(resolved[1].notes.len(), 2);
    assert_eq!(resolved[1].notes[0].text, "Core content");
    assert_eq!(resolved[1].notes[1].text, "Needs color correction");
    assert!(resolved[2].notes.is_empty());
}

#[test]
fn notes_preserved_alongside_text_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_note("shot-001", "Agent feedback: great intro").unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();

    // Both text preview and notes should be present
    assert!(resolved[0].text_preview.is_some());
    assert_eq!(
        resolved[0].text_preview.as_deref(),
        Some("Welcome to the interview")
    );
    assert_eq!(resolved[0].notes.len(), 1);
    assert_eq!(resolved[0].notes[0].text, "Agent feedback: great intro");
}

#[test]
fn notes_in_resolved_json_output() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_note("shot-001", "First note for agent").unwrap();
    doc.add_note("shot-001", "Second note for agent").unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    let json = serde_json::to_value(&resolved).unwrap();

    let shot = &json[0];
    assert_eq!(shot["id"], "shot-001");
    assert!(shot["text_preview"].is_string());
    assert_eq!(shot["notes"][0]["text"], "First note for agent");
    assert_eq!(shot["notes"][1]["text"], "Second note for agent");
    // Timestamps must be present
    assert!(shot["notes"][0]["created"].is_string());
    assert!(shot["notes"][1]["created"].is_string());
}

#[test]
fn notes_omitted_from_json_when_empty() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    let json = serde_json::to_value(&resolved).unwrap();

    // notes should be omitted (skip_serializing_if = "Vec::is_empty")
    assert!(json[0].get("notes").is_none());
}

#[test]
fn resolved_edit_with_notes_after_reorder() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 8 }).unwrap();
    doc.add_note("shot-001", "Opening note").unwrap();
    doc.add_note("shot-002", "Middle note").unwrap();

    // Reorder: move shot-002 to position 0
    doc.move_shot("shot-002", 0).unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 2);

    // After reorder, shot-002 is first
    assert_eq!(resolved[0].id, "shot-002");
    assert_eq!(resolved[0].notes.len(), 1);
    assert_eq!(resolved[0].notes[0].text, "Middle note");

    assert_eq!(resolved[1].id, "shot-001");
    assert_eq!(resolved[1].notes.len(), 1);
    assert_eq!(resolved[1].notes[0].text, "Opening note");
}

#[test]
fn resolved_edit_roundtrip_with_notes() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_note("shot-001", "Agent context note").unwrap();

    // Save, load, then resolve
    let path = tmp.path().join("edits/test.edit.json");
    doc.save(&path).unwrap();
    let loaded = EditDocument::load(&path).unwrap();

    let resolved = resolve_edit(&loaded, tmp.path()).unwrap();
    assert_eq!(resolved[0].notes.len(), 1);
    assert_eq!(resolved[0].notes[0].text, "Agent context note");
}
