//! TEST-039: Scene description scaffolding
//!
//! Verifies that `set_scene_description()` correctly sets, overwrites, and
//! preserves scene descriptions with proper audit-trail semantics. Tests
//! persistence, boundary checks, and interaction with fixtures.

use ar_edit_core::index::{load_index, save_index, set_scene_description};
use ar_edit_core::models::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_index(name: &str) -> SourceIndex {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/indexes")
        .join(name);
    let data = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&data).unwrap()
}

fn setup_index(tmp: &TempDir, index: &SourceIndex) {
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();
    save_index(tmp.path(), index).unwrap();
}

// -- Set description on empty scene -------------------------------------------

#[test]
fn set_description_on_scene_without_one() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    // Scene 2 has no description in the fixture
    assert_eq!(idx.scenes[2].description, None);

    let (updated, old) = set_scene_description(
        tmp.path(),
        "src-001",
        2,
        "B-roll montage",
    )
    .unwrap();

    assert_eq!(old, None);
    assert_eq!(
        updated.scenes[2].description.as_deref(),
        Some("B-roll montage")
    );
}

// -- Overwrite existing description -------------------------------------------

#[test]
fn overwrite_existing_description_returns_old_value() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    // Scene 0 has "Interior office, wide shot"
    assert_eq!(
        idx.scenes[0].description.as_deref(),
        Some("Interior office, wide shot")
    );

    let (updated, old) = set_scene_description(
        tmp.path(),
        "src-001",
        0,
        "Lobby establishing shot",
    )
    .unwrap();

    assert_eq!(old.as_deref(), Some("Interior office, wide shot"));
    assert_eq!(
        updated.scenes[0].description.as_deref(),
        Some("Lobby establishing shot")
    );
}

// -- Multiple overwrites chain correctly --------------------------------------

#[test]
fn chained_overwrites_track_history() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-003-no-descriptions.index.json");
    setup_index(&tmp, &idx);

    // First set
    let (_, old1) = set_scene_description(tmp.path(), "src-003", 0, "Draft 1").unwrap();
    assert_eq!(old1, None);

    // Second set returns first value
    let (_, old2) = set_scene_description(tmp.path(), "src-003", 0, "Draft 2").unwrap();
    assert_eq!(old2.as_deref(), Some("Draft 1"));

    // Third set returns second value
    let (updated, old3) = set_scene_description(tmp.path(), "src-003", 0, "Final").unwrap();
    assert_eq!(old3.as_deref(), Some("Draft 2"));
    assert_eq!(updated.scenes[0].description.as_deref(), Some("Final"));
}

// -- Other scenes remain unchanged --------------------------------------------

#[test]
fn setting_one_scene_does_not_affect_others() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    let (updated, _) = set_scene_description(
        tmp.path(),
        "src-001",
        1,
        "Updated scene 1",
    )
    .unwrap();

    // Scene 0 unchanged
    assert_eq!(updated.scenes[0].description, idx.scenes[0].description);
    // Scene 2 unchanged
    assert_eq!(updated.scenes[2].description, idx.scenes[2].description);
    // Scene 3 unchanged
    assert_eq!(updated.scenes[3].description, idx.scenes[3].description);
    // Only scene 1 changed
    assert_eq!(
        updated.scenes[1].description.as_deref(),
        Some("Updated scene 1")
    );
}

// -- Persistence to disk ------------------------------------------------------

#[test]
fn description_persisted_to_disk() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-002.index.json");
    setup_index(&tmp, &idx);

    set_scene_description(tmp.path(), "src-002", 0, "Studio overhead").unwrap();

    // Reload from disk independently
    let reloaded = load_index(tmp.path(), "src-002").unwrap();
    assert_eq!(
        reloaded.scenes[0].description.as_deref(),
        Some("Studio overhead")
    );
}

// -- Out-of-range errors ------------------------------------------------------

#[test]
fn out_of_range_scene_index_returns_error() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    let err = set_scene_description(tmp.path(), "src-001", 10, "nope").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("out of range"), "unexpected error: {msg}");
}

#[test]
fn out_of_range_at_exact_boundary() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    // scene_count is 4, so index 4 is out of range
    let err = set_scene_description(tmp.path(), "src-001", 4, "nope").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("out of range"), "unexpected error: {msg}");
}

#[test]
fn last_valid_scene_index_succeeds() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    // scene_count is 4, so index 3 is valid
    let (updated, _) = set_scene_description(
        tmp.path(),
        "src-001",
        3,
        "Updated last scene",
    )
    .unwrap();
    assert_eq!(
        updated.scenes[3].description.as_deref(),
        Some("Updated last scene")
    );
}

// -- Not indexed error --------------------------------------------------------

#[test]
fn set_description_on_nonexistent_index_returns_error() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let err = set_scene_description(tmp.path(), "src-999", 0, "text").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("not indexed"), "unexpected error: {msg}");
}

// -- Metadata and non-description fields preserved ----------------------------

#[test]
fn metadata_preserved_after_description_update() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    let (updated, _) = set_scene_description(
        tmp.path(),
        "src-001",
        0,
        "New description",
    )
    .unwrap();

    assert_eq!(updated.metadata, idx.metadata);
    assert_eq!(updated.source_id, idx.source_id);
    assert_eq!(updated.scene_count, idx.scene_count);
    assert_eq!(updated.thumbnails.len(), idx.thumbnails.len());
}

// -- Scene structural fields preserved ----------------------------------------

#[test]
fn scene_timing_preserved_after_description_update() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-001.index.json");
    setup_index(&tmp, &idx);

    let (updated, _) = set_scene_description(
        tmp.path(),
        "src-001",
        1,
        "New desc",
    )
    .unwrap();

    // Timing and thumbnail unchanged
    assert_eq!(updated.scenes[1].start_ms, idx.scenes[1].start_ms);
    assert_eq!(updated.scenes[1].end_ms, idx.scenes[1].end_ms);
    assert_eq!(updated.scenes[1].thumbnail, idx.scenes[1].thumbnail);
    assert_eq!(updated.scenes[1].index, idx.scenes[1].index);
}

// -- Set all descriptions on bare fixture -------------------------------------

#[test]
fn set_descriptions_on_all_scenes_of_bare_fixture() {
    let tmp = TempDir::new().unwrap();
    let idx = fixture_index("src-003-no-descriptions.index.json");
    setup_index(&tmp, &idx);

    set_scene_description(tmp.path(), "src-003", 0, "Opening wide").unwrap();
    set_scene_description(tmp.path(), "src-003", 1, "Closing shot").unwrap();

    let reloaded = load_index(tmp.path(), "src-003").unwrap();
    assert_eq!(
        reloaded.scenes[0].description.as_deref(),
        Some("Opening wide")
    );
    assert_eq!(
        reloaded.scenes[1].description.as_deref(),
        Some("Closing shot")
    );
}
