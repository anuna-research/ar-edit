//! TEST-041: Index summary
//!
//! Verifies that a SourceIndex can be summarized: scene count, total duration,
//! described vs undescribed scenes, thumbnail coverage, and metadata extraction.
//! Uses pre-generated index fixtures to validate summary computations.

use ar_edit_core::index::{load_index, save_index};
use ar_edit_core::models::*;
use std::path::PathBuf;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_index(name: &str) -> SourceIndex {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/indexes")
        .join(name);
    let data = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&data).unwrap()
}

/// Summary statistics from a SourceIndex.
#[allow(dead_code)]
struct IndexSummary {
    source_id: String,
    scene_count: u32,
    duration_ms: u64,
    thumbnail_count: usize,
    described_scenes: u32,
    undescribed_scenes: u32,
    resolution: (u32, u32),
    codec: String,
    file_size_bytes: u64,
    total_scene_coverage_ms: u64,
}

fn summarize(idx: &SourceIndex) -> IndexSummary {
    let described = idx
        .scenes
        .iter()
        .filter(|s| s.description.is_some())
        .count() as u32;

    let total_scene_coverage: u64 = idx
        .scenes
        .iter()
        .map(|s| s.end_ms.saturating_sub(s.start_ms))
        .sum();

    IndexSummary {
        source_id: idx.source_id.clone(),
        scene_count: idx.scene_count,
        duration_ms: idx.metadata.duration_ms,
        thumbnail_count: idx.thumbnails.len(),
        described_scenes: described,
        undescribed_scenes: idx.scene_count - described,
        resolution: idx.metadata.resolution,
        codec: idx.metadata.codec.clone(),
        file_size_bytes: idx.metadata.file_size_bytes,
        total_scene_coverage_ms: total_scene_coverage,
    }
}

// -- Basic summary from fixture -----------------------------------------------

