//! TEST-045: Transcript highlighting
//!
//! Verifies that the transcript data pipeline produces the correct information
//! for highlighting selected word/scene ranges in the transcript panel.
//! Tests cover: word-range highlighting across segments, scene-range highlighting,
//! time-range display, and auto-scroll data.

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

fn make_long_transcript() -> Transcript {
    let mut words = Vec::new();
    let texts = [
        "Welcome",
        "to",
        "the",
        "interview",
        "today",
        "we",
        "are",
        "going",
        "to",
        "discuss",
        "the",
        "latest",
        "developments",
        "in",
        "climate",
        "policy",
        "across",
        "the",
        "globe",
        "and",
        "its",
        "impact",
        "on",
        "future",
        "generations",
    ];

    for (i, text) in texts.iter().enumerate() {
        words.push(Word {
            index: i as u32,
            text: text.to_string(),
            start_ms: (i as u64) * 400,
            end_ms: (i as u64) * 400 + 350,
            confidence: 0.95,
        });
    }

    let mid = words.len() / 2;
    let seg1_words = words[..mid].to_vec();
    let seg2_words = words[mid..].to_vec();

    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: (texts.len() as u64) * 400,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: seg1_words.last().unwrap().end_ms,
                text: seg1_words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                words: seg1_words,
            },
            TranscriptSegment {
                index: 1,
                start_ms: words[mid].start_ms,
                end_ms: seg2_words.last().unwrap().end_ms,
                text: seg2_words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                words: seg2_words,
            },
        ],
        word_count: texts.len() as u32,
    }
}

fn make_index() -> SourceIndex {
    SourceIndex {
        source_id: "src-002".into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms: 120000,
            resolution: (1920, 1080),
            codec: "h264".into(),
            file_size_bytes: 52428800,
        },
        thumbnails: vec![],
        scene_count: 4,
        scenes: vec![
            Scene {
                index: 0,
                start_ms: 0,
                end_ms: 30000,
                thumbnail: "t0.jpg".into(),
                description: Some("Opening wide shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 30000,
                end_ms: 60000,
                thumbnail: "t1.jpg".into(),
                description: None,
            },
            Scene {
                index: 2,
                start_ms: 60000,
                end_ms: 90000,
                thumbnail: "t2.jpg".into(),
                description: Some("Interview medium".into()),
            },
            Scene {
                index: 3,
                start_ms: 90000,
                end_ms: 120000,
                thumbnail: "t3.jpg".into(),
                description: Some("Closing shot".into()),
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
        serde_json::to_string(&make_long_transcript()).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("index/src-002.index.json"),
        serde_json::to_string(&make_index()).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests: Word-range highlighting
// ---------------------------------------------------------------------------

/// The text preview for a word range reflects the exact words in the range.
#[test]
fn word_range_preview_matches_selected_words() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    // Select words 2-4: "the", "interview", "today"
    doc.add_shot("src-001", ShotRange::Words { from: 2, to: 4 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let preview = resolved[0].text_preview.as_deref().unwrap();
    assert!(preview.contains("the"));
    assert!(preview.contains("interview"));
    assert!(preview.contains("today"));
}

/// A word range within a single segment produces correct timestamps.
#[test]
fn single_segment_word_range_timestamps() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    // Words 0-4 are in segment 0
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 0); // word 0 start
    assert_eq!(resolved[0].end_ms, 1950); // word 4 end = 4*400+350
}

/// A cross-segment word range has correct start/end spanning both segments.
#[test]
fn cross_segment_word_range_timestamps() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    // Words 10-14 span segment boundary (mid=12)
    doc.add_shot("src-001", ShotRange::Words { from: 10, to: 14 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 4000); // word 10: 10*400 = 4000
    assert_eq!(resolved[0].end_ms, 5950); // word 14: 14*400+350 = 5950
    assert!(resolved[0].start_ms < resolved[0].end_ms);
    assert!(resolved[0].duration_ms > 0);
}

/// A single-word range produces the exact word's timestamps.
#[test]
fn single_word_range() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 5, to: 6 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    // Word 5: start=5*400=2000; Word 6: end=6*400+350=2750
    assert_eq!(resolved[0].start_ms, 2000);
    assert_eq!(resolved[0].end_ms, 2750);
    let preview = resolved[0].text_preview.as_deref().unwrap();
    assert_eq!(preview, "we are");
}

// ---------------------------------------------------------------------------
// Tests: Scene-range highlighting
// ---------------------------------------------------------------------------

/// A scene range includes all scenes in the range.
#[test]
fn scene_range_includes_all_scenes() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 0); // scene 0 start
    assert_eq!(resolved[0].end_ms, 90000); // scene 2 end
    let preview = resolved[0].scene_preview.as_deref().unwrap();
    assert!(preview.contains("Opening wide shot"));
    assert!(preview.contains("Interview medium"));
}

/// Scene range with no descriptions still resolves timestamps.
#[test]
fn scene_range_without_descriptions() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    // Scenes 1 (no description) and 2 (has description "Interview medium")
    doc.add_shot("src-002", ShotRange::Scenes { from: 1, to: 2 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 30000);
    assert_eq!(resolved[0].end_ms, 90000);
    // Preview shows only described scenes
    let preview = resolved[0].scene_preview.as_deref().unwrap();
    assert!(preview.contains("Interview medium"));
}

/// A single-scene range has correct timestamps.
#[test]
fn single_scene_range_timestamps() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 2, to: 3 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 60000);
    assert_eq!(resolved[0].end_ms, 120000);
    assert_eq!(resolved[0].duration_ms, 60000);
}

// ---------------------------------------------------------------------------
// Tests: Time-range display
// ---------------------------------------------------------------------------

/// Time ranges display correctly with start, end, and duration.
#[test]
fn time_range_display_fields() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot(
        "src-001",
        ShotRange::Time {
            from_ms: 15000,
            to_ms: 45000,
        },
    )
    .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].start_ms, 15000);
    assert_eq!(resolved[0].end_ms, 45000);
    assert_eq!(resolved[0].duration_ms, 30000);
    assert!(resolved[0].text_preview.is_none());
    assert!(resolved[0].scene_preview.is_none());
}

// ---------------------------------------------------------------------------
// Tests: Switching between shots (auto-scroll data)
// ---------------------------------------------------------------------------

/// Each shot's source field determines which transcript to display.
#[test]
fn different_sources_load_different_transcripts() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    // First shot: words => text preview from transcript
    assert!(resolved[0].text_preview.is_some());
    assert!(resolved[0].scene_preview.is_none());
    // Second shot: scenes => scene preview from index
    assert!(resolved[1].text_preview.is_none());
    assert!(resolved[1].scene_preview.is_some());
}

/// Navigating between shots with the same source but different ranges should
/// produce different previews.
#[test]
fn same_source_different_ranges() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 4 })
        .unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 14, to: 18 })
        .unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let preview1 = resolved[0].text_preview.as_deref().unwrap();
    let preview2 = resolved[1].text_preview.as_deref().unwrap();
    assert_ne!(preview1, preview2);
    assert!(preview1.contains("Welcome"));
    assert!(preview2.contains("climate"));
}

/// Shots with notes should include note data for transcript panel display.
#[test]
fn shot_notes_available_for_transcript_panel() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap();
    doc.add_note("shot-001", "Highlight this section").unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved[0].notes.len(), 1);
    assert_eq!(resolved[0].notes[0].text, "Highlight this section");
}
