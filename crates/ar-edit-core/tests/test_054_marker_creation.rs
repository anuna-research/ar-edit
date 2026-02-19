//! TEST-054: Marker creation
//!
//! Verifies that markers can be created on a source with correct ID assignment,
//! range types, labels, notes, timestamps, and file persistence.

use ar_edit_core::marker::{add_marker, list_markers};
use ar_edit_core::models::ShotRange;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_project() -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("annotations")).unwrap();
    tmp
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn create_marker_with_words_range() {
    let tmp = setup_project();
    let marker = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 45, to: 120 },
        "select",
        Some("Best take"),
    )
    .unwrap();

    assert_eq!(marker.id, "mark-001");
    assert_eq!(marker.range, ShotRange::Words { from: 45, to: 120 });
    assert_eq!(marker.label, "select");
    assert_eq!(marker.note.as_deref(), Some("Best take"));
}

#[test]
fn create_marker_with_scenes_range() {
    let tmp = setup_project();
    let marker = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Scenes { from: 1, to: 3 },
        "hero",
        None,
    )
    .unwrap();

    assert_eq!(marker.id, "mark-001");
    assert_eq!(marker.range, ShotRange::Scenes { from: 1, to: 3 });
    assert_eq!(marker.label, "hero");
    assert_eq!(marker.note, None);
}

#[test]
fn create_marker_with_time_range() {
    let tmp = setup_project();
    let marker = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Time { from_ms: 5000, to_ms: 12000 },
        "avoid",
        Some("Bad audio"),
    )
    .unwrap();

    assert_eq!(marker.id, "mark-001");
    assert_eq!(marker.range, ShotRange::Time { from_ms: 5000, to_ms: 12000 });
    assert_eq!(marker.label, "avoid");
    assert_eq!(marker.note.as_deref(), Some("Bad audio"));
}

#[test]
fn sequential_ids_across_multiple_markers() {
    let tmp = setup_project();

    let m1 = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 10 },
        "select",
        None,
    )
    .unwrap();

    let m2 = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Time { from_ms: 5000, to_ms: 8000 },
        "avoid",
        Some("Background noise"),
    )
    .unwrap();

    let m3 = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Scenes { from: 0, to: 1 },
        "hero",
        None,
    )
    .unwrap();

    assert_eq!(m1.id, "mark-001");
    assert_eq!(m2.id, "mark-002");
    assert_eq!(m3.id, "mark-003");
}

#[test]
fn marker_persisted_to_disk() {
    let tmp = setup_project();

    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 50 },
        "select",
        Some("Great intro"),
    )
    .unwrap();

    // Verify file exists and can be read back
    let loaded = list_markers(tmp.path(), "src-001").unwrap();
    assert_eq!(loaded.source_id, "src-001");
    assert_eq!(loaded.markers.len(), 1);
    assert_eq!(loaded.markers[0].id, "mark-001");
    assert_eq!(loaded.markers[0].label, "select");
    assert_eq!(loaded.markers[0].note.as_deref(), Some("Great intro"));
    assert_eq!(loaded.markers[0].range, ShotRange::Words { from: 0, to: 50 });
}

#[test]
fn marker_json_structure() {
    let tmp = setup_project();

    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 10, to: 20 },
        "select",
        Some("Key point"),
    )
    .unwrap();

    let path = tmp.path().join("annotations/src-001.markers.json");
    let content = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(json["source_id"], "src-001");
    assert_eq!(json["markers"][0]["id"], "mark-001");
    assert_eq!(json["markers"][0]["label"], "select");
    assert_eq!(json["markers"][0]["note"], "Key point");
    assert_eq!(
        json["markers"][0]["range"],
        serde_json::json!({ "words": { "from": 10, "to": 20 } })
    );
    // created timestamp must be present
    assert!(json["markers"][0]["created"].is_string());
}

#[test]
fn marker_timestamp_is_set() {
    let tmp = setup_project();
    let before = chrono::Utc::now();

    let marker = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 5 },
        "select",
        None,
    )
    .unwrap();

    let after = chrono::Utc::now();
    assert!(marker.created >= before);
    assert!(marker.created <= after);
}

#[test]
fn freeform_label_values() {
    let tmp = setup_project();

    let m1 = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 5 },
        "great-energy",
        None,
    )
    .unwrap();

    let m2 = add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 6, to: 10 },
        "needs-color-grade",
        Some("Slightly underexposed"),
    )
    .unwrap();

    assert_eq!(m1.label, "great-energy");
    assert_eq!(m2.label, "needs-color-grade");
}

#[test]
fn per_source_id_isolation() {
    let tmp = setup_project();

    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 10 },
        "select",
        None,
    )
    .unwrap();

    let m2 = add_marker(
        tmp.path(),
        "src-002",
        ShotRange::Words { from: 0, to: 20 },
        "hero",
        None,
    )
    .unwrap();

    // Each source starts its own ID sequence at mark-001
    assert_eq!(m2.id, "mark-001");

    let src1 = list_markers(tmp.path(), "src-001").unwrap();
    let src2 = list_markers(tmp.path(), "src-002").unwrap();
    assert_eq!(src1.markers.len(), 1);
    assert_eq!(src2.markers.len(), 1);
}
