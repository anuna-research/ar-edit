//! TEST-044: Timeline panel
//!
//! Verifies that the display resolution pipeline produces correct data for
//! timeline rendering: shot IDs, source IDs, durations, range tags, and
//! text/scene previews for all three range types.

use std::fs;
use std::path::Path;

use ar_edit_core::display;
use ar_edit_core::models::{
    Defaults, EditDocument, Manifest, Scene, ShotRange, Source, SourceIndex, SourceMetadata,
    Transcript, TranscriptSegment, Word,
};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_source(id: &str, transcribed: bool, indexed: bool) -> Source {
    Source {
        id: id.into(),
        path: format!("sources/{id}.mp4").into(),
        original_filename: format!("{id}.mp4"),
        duration_ms: 124500,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (1920, 1080),
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed,
        indexed,
    }
}

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 12400,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 5230,
                text: "Welcome to the interview today".into(),
                words: vec![
                    Word { index: 0, text: "Welcome".into(), start_ms: 0, end_ms: 420, confidence: 0.95 },
                    Word { index: 1, text: "to".into(), start_ms: 420, end_ms: 540, confidence: 0.97 },
                    Word { index: 2, text: "the".into(), start_ms: 540, end_ms: 650, confidence: 0.98 },
                    Word { index: 3, text: "interview".into(), start_ms: 650, end_ms: 1200, confidence: 0.96 },
                    Word { index: 4, text: "today".into(), start_ms: 1200, end_ms: 1800, confidence: 0.94 },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 5230,
                end_ms: 12400,
                text: "We discuss climate policy changes".into(),
                words: vec![
                    Word { index: 5, text: "We".into(), start_ms: 5230, end_ms: 5500, confidence: 0.99 },
                    Word { index: 6, text: "discuss".into(), start_ms: 5500, end_ms: 6000, confidence: 0.93 },
                    Word { index: 7, text: "climate".into(), start_ms: 6000, end_ms: 6500, confidence: 0.91 },
                    Word { index: 8, text: "policy".into(), start_ms: 6500, end_ms: 7000, confidence: 0.90 },
                    Word { index: 9, text: "changes".into(), start_ms: 7000, end_ms: 7800, confidence: 0.88 },
                ],
            },
        ],
        word_count: 10,
    }
}

fn make_index() -> SourceIndex {
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
                description: Some("Wide establishing shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 18000,
                end_ms: 45000,
                thumbnail: "thumbnails/src-002_00m18s.jpg".into(),
                description: Some("Medium shot, speaker".into()),
            },
            Scene {
                index: 2,
                start_ms: 45000,
                end_ms: 90000,
                thumbnail: "thumbnails/src-002_00m45s.jpg".into(),
                description: Some("Close-up reaction".into()),
            },
        ],
    }
}

fn setup_project(dir: &Path) {
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("edits")).unwrap();

    let manifest = Manifest {
        version: "1.0.0".into(),
        name: "test".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
        sources: vec![
            make_source("src-001", true, false),
            make_source("src-002", false, true),
        ],
        next_source_id: 3,
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

    fs::write(
        dir.join("index/src-002.index.json"),
        serde_json::to_string(&make_index()).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests: Shot fields
// ---------------------------------------------------------------------------

/// Each resolved shot must have a non-empty ID matching the shot-NNN pattern.
#[test]
fn resolved_shots_have_valid_ids() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 1000, to_ms: 5000 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].id, "shot-001");
    assert_eq!(resolved[1].id, "shot-002");
    assert_eq!(resolved[2].id, "shot-003");
}

/// Each resolved shot must reference its correct source.
#[test]
fn resolved_shots_have_correct_sources() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].source, "src-001");
    assert_eq!(resolved[1].source, "src-002");
}

/// Word-range shots produce duration from first word's start_ms to last word's end_ms.
#[test]
fn word_range_shot_has_correct_duration() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    // words 0..4: start=0 (word 0), end=1800 (word 4)
    assert_eq!(resolved[0].start_ms, 0);
    assert_eq!(resolved[0].end_ms, 1800);
    assert_eq!(resolved[0].duration_ms, 1800);
}

/// Scene-range shots produce duration spanning from first scene start to last scene end.
#[test]
fn scene_range_shot_has_correct_duration() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    // scenes 0..2: start=0, end=90000
    assert_eq!(resolved[0].start_ms, 0);
    assert_eq!(resolved[0].end_ms, 90000);
    assert_eq!(resolved[0].duration_ms, 90000);
}

