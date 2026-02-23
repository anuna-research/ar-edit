//! TEST-046: Source list
//!
//! Verifies that source metadata is correctly available for the source list
//! panel: source IDs, filenames, duration, codecs, resolution, frame rate,
//! audio specs, and transcribed/indexed status indicators.

use std::fs;
use std::path::Path;

use ar_edit_core::display;
use ar_edit_core::models::{Defaults, Manifest, Source};
use ar_edit_core::project;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_manifest(sources: Vec<Source>) -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        name: "test-project".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
        sources,
        next_source_id: 4,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    }
}

fn make_source(
    id: &str,
    filename: &str,
    duration_ms: u64,
    resolution: (u32, u32),
    transcribed: bool,
    indexed: bool,
) -> Source {
    Source {
        id: id.into(),
        path: format!("sources/{id}.mp4").into(),
        original_filename: filename.into(),
        duration_ms,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution,
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed,
        indexed,
    }
}

fn setup_project(dir: &Path, manifest: &Manifest) {
    fs::create_dir_all(dir.join("sources")).unwrap();
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("edits")).unwrap();
    fs::create_dir_all(dir.join("thumbnails")).unwrap();
    fs::create_dir_all(dir.join("annotations")).unwrap();

    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests: Source list contents
// ---------------------------------------------------------------------------

/// Source list shows all registered sources.
#[test]
fn source_list_shows_all_sources() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source(
            "src-001",
            "interview-alice.mp4",
            124500,
            (1920, 1080),
            true,
            true,
        ),
        make_source(
            "src-002",
            "broll-city.mp4",
            45000,
            (3840, 2160),
            false,
            false,
        ),
        make_source("src-003", "voiceover.mp4", 30000, (1280, 720), true, false),
    ]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(loaded.sources.len(), 3);
    assert_eq!(loaded.sources[0].id, "src-001");
    assert_eq!(loaded.sources[1].id, "src-002");
    assert_eq!(loaded.sources[2].id, "src-003");
}

/// Source IDs are preserved from the manifest.
#[test]
fn source_ids_preserved() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source(
        "src-001",
        "file1.mp4",
        60000,
        (1920, 1080),
        false,
        false,
    )]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(loaded.sources[0].id, "src-001");
}

/// Original filenames are available for display.
#[test]
fn source_filenames_available() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source(
            "src-001",
            "interview-alice.mp4",
            60000,
            (1920, 1080),
            false,
            false,
        ),
        make_source(
            "src-002",
            "broll-city-drone.mp4",
            30000,
            (3840, 2160),
            false,
            false,
        ),
    ]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(loaded.sources[0].original_filename, "interview-alice.mp4");
    assert_eq!(loaded.sources[1].original_filename, "broll-city-drone.mp4");
}

/// Duration is displayed as formatted time.
#[test]
fn source_duration_formatted() {
    let source = make_source("src-001", "test.mp4", 124500, (1920, 1080), false, false);
    let formatted = display::format_time(source.duration_ms);
    assert_eq!(formatted, "02:04.500");
}

/// Duration for sub-second clip.
#[test]
fn source_duration_short() {
    let source = make_source("src-001", "test.mp4", 500, (1920, 1080), false, false);
    let formatted = display::format_time(source.duration_ms);
    assert_eq!(formatted, "00:00.500");
}

// ---------------------------------------------------------------------------
// Tests: Status indicators
// ---------------------------------------------------------------------------

/// A source with both transcribed and indexed flags shows both indicators.
#[test]
fn source_both_transcribed_and_indexed() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source(
        "src-001",
        "test.mp4",
        60000,
        (1920, 1080),
        true,
        true,
    )]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    let source = &loaded.sources[0];
    assert!(source.transcribed);
    assert!(source.indexed);
}

/// A source with only transcribed flag set.
#[test]
fn source_transcribed_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source(
        "src-001",
        "test.mp4",
        60000,
        (1920, 1080),
        true,
        false,
    )]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert!(loaded.sources[0].transcribed);
    assert!(!loaded.sources[0].indexed);
}

/// A source with only indexed flag set.
#[test]
fn source_indexed_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source(
        "src-001",
        "test.mp4",
        60000,
        (1920, 1080),
        false,
        true,
    )]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert!(!loaded.sources[0].transcribed);
    assert!(loaded.sources[0].indexed);
}

/// A source with neither flag set.
#[test]
fn source_neither_transcribed_nor_indexed() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source(
        "src-001",
        "test.mp4",
        60000,
        (1920, 1080),
        false,
        false,
    )]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert!(!loaded.sources[0].transcribed);
    assert!(!loaded.sources[0].indexed);
}

// ---------------------------------------------------------------------------
// Tests: Source metadata
// ---------------------------------------------------------------------------

/// Resolution is available for source detail view.
#[test]
fn source_resolution_available() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source(
        "src-001",
        "test.mp4",
        60000,
        (3840, 2160),
        false,
        false,
    )]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(loaded.sources[0].resolution, (3840, 2160));
}

/// Video and audio codecs are available.
#[test]
fn source_codecs_available() {
    let tmp = TempDir::new().unwrap();
    let source = make_source("src-001", "test.mp4", 60000, (1920, 1080), false, false);
    let manifest = make_manifest(vec![source]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(loaded.sources[0].video_codec, "h264");
    assert_eq!(loaded.sources[0].audio_codec, "aac");
}

/// Frame rate is available.
#[test]
fn source_frame_rate_available() {
    let tmp = TempDir::new().unwrap();
    let source = make_source("src-001", "test.mp4", 60000, (1920, 1080), false, false);
    let manifest = make_manifest(vec![source]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert!((loaded.sources[0].frame_rate - 29.97).abs() < 0.01);
}

/// Audio channels and sample rate are available.
#[test]
fn source_audio_specs_available() {
    let tmp = TempDir::new().unwrap();
    let source = make_source("src-001", "test.mp4", 60000, (1920, 1080), false, false);
    let manifest = make_manifest(vec![source]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert_eq!(loaded.sources[0].audio_channels, 2);
    assert_eq!(loaded.sources[0].audio_sample_rate, 48000);
}

/// Empty source list is handled correctly.
#[test]
fn empty_source_list() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![]);
    setup_project(tmp.path(), &manifest);

    let loaded = project::read_manifest(tmp.path()).unwrap();
    assert!(loaded.sources.is_empty());
}

/// Sources round-trip through JSON serialization.
#[test]
fn source_data_roundtrips() {
    let source = make_source("src-001", "test.mp4", 60000, (1920, 1080), true, true);
    let json = serde_json::to_value(&source).unwrap();
    let back: Source = serde_json::from_value(json).unwrap();
    assert_eq!(back.id, source.id);
    assert_eq!(back.original_filename, source.original_filename);
    assert_eq!(back.duration_ms, source.duration_ms);
    assert_eq!(back.resolution, source.resolution);
    assert_eq!(back.transcribed, source.transcribed);
    assert_eq!(back.indexed, source.indexed);
}
