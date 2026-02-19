//! TEST-037: Source indexing
//!
//! Verifies that `SourceIndex` can be constructed, serialized, loaded, saved,
//! and round-tripped correctly. Tests fixture loading, field integrity,
//! scene boundary correctness, and metadata consistency.

use ar_edit_core::index::{load_index, save_index};
use ar_edit_core::models::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_index(name: &str) -> SourceIndex {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/indexes")
        .join(name);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    serde_json::from_str(&data).unwrap()
}

fn make_index(source_id: &str, scenes: Vec<Scene>, duration_ms: u64) -> SourceIndex {
    let scene_count = scenes.len() as u32;
    SourceIndex {
        source_id: source_id.into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms,
            resolution: (1920, 1080),
            codec: "h264".into(),
            file_size_bytes: 52428800,
        },
        thumbnails: vec![],
        scene_count,
        scenes,
    }
}

fn make_scene(index: u32, start_ms: u64, end_ms: u64, description: Option<&str>) -> Scene {
    Scene {
        index,
        start_ms,
        end_ms,
        thumbnail: Default::default(),
        description: description.map(String::from),
    }
}

// -- Fixture loading ----------------------------------------------------------

#[test]
fn fixture_src_001_loads_correctly() {
    let idx = fixture_index("src-001.index.json");
    assert_eq!(idx.source_id, "src-001");
    assert_eq!(idx.metadata.duration_ms, 124500);
    assert_eq!(idx.metadata.resolution, (1920, 1080));
    assert_eq!(idx.metadata.codec, "h264");
    assert_eq!(idx.metadata.file_size_bytes, 52428800);
    assert_eq!(idx.scene_count, 4);
    assert_eq!(idx.scenes.len(), 4);
}

#[test]
fn fixture_src_002_loads_correctly() {
    let idx = fixture_index("src-002.index.json");
    assert_eq!(idx.source_id, "src-002");
    assert_eq!(idx.metadata.duration_ms, 90000);
    assert_eq!(idx.metadata.resolution, (1280, 720));
    assert_eq!(idx.metadata.codec, "h265");
    assert_eq!(idx.scene_count, 3);
    assert_eq!(idx.scenes.len(), 3);
}

#[test]
fn fixture_src_003_no_descriptions_loads() {
    let idx = fixture_index("src-003-no-descriptions.index.json");
    assert_eq!(idx.source_id, "src-003");
    assert_eq!(idx.metadata.resolution, (3840, 2160));
    assert_eq!(idx.scene_count, 2);
    for scene in &idx.scenes {
        assert_eq!(scene.description, None);
    }
}

// -- Scene boundary correctness -----------------------------------------------

#[test]
fn scenes_cover_full_duration_no_gaps() {
    let idx = fixture_index("src-001.index.json");

    // First scene starts at 0
    assert_eq!(idx.scenes[0].start_ms, 0);

    // Last scene ends at duration_ms
    let last = idx.scenes.last().unwrap();
    assert_eq!(last.end_ms, idx.metadata.duration_ms);

    // No gaps or overlaps: each scene start_ms equals the previous end_ms
    for window in idx.scenes.windows(2) {
        assert_eq!(
            window[0].end_ms, window[1].start_ms,
            "gap between scene {} and scene {}",
            window[0].index, window[1].index
        );
    }
}

#[test]
fn scene_indices_are_sequential() {
    let idx = fixture_index("src-001.index.json");
    for (i, scene) in idx.scenes.iter().enumerate() {
        assert_eq!(scene.index, i as u32);
    }
}

#[test]
fn scene_count_matches_scenes_vec() {
    let idx = fixture_index("src-001.index.json");
    assert_eq!(idx.scene_count, idx.scenes.len() as u32);
}

#[test]
fn every_scene_has_positive_duration() {
    let idx = fixture_index("src-001.index.json");
    for scene in &idx.scenes {
        assert!(
            scene.end_ms > scene.start_ms,
            "scene {} has zero or negative duration",
            scene.index
        );
    }
}

// -- Save / load round-trip ---------------------------------------------------

#[test]
fn save_and_load_roundtrip_with_fixture() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let original = fixture_index("src-001.index.json");
    save_index(tmp.path(), &original).unwrap();

    let loaded = load_index(tmp.path(), "src-001").unwrap();
    assert_eq!(loaded, original);
}

#[test]
fn save_and_load_roundtrip_programmatic() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let index = make_index(
        "src-010",
        vec![
            make_scene(0, 0, 15000, Some("Opening")),
            make_scene(1, 15000, 45000, None),
            make_scene(2, 45000, 60000, Some("Closing")),
        ],
        60000,
    );

    save_index(tmp.path(), &index).unwrap();
    let loaded = load_index(tmp.path(), "src-010").unwrap();
    assert_eq!(loaded, index);
}

// -- JSON serialization -------------------------------------------------------

#[test]
fn index_json_has_expected_structure() {
    let idx = fixture_index("src-001.index.json");
    let json = serde_json::to_value(&idx).unwrap();

    assert_eq!(json["source_id"], "src-001");
    assert!(json["indexed_at"].is_string());
    assert!(json["metadata"].is_object());
    assert!(json["thumbnails"].is_array());
    assert!(json["scenes"].is_array());
    assert_eq!(json["scene_count"], 4);
}

#[test]
fn index_serde_roundtrip() {
    let idx = fixture_index("src-001.index.json");
    let json = serde_json::to_string(&idx).unwrap();
    let back: SourceIndex = serde_json::from_str(&json).unwrap();
    assert_eq!(back, idx);
}

// -- Error cases --------------------------------------------------------------

#[test]
fn load_nonexistent_index_returns_not_indexed() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let err = load_index(tmp.path(), "src-999").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("not indexed"), "unexpected error: {msg}");
}

// -- Metadata fields ----------------------------------------------------------

#[test]
fn metadata_fields_are_correct() {
    let idx = fixture_index("src-002.index.json");
    assert_eq!(idx.metadata.duration_ms, 90000);
    assert_eq!(idx.metadata.resolution, (1280, 720));
    assert_eq!(idx.metadata.codec, "h265");
    assert_eq!(idx.metadata.file_size_bytes, 31457280);
}

// -- Multiple indexes can coexist on disk -------------------------------------

#[test]
fn multiple_indexes_saved_independently() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let idx1 = fixture_index("src-001.index.json");
    let idx2 = fixture_index("src-002.index.json");

    save_index(tmp.path(), &idx1).unwrap();
    save_index(tmp.path(), &idx2).unwrap();

    let loaded1 = load_index(tmp.path(), "src-001").unwrap();
    let loaded2 = load_index(tmp.path(), "src-002").unwrap();

    assert_eq!(loaded1, idx1);
    assert_eq!(loaded2, idx2);
}
