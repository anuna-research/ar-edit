//! TEST-038: Thumbnail extraction
//!
//! Verifies that thumbnail entries in a SourceIndex are correctly structured:
//! sorted by timestamp, properly named, deduplicated at scene boundaries,
//! and that scene thumbnails reference valid paths.

use ar_edit_core::models::*;
use std::collections::HashSet;
use std::path::PathBuf;

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

// -- Thumbnail ordering -------------------------------------------------------

#[test]
fn thumbnails_sorted_by_timestamp() {
    let idx = fixture_index("src-001.index.json");
    assert!(idx.thumbnails.len() > 1);
    for window in idx.thumbnails.windows(2) {
        assert!(
            window[0].timestamp_ms <= window[1].timestamp_ms,
            "thumbnails not sorted: {} > {}",
            window[0].timestamp_ms,
            window[1].timestamp_ms
        );
    }
}

#[test]
fn thumbnails_sorted_src_002() {
    let idx = fixture_index("src-002.index.json");
    for window in idx.thumbnails.windows(2) {
        assert!(window[0].timestamp_ms <= window[1].timestamp_ms);
    }
}

// -- No duplicate timestamps --------------------------------------------------

#[test]
fn no_duplicate_thumbnail_timestamps() {
    let idx = fixture_index("src-001.index.json");
    let timestamps: Vec<u64> = idx.thumbnails.iter().map(|t| t.timestamp_ms).collect();
    let unique: HashSet<u64> = timestamps.iter().copied().collect();
    assert_eq!(
        timestamps.len(),
        unique.len(),
        "duplicate thumbnail timestamps found"
    );
}

// -- Thumbnail paths follow naming convention ---------------------------------

#[test]
fn thumbnail_paths_follow_naming_convention() {
    let idx = fixture_index("src-001.index.json");
    for thumb in &idx.thumbnails {
        let path_str = thumb.path.to_string_lossy();
        assert!(
            path_str.starts_with("thumbnails/"),
            "thumbnail path should be under thumbnails/: {path_str}"
        );
        assert!(
            path_str.ends_with(".jpg"),
            "thumbnail should be JPEG: {path_str}"
        );
        assert!(
            path_str.contains("src-001"),
            "thumbnail should contain source id: {path_str}"
        );
    }
}

#[test]
fn thumbnail_filename_encodes_timestamp() {
    let idx = fixture_index("src-001.index.json");

    // Verify specific known thumbnails
    let t_00m00s = idx.thumbnails.iter().find(|t| t.timestamp_ms == 0).unwrap();
    assert!(t_00m00s.path.to_string_lossy().contains("00m00s"));

    let t_00m18s = idx.thumbnails.iter().find(|t| t.timestamp_ms == 18000).unwrap();
    assert!(t_00m18s.path.to_string_lossy().contains("00m18s"));

    let t_01m27s = idx.thumbnails.iter().find(|t| t.timestamp_ms == 87000).unwrap();
    assert!(t_01m27s.path.to_string_lossy().contains("01m27s"));
}

// -- Thumbnails start at 0 ---------------------------------------------------

#[test]
fn first_thumbnail_at_zero() {
    let idx = fixture_index("src-001.index.json");
    assert_eq!(idx.thumbnails[0].timestamp_ms, 0);
}

// -- All thumbnails within duration -------------------------------------------

#[test]
fn all_thumbnails_within_duration() {
    let idx = fixture_index("src-001.index.json");
    for thumb in &idx.thumbnails {
        assert!(
            thumb.timestamp_ms < idx.metadata.duration_ms,
            "thumbnail at {}ms exceeds duration {}ms",
            thumb.timestamp_ms,
            idx.metadata.duration_ms
        );
    }
}

// -- Scene boundaries appear in thumbnails ------------------------------------

#[test]
fn scene_start_timestamps_in_thumbnails() {
    let idx = fixture_index("src-001.index.json");
    let thumb_timestamps: HashSet<u64> = idx.thumbnails.iter().map(|t| t.timestamp_ms).collect();

    for scene in &idx.scenes {
        assert!(
            thumb_timestamps.contains(&scene.start_ms),
            "scene {} start_ms {} not found in thumbnail timestamps",
            scene.index,
            scene.start_ms
        );
    }
}

// -- Scene thumbnails reference valid paths -----------------------------------

#[test]
fn scene_thumbnails_are_in_thumbnails_list() {
    let idx = fixture_index("src-001.index.json");
    let thumb_paths: HashSet<PathBuf> = idx.thumbnails.iter().map(|t| t.path.clone()).collect();

    for scene in &idx.scenes {
        assert!(
            thumb_paths.contains(&scene.thumbnail),
            "scene {} thumbnail {} not found in thumbnails list",
            scene.index,
            scene.thumbnail.display()
        );
    }
}

// -- Interval thumbnails present (10s default) --------------------------------

#[test]
fn interval_thumbnails_at_10s_boundaries() {
    let idx = fixture_index("src-001.index.json");
    let thumb_timestamps: HashSet<u64> = idx.thumbnails.iter().map(|t| t.timestamp_ms).collect();

    // With 124500ms duration and 10s interval, we expect 0, 10000, 20000, ..., 120000
    let expected_intervals: Vec<u64> = (0..=12).map(|i| i * 10000).collect();
    for ts in &expected_intervals {
        assert!(
            thumb_timestamps.contains(ts),
            "expected interval thumbnail at {}ms",
            ts
        );
    }
}

// -- Thumbnail description is optional ----------------------------------------

#[test]
fn thumbnail_description_is_optional() {
    let idx = fixture_index("src-001.index.json");

    let with_desc = idx.thumbnails.iter().filter(|t| t.description.is_some()).count();
    let without_desc = idx.thumbnails.iter().filter(|t| t.description.is_none()).count();

    assert!(with_desc > 0, "fixture should have at least one thumbnail with description");
    assert!(without_desc > 0, "fixture should have at least one thumbnail without description");
}

// -- Thumbnail count combines scenes + intervals (merged, deduped) ------------

#[test]
fn thumbnail_count_matches_merged_timestamps() {
    let idx = fixture_index("src-001.index.json");

    // Manually compute expected merged timestamps (scenes + 10s intervals)
    let mut expected = std::collections::BTreeSet::new();
    for scene in &idx.scenes {
        expected.insert(scene.start_ms);
    }
    let mut t = 0u64;
    while t < idx.metadata.duration_ms {
        expected.insert(t);
        t += 10000;
    }

    assert_eq!(
        idx.thumbnails.len(),
        expected.len(),
        "thumbnail count should match merged scene + interval timestamps"
    );
}

// -- Second fixture has different interval behavior ---------------------------

#[test]
fn src_002_thumbnails_include_scene_boundaries() {
    let idx = fixture_index("src-002.index.json");
    let thumb_timestamps: HashSet<u64> = idx.thumbnails.iter().map(|t| t.timestamp_ms).collect();

    for scene in &idx.scenes {
        assert!(
            thumb_timestamps.contains(&scene.start_ms),
            "src-002 scene {} start {}ms missing from thumbnails",
            scene.index,
            scene.start_ms
        );
    }
}
