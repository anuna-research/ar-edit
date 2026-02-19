//! TEST-016: Display resolves ranges
//!
//! Verifies that `display::resolve_edit()` resolves shots against on-disk
//! transcripts and indexes, producing `ResolvedShot`s with correct timestamps,
//! text/scene previews, and preserved notes.

use ar_edit_core::display::{format_time, resolve_edit};
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
        duration_ms: 124500,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 5230,
                text: "Welcome to the interview".into(),
                words: vec![
                    Word { index: 0, text: "Welcome".into(), start_ms: 0, end_ms: 420, confidence: 0.95 },
                    Word { index: 1, text: "to".into(), start_ms: 420, end_ms: 540, confidence: 0.97 },
                    Word { index: 2, text: "the".into(), start_ms: 540, end_ms: 650, confidence: 0.98 },
                    Word { index: 3, text: "interview".into(), start_ms: 650, end_ms: 1200, confidence: 0.96 },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 5230,
                end_ms: 12400,
                text: "Today we discuss climate".into(),
                words: vec![
                    Word { index: 4, text: "Today".into(), start_ms: 5230, end_ms: 5600, confidence: 0.94 },
                    Word { index: 5, text: "we".into(), start_ms: 5600, end_ms: 5750, confidence: 0.99 },
                    Word { index: 6, text: "discuss".into(), start_ms: 5750, end_ms: 6200, confidence: 0.93 },
                    Word { index: 7, text: "climate".into(), start_ms: 6200, end_ms: 6800, confidence: 0.91 },
                ],
            },
        ],
        word_count: 8,
    }
}

fn make_source_index() -> SourceIndex {
    SourceIndex {
        source_id: "src-002".into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms: 90000,
            resolution: (1920, 1080),
            codec: "h264".into(),
            file_size_bytes: 52428800,
        },
        thumbnails: vec![],
        scene_count: 3,
        scenes: vec![
            Scene {
                index: 0,
                start_ms: 0,
                end_ms: 18000,
                thumbnail: "thumbnails/src-002_00m00s.jpg".into(),
                description: Some("Interior office, wide shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 18000,
                end_ms: 45000,
                thumbnail: "thumbnails/src-002_00m18s.jpg".into(),
                description: None,
            },
            Scene {
                index: 2,
                start_ms: 45000,
                end_ms: 90000,
                thumbnail: "thumbnails/src-002_00m45s.jpg".into(),
                description: Some("Close-up interview".into()),
            },
        ],
    }
}

fn setup_project(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();

    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();

    let index = make_source_index();
    std::fs::write(
        dir.join("index/src-002.index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();
}

// -- Words resolution ---------------------------------------------------------

#[test]
fn resolve_words_shot_produces_timestamps() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].start_ms, 0);
    assert_eq!(resolved[0].end_ms, 1200);
    assert_eq!(resolved[0].duration_ms, 1200);
}

#[test]
fn resolve_words_shot_has_text_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(
        resolved[0].text_preview.as_deref(),
        Some("Welcome to the interview")
    );
    assert!(resolved[0].scene_preview.is_none());
}

#[test]
fn resolve_words_cross_segment() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 2, to: 6 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 540);
    assert_eq!(resolved[0].end_ms, 6200);
    assert_eq!(
        resolved[0].text_preview.as_deref(),
        Some("the interview Today we discuss")
    );
}

// -- Scenes resolution --------------------------------------------------------

#[test]
fn resolve_scenes_shot_produces_timestamps() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 0);
    assert_eq!(resolved[0].end_ms, 90000);
    assert_eq!(resolved[0].duration_ms, 90000);
}

#[test]
fn resolve_scenes_shot_has_scene_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(
        resolved[0].scene_preview.as_deref(),
        Some("Interior office, wide shot; Close-up interview")
    );
    assert!(resolved[0].text_preview.is_none());
}

#[test]
fn resolve_scenes_without_descriptions_shows_fallback() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 1, to: 1 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].scene_preview.as_deref(), Some("scenes 1..1"));
}

// -- Time resolution ----------------------------------------------------------

#[test]
fn resolve_time_shot_is_passthrough() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 10000 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 5000);
    assert_eq!(resolved[0].end_ms, 10000);
    assert_eq!(resolved[0].duration_ms, 5000);
    assert!(resolved[0].text_preview.is_none());
    assert!(resolved[0].scene_preview.is_none());
}

// -- Mixed shots from multiple sources ----------------------------------------

#[test]
fn resolve_mixed_shots_from_different_sources() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 });
    doc.add_shot("src-001", ShotRange::Time { from_ms: 1000, to_ms: 3000 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);
    assert!(resolved[0].text_preview.is_some());
    assert!(resolved[1].scene_preview.is_some());
    assert!(resolved[2].text_preview.is_none());
    assert!(resolved[2].scene_preview.is_none());
}

// -- Notes preserved ----------------------------------------------------------

#[test]
fn resolve_preserves_notes() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_note("shot-001", "Great take").unwrap();
    doc.add_note("shot-001", "Use this as opener").unwrap();

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].notes.len(), 2);
    assert_eq!(resolved[0].notes[0].text, "Great take");
    assert_eq!(resolved[0].notes[1].text, "Use this as opener");
}

// -- Empty document -----------------------------------------------------------

#[test]
fn resolve_empty_document() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let doc = EditDocument::create("test");
    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert!(resolved.is_empty());
}

// -- Resolved fields ----------------------------------------------------------

#[test]
fn resolved_shot_has_all_fields() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });

    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    let r = &resolved[0];
    assert_eq!(r.id, "shot-001");
    assert_eq!(r.source, "src-001");
    assert_eq!(r.range, ShotRange::Words { from: 4, to: 7 });
    assert_eq!(r.start_ms, 5230);
    assert_eq!(r.end_ms, 6800);
    assert_eq!(r.duration_ms, 1570);
}

// -- format_time --------------------------------------------------------------

#[test]
fn format_time_various_values() {
    assert_eq!(format_time(0), "00:00.000");
    assert_eq!(format_time(6800), "00:06.800");
    assert_eq!(format_time(90000), "01:30.000");
    assert_eq!(format_time(61500), "01:01.500");
}