/// Time-range shots pass through timestamps directly.
#[test]
fn time_range_shot_passthrough() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Time { from_ms: 3500, to_ms: 9200 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 3500);
    assert_eq!(resolved[0].end_ms, 9200);
    assert_eq!(resolved[0].duration_ms, 5700);
}

// ---------------------------------------------------------------------------
// Tests: Text/scene previews
// ---------------------------------------------------------------------------

/// Word-range shots include a text preview from the transcript.
#[test]
fn word_shot_has_text_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let preview = resolved[0].text_preview.as_deref().unwrap();
    assert_eq!(preview, "Welcome to the interview");
    assert!(resolved[0].scene_preview.is_none());
}

/// Scene-range shots include a scene description preview.
#[test]
fn scene_shot_has_scene_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let preview = resolved[0].scene_preview.as_deref().unwrap();
    assert!(preview.contains("Wide establishing shot"));
    assert!(preview.contains("Medium shot, speaker"));
    assert!(resolved[0].text_preview.is_none());
}

/// Time-range shots have no preview (neither text nor scene).
#[test]
fn time_shot_has_no_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 5000 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert!(resolved[0].text_preview.is_none());
    assert!(resolved[0].scene_preview.is_none());
}

// ---------------------------------------------------------------------------
// Tests: format_time (used for duration display in timeline)
// ---------------------------------------------------------------------------

#[test]
fn format_time_zero() {
    assert_eq!(display::format_time(0), "00:00.000");
}

#[test]
fn format_time_subsecond() {
    assert_eq!(display::format_time(500), "00:00.500");
}

#[test]
fn format_time_seconds() {
    assert_eq!(display::format_time(5000), "00:05.000");
}

#[test]
fn format_time_minutes_and_seconds() {
    assert_eq!(display::format_time(90000), "01:30.000");
}

#[test]
fn format_time_with_milliseconds() {
    assert_eq!(display::format_time(1234), "00:01.234");
}

// ---------------------------------------------------------------------------
// Tests: Range preservation
// ---------------------------------------------------------------------------

/// The original ShotRange is preserved in resolved shots for display.
#[test]
fn resolved_shots_preserve_range_type() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 1, to: 2 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 100, to_ms: 200 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert!(matches!(resolved[0].range, ShotRange::Words { from: 0, to: 3 }));
    assert!(matches!(resolved[1].range, ShotRange::Scenes { from: 1, to: 2 }));
    assert!(matches!(resolved[2].range, ShotRange::Time { from_ms: 100, to_ms: 200 }));
}

// ---------------------------------------------------------------------------
// Tests: Multi-shot timeline ordering
// ---------------------------------------------------------------------------

/// Resolved shots maintain the same order as the edit snapshot.
#[test]
fn resolved_shots_maintain_order() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 5, to: 9 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 1000 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 4);
    assert_eq!(resolved[0].id, "shot-001");
    assert_eq!(resolved[1].id, "shot-002");
    assert_eq!(resolved[2].id, "shot-003");
    assert_eq!(resolved[3].id, "shot-004");
}

/// After a move operation, resolved shots reflect the new order.
#[test]
fn resolved_shots_reflect_move() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 5, to: 9 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 1000 }).unwrap();

    // Move shot-003 to position 0
    doc.move_shot("shot-003", 0).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].id, "shot-003");
    assert_eq!(resolved[1].id, "shot-001");
    assert_eq!(resolved[2].id, "shot-002");
}

/// Notes are included in resolved shots.
#[test]
fn resolved_shots_include_notes() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_note("shot-001", "Great take").unwrap();
    doc.add_note("shot-001", "Use this").unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].notes.len(), 2);
    assert_eq!(resolved[0].notes[0].text, "Great take");
    assert_eq!(resolved[0].notes[1].text, "Use this");
}

/// Cross-segment word ranges still produce a correct preview.
#[test]
fn cross_segment_word_range_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    // Words 3-7 span across segments
    doc.add_shot("src-001", ShotRange::Words { from: 3, to: 7 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let preview = resolved[0].text_preview.as_deref().unwrap();
    assert!(preview.contains("interview"));
    assert!(preview.contains("climate"));
}
