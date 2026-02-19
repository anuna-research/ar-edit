//! TEST-001: Project init structure
//!
//! Verifies that `project::init()` creates the correct directory layout,
//! writes a valid manifest.json, and rejects duplicate initialisation.

use std::fs;
use std::path::Path;

use ar_edit_core::models::Manifest;
use ar_edit_core::project;
use tempfile::TempDir;

/// All subdirectories that must exist after init.
const EXPECTED_DIRS: &[&str] = &[
    "sources",
    "transcripts",
    "index",
    "thumbnails",
    "edits",
    "annotations",
];

#[test]
fn init_creates_all_subdirectories() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");

    project::init(&project_dir).unwrap();

    for dir in EXPECTED_DIRS {
        let path = project_dir.join(dir);
        assert!(path.is_dir(), "expected directory missing: {dir}");
    }
}

#[test]
fn init_creates_manifest_json() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");

    project::init(&project_dir).unwrap();

    let manifest_path = project_dir.join("manifest.json");
    assert!(manifest_path.is_file(), "manifest.json not created");

    let content = fs::read_to_string(&manifest_path).unwrap();
    let manifest: Manifest = serde_json::from_str(&content).unwrap();
    assert_eq!(manifest.version, "1.0.0");
}

#[test]
fn init_manifest_has_correct_defaults() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("defaults-project");

    let manifest = project::init(&project_dir).unwrap();

    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.name, "defaults-project");
    assert!(manifest.sources.is_empty());
    assert_eq!(manifest.next_source_id, 1);
    assert_eq!(manifest.defaults.whisper_model, "base");
    assert_eq!(manifest.defaults.thumbnail_interval_sec, 10);
    assert_eq!(manifest.defaults.render_codec, "h264");
    assert_eq!(manifest.defaults.render_container, "mp4");
}

#[test]
fn init_uses_directory_name_as_project_name() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("my-awesome-project");

    let manifest = project::init(&project_dir).unwrap();
    assert_eq!(manifest.name, "my-awesome-project");
}

#[test]
fn init_manifest_roundtrips_through_disk() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("roundtrip-project");

    let original = project::init(&project_dir).unwrap();
    let loaded = project::read_manifest(&project_dir).unwrap();

    assert_eq!(loaded.name, original.name);
    assert_eq!(loaded.version, original.version);
    assert_eq!(loaded.next_source_id, original.next_source_id);
    assert_eq!(loaded.defaults, original.defaults);
    assert_eq!(loaded.sources.len(), original.sources.len());
}

#[test]
fn init_rejects_existing_directory() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("existing");
    fs::create_dir(&project_dir).unwrap();

    let err = project::init(&project_dir).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("already exists"),
        "expected DirectoryExists error, got: {msg}"
    );
}

#[test]
fn init_no_extra_files_in_root() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("clean-project");

    project::init(&project_dir).unwrap();

    let entries: Vec<_> = fs::read_dir(&project_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    // 6 subdirectories + manifest.json = 7 entries
    assert_eq!(
        entries.len(),
        EXPECTED_DIRS.len() + 1,
        "unexpected files in project root"
    );
}

#[test]
fn init_subdirectories_are_empty() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("empty-subdirs");

    project::init(&project_dir).unwrap();

    for dir in EXPECTED_DIRS {
        let count = fs::read_dir(project_dir.join(dir)).unwrap().count();
        assert_eq!(count, 0, "subdirectory {dir} should be empty after init");
    }
}

/// Verify fixture files can be loaded as valid model types.
#[test]
fn fixture_transcript_is_valid() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixture = workspace_root.join("tests/fixtures/transcripts/src-001.transcript.json");
    let content = fs::read_to_string(&fixture).unwrap();
    let transcript: ar_edit_core::models::Transcript = serde_json::from_str(&content).unwrap();
    assert_eq!(transcript.source_id, "src-001");
    assert_eq!(transcript.word_count, 10);
    assert_eq!(transcript.segments.len(), 2);
}

#[test]
fn fixture_index_is_valid() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixture = workspace_root.join("tests/fixtures/indexes/src-001.index.json");
    let content = fs::read_to_string(&fixture).unwrap();
    let index: ar_edit_core::models::SourceIndex = serde_json::from_str(&content).unwrap();
    assert_eq!(index.source_id, "src-001");
    assert_eq!(index.scene_count, 2);
    assert_eq!(index.scenes.len(), 2);
}