#[test]
fn summary_src_001_scene_count() {
    let idx = fixture_index("src-001.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.scene_count, 4);
}

#[test]
fn summary_src_001_duration() {
    let idx = fixture_index("src-001.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.duration_ms, 124500);
}

#[test]
fn summary_src_001_thumbnail_count() {
    let idx = fixture_index("src-001.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.thumbnail_count, 16);
}

#[test]
fn summary_src_001_described_vs_undescribed() {
    let idx = fixture_index("src-001.index.json");
    let summary = summarize(&idx);
    // Scenes 0, 1, 3 have descriptions; scene 2 does not
    assert_eq!(summary.described_scenes, 3);
    assert_eq!(summary.undescribed_scenes, 1);
}

#[test]
fn summary_src_001_metadata() {
    let idx = fixture_index("src-001.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.resolution, (1920, 1080));
    assert_eq!(summary.codec, "h264");
    assert_eq!(summary.file_size_bytes, 52428800);
}

// -- Scene coverage sums to full duration -------------------------------------

#[test]
fn scene_coverage_equals_duration() {
    let idx = fixture_index("src-001.index.json");
    let summary = summarize(&idx);
    assert_eq!(
        summary.total_scene_coverage_ms, summary.duration_ms,
        "scene coverage should equal total duration"
    );
}

#[test]
fn scene_coverage_equals_duration_src_002() {
    let idx = fixture_index("src-002.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.total_scene_coverage_ms, summary.duration_ms);
}

// -- Second fixture summary ---------------------------------------------------

#[test]
fn summary_src_002_scene_count() {
    let idx = fixture_index("src-002.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.scene_count, 3);
}

#[test]
fn summary_src_002_all_described() {
    let idx = fixture_index("src-002.index.json");
    let summary = summarize(&idx);
    // All 3 scenes in src-002 have descriptions
    assert_eq!(summary.described_scenes, 3);
    assert_eq!(summary.undescribed_scenes, 0);
}

#[test]
fn summary_src_002_metadata() {
    let idx = fixture_index("src-002.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.resolution, (1280, 720));
    assert_eq!(summary.codec, "h265");
    assert_eq!(summary.duration_ms, 90000);
}

// -- No-description fixture ---------------------------------------------------

#[test]
fn summary_no_descriptions_all_undescribed() {
    let idx = fixture_index("src-003-no-descriptions.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.described_scenes, 0);
    assert_eq!(summary.undescribed_scenes, 2);
}

#[test]
fn summary_no_descriptions_metadata() {
    let idx = fixture_index("src-003-no-descriptions.index.json");
    let summary = summarize(&idx);
    assert_eq!(summary.resolution, (3840, 2160));
    assert_eq!(summary.duration_ms, 60000);
}

// -- Summary after description update -----------------------------------------

#[test]
fn summary_updates_after_adding_description() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let idx = fixture_index("src-003-no-descriptions.index.json");
    save_index(tmp.path(), &idx).unwrap();

    // Before: 0 described
    let summary_before = summarize(&idx);
    assert_eq!(summary_before.described_scenes, 0);

    // Add description to scene 0
    ar_edit_core::index::set_scene_description(tmp.path(), "src-003", 0, "Opening shot").unwrap();

    let updated = load_index(tmp.path(), "src-003").unwrap();
    let summary_after = summarize(&updated);
    assert_eq!(summary_after.described_scenes, 1);
    assert_eq!(summary_after.undescribed_scenes, 1);
}

// -- Average scene duration ---------------------------------------------------

#[test]
fn average_scene_duration() {
    let idx = fixture_index("src-001.index.json");
    let avg_ms = idx.metadata.duration_ms / idx.scene_count as u64;
    // 124500 / 4 = 31125
    assert_eq!(avg_ms, 31125);
}

// -- Longest and shortest scene -----------------------------------------------

#[test]
fn longest_scene_identified() {
    let idx = fixture_index("src-001.index.json");
    let longest = idx
        .scenes
        .iter()
        .max_by_key(|s| s.end_ms - s.start_ms)
        .unwrap();
    // Scene 2: 45000..87000 = 42000ms (the longest)
    assert_eq!(longest.index, 2);
    assert_eq!(longest.end_ms - longest.start_ms, 42000);
}

#[test]
fn shortest_scene_identified() {
    let idx = fixture_index("src-001.index.json");
    let shortest = idx
        .scenes
        .iter()
        .min_by_key(|s| s.end_ms - s.start_ms)
        .unwrap();
    // Scene 0: 0..18000 = 18000ms (the shortest)
    assert_eq!(shortest.index, 0);
    assert_eq!(shortest.end_ms - shortest.start_ms, 18000);
}

// -- JSON summary serialization -----------------------------------------------

#[test]
fn index_json_contains_all_summary_fields() {
    let idx = fixture_index("src-001.index.json");
    let json = serde_json::to_value(&idx).unwrap();

    assert!(json["source_id"].is_string());
    assert!(json["indexed_at"].is_string());
    assert!(json["metadata"]["duration_ms"].is_number());
    assert!(json["metadata"]["resolution"].is_array());
    assert!(json["metadata"]["codec"].is_string());
    assert!(json["metadata"]["file_size_bytes"].is_number());
    assert!(json["scene_count"].is_number());
    assert!(json["thumbnails"].is_array());
    assert!(json["scenes"].is_array());
}

// -- Multiple indexes loaded from disk ----------------------------------------

#[test]
fn load_and_summarize_multiple_indexes() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let idx1 = fixture_index("src-001.index.json");
    let idx2 = fixture_index("src-002.index.json");
    let idx3 = fixture_index("src-003-no-descriptions.index.json");

    save_index(tmp.path(), &idx1).unwrap();
    save_index(tmp.path(), &idx2).unwrap();
    save_index(tmp.path(), &idx3).unwrap();

    let loaded1 = load_index(tmp.path(), "src-001").unwrap();
    let loaded2 = load_index(tmp.path(), "src-002").unwrap();
    let loaded3 = load_index(tmp.path(), "src-003").unwrap();

    let s1 = summarize(&loaded1);
    let s2 = summarize(&loaded2);
    let s3 = summarize(&loaded3);

    // Total scenes across all sources
    let total_scenes = s1.scene_count + s2.scene_count + s3.scene_count;
    assert_eq!(total_scenes, 9); // 4 + 3 + 2

    // Total described
    let total_described = s1.described_scenes + s2.described_scenes + s3.described_scenes;
    assert_eq!(total_described, 6); // 3 + 3 + 0

    // Total duration
    let total_duration = s1.duration_ms + s2.duration_ms + s3.duration_ms;
    assert_eq!(total_duration, 274500); // 124500 + 90000 + 60000
}
