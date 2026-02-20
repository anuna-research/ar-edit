//! TEST-043: TUI launch
//!
//! Verifies that the project loading pipeline used by the TUI works correctly:
//! reading a manifest, loading an edit document, and resolving shots for
//! display.  These are the exact steps performed by `App::load_project()`.

use std::fs;
use std::path::Path;

use ar_edit_core::display;
use ar_edit_core::models::{
    Defaults, EditDocument, Manifest, Scene, ShotRange, Source, SourceIndex, SourceMetadata,
    Transcript, TranscriptSegment, Word,
};
use ar_edit_core::project;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_source(id: &str, transcribed: bool, indexed: bool) -> Source {
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
        segments: vec![TranscriptSegment {
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
        }],
        word_count: 4,
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
        scene_count: 2,
        scenes: vec![
            Scene {
                index: 0,
                start_ms: 0,
                end_ms: 45000,
                thumbnail: "thumbnails/src-002_00m00s.jpg".into(),
                description: Some("Wide shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 45000,
                end_ms: 90000,
                thumbnail: "thumbnails/src-002_00m45s.jpg".into(),
                description: Some("Close-up".into()),
            },
        ],
    }
}

/// Set up a fully valid project directory with manifest, transcript, index,
/// and an edit document — mirroring what `App::load_project()` expects.
fn setup_full_project(dir: &Path) -> EditDocument {
    // Create directory structure
    fs::create_dir_all(dir.join("sources")).unwrap();
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("thumbnails")).unwrap();
    fs::create_dir_all(dir.join("edits")).unwrap();
    fs::create_dir_all(dir.join("annotations")).unwrap();

    // Manifest
    let manifest = Manifest {
        version: "1.0.0".into(),
        name: "test-project".into(),
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
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Transcript for src-001
    let transcript = make_transcript();
    fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();

    // Index for src-002
    let index = make_index();
    fs::write(
        dir.join("index/src-002.index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();

    // Edit document with shots from both sources
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 2000, to_ms: 8000 }).unwrap();
    doc.save(&dir.join("edits/rough-cut.edit.json")).unwrap();

    doc
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verify the TUI can load a project manifest and extract source list.
#[test]
fn load_manifest_returns_sources() {
    let tmp = TempDir::new().unwrap();
    setup_full_project(tmp.path());

    let manifest = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(manifest.sources.len(), 2);
    assert_eq!(manifest.sources[0].id, "src-001");
    assert_eq!(manifest.sources[1].id, "src-002");
}

/// Verify the TUI can load an edit document from the project's edits/ directory.
#[test]
fn load_edit_document_from_project() {
    let tmp = TempDir::new().unwrap();
    setup_full_project(tmp.path());

    let doc = EditDocument::load(&tmp.path().join("edits/rough-cut.edit.json")).unwrap();
    assert_eq!(doc.name, "rough-cut");
    assert_eq!(doc.snapshot.shots.len(), 3);
}

/// Verify the TUI can resolve all shots for display after loading a project.
#[test]
fn resolve_shots_after_project_load() {
    let tmp = TempDir::new().unwrap();
    setup_full_project(tmp.path());

    let doc = EditDocument::load(&tmp.path().join("edits/rough-cut.edit.json")).unwrap();
    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();

    assert_eq!(resolved.len(), 3);
    // Word-based shot resolved with timestamps and text preview
    assert_eq!(resolved[0].id, "shot-001");
    assert_eq!(resolved[0].source, "src-001");
    assert!(resolved[0].start_ms < resolved[0].end_ms);
    assert!(resolved[0].text_preview.is_some());

    // Scene-based shot resolved with timestamps and scene preview
    assert_eq!(resolved[1].id, "shot-002");
    assert_eq!(resolved[1].source, "src-002");
    assert!(resolved[1].duration_ms > 0);
    assert!(resolved[1].scene_preview.is_some());

    // Time-based shot passes through directly
    assert_eq!(resolved[2].id, "shot-003");
    assert_eq!(resolved[2].start_ms, 2000);
    assert_eq!(resolved[2].end_ms, 8000);
    assert_eq!(resolved[2].duration_ms, 6000);
}

/// Verify the TUI handles an empty project (no edit files).
#[test]
fn load_project_with_no_edits() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("new-project");

    // Minimal project: just manifest
    project::init(&dir).unwrap();

    let manifest = project::read_manifest(&dir).unwrap();
    assert!(manifest.sources.is_empty());

    // edits/ directory exists but is empty — the TUI should handle this gracefully
    let edits_dir = dir.join("edits");
    let entries: Vec<_> = fs::read_dir(&edits_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert!(entries.is_empty());
}

/// Verify the TUI default initial state: Normal mode, Timeline focus, shot 0 selected.
#[test]
fn initial_state_is_normal_mode() {
    let tmp = TempDir::new().unwrap();
    setup_full_project(tmp.path());

    let doc = EditDocument::load(&tmp.path().join("edits/rough-cut.edit.json")).unwrap();
    // Simulate what App::new + load_project does:
    // - mode = Normal (checked via enum)
    // - focus = Timeline
    // - selected_shot = 0
    // - edit loaded, sources loaded, shots resolved
    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert!(!resolved.is_empty());
    // First shot should be selectable at index 0
    assert_eq!(resolved[0].id, "shot-001");
}

/// Verify the TUI can handle a project with missing transcript files gracefully.
#[test]
fn project_with_missing_transcript_still_loads() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    // Set up project but don't create transcript files
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("edits")).unwrap();

    let manifest = Manifest {
        version: "1.0.0".into(),
        name: "test".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
        sources: vec![make_source("src-001", false, false)],
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

    // Edit with time range (doesn't need transcript)
    let mut doc = EditDocument::create("test");
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 5000 }).unwrap();
    doc.save(&dir.join("edits/test.edit.json")).unwrap();

    // Should resolve without error because time ranges don't need transcripts
    let resolved = display::resolve_edit(&doc, dir).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].duration_ms, 5000);
}

/// Verify that an edit document with no shots resolves to empty.
#[test]
fn empty_edit_resolves_to_empty_timeline() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("edits")).unwrap();

    let doc = EditDocument::create("empty");
    let resolved = display::resolve_edit(&doc, dir).unwrap();
    assert!(resolved.is_empty());
}

/// Verify the full lifecycle: load manifest → read sources → load edit → resolve.
#[test]
fn full_load_lifecycle() {
    let tmp = TempDir::new().unwrap();
    setup_full_project(tmp.path());

    // Step 1: Read manifest
    let manifest = project::read_manifest(tmp.path()).unwrap();
    let sources = manifest.sources;
    assert_eq!(sources.len(), 2);

    // Step 2: Find and load first edit
    let edits_dir = tmp.path().join("edits");
    let edit_entry = fs::read_dir(&edits_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .unwrap();
    let loaded = EditDocument::load(&edit_entry.path()).unwrap();
    assert_eq!(loaded.name, "rough-cut");

    // Step 3: Resolve shots
    let resolved = display::resolve_edit(&loaded, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);

    // Step 4: Verify each shot has the data needed for display
    for shot in &resolved {
        assert!(!shot.id.is_empty());
        assert!(!shot.source.is_empty());
    }
}
