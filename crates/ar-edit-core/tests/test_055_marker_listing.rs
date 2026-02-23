//! TEST-055: Marker listing with filters
//!
//! Verifies that markers can be listed per source, across all sources,
//! and filtered by label. Also verifies that empty sources and missing
//! files are handled correctly.

use ar_edit_core::marker::{add_marker, list_all_markers, list_markers};
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

fn seed_markers(tmp: &TempDir) {
    // src-001: 2 markers (select + avoid)
    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 50 },
        "select",
        Some("Good opening"),
    )
    .unwrap();
    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Time {
            from_ms: 62000,
            to_ms: 68000,
        },
        "avoid",
        Some("Bad audio"),
    )
    .unwrap();

    // src-002: 1 marker (hero)
    add_marker(
        tmp.path(),
        "src-002",
        ShotRange::Scenes { from: 0, to: 2 },
        "hero",
        None,
    )
    .unwrap();

    // src-003: 2 markers (select + select)
    add_marker(
        tmp.path(),
        "src-003",
        ShotRange::Words { from: 10, to: 30 },
        "select",
        None,
    )
    .unwrap();
    add_marker(
        tmp.path(),
        "src-003",
        ShotRange::Words { from: 40, to: 60 },
        "select",
        Some("Great energy"),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn list_markers_for_single_source() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let src1 = list_markers(tmp.path(), "src-001").unwrap();
    assert_eq!(src1.source_id, "src-001");
    assert_eq!(src1.markers.len(), 2);
    assert_eq!(src1.markers[0].id, "mark-001");
    assert_eq!(src1.markers[0].label, "select");
    assert_eq!(src1.markers[1].id, "mark-002");
    assert_eq!(src1.markers[1].label, "avoid");
}

#[test]
fn list_markers_for_empty_source() {
    let tmp = setup_project();

    let loaded = list_markers(tmp.path(), "src-999").unwrap();
    assert_eq!(loaded.source_id, "src-999");
    assert!(loaded.markers.is_empty());
}

#[test]
fn list_all_markers_across_sources() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let all = list_all_markers(tmp.path()).unwrap();
    assert_eq!(all.len(), 3); // src-001, src-002, src-003

    // Sorted by filename
    assert_eq!(all[0].source_id, "src-001");
    assert_eq!(all[0].markers.len(), 2);
    assert_eq!(all[1].source_id, "src-002");
    assert_eq!(all[1].markers.len(), 1);
    assert_eq!(all[2].source_id, "src-003");
    assert_eq!(all[2].markers.len(), 2);
}

#[test]
fn list_all_markers_empty_project() {
    let tmp = setup_project();
    let all = list_all_markers(tmp.path()).unwrap();
    assert!(all.is_empty());
}

#[test]
fn list_all_markers_no_annotations_dir() {
    let tmp = TempDir::new().unwrap();
    // No annotations/ directory at all
    let all = list_all_markers(tmp.path()).unwrap();
    assert!(all.is_empty());
}

#[test]
fn filter_markers_by_label_select() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let src1 = list_markers(tmp.path(), "src-001").unwrap();
    let selected: Vec<_> = src1
        .markers
        .iter()
        .filter(|m| m.label == "select")
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "mark-001");
    assert_eq!(selected[0].range, ShotRange::Words { from: 0, to: 50 });
}

#[test]
fn filter_markers_by_label_avoid() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let src1 = list_markers(tmp.path(), "src-001").unwrap();
    let avoided: Vec<_> = src1.markers.iter().filter(|m| m.label == "avoid").collect();
    assert_eq!(avoided.len(), 1);
    assert_eq!(avoided[0].note.as_deref(), Some("Bad audio"));
}

#[test]
fn filter_all_sources_by_label() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let all = list_all_markers(tmp.path()).unwrap();
    let all_select: Vec<_> = all
        .iter()
        .flat_map(|sm| sm.markers.iter())
        .filter(|m| m.label == "select")
        .collect();

    // src-001 has 1 select, src-003 has 2 select = 3 total
    assert_eq!(all_select.len(), 3);
}

#[test]
fn filter_by_range_type() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let all = list_all_markers(tmp.path()).unwrap();
    let time_markers: Vec<_> = all
        .iter()
        .flat_map(|sm| sm.markers.iter())
        .filter(|m| matches!(m.range, ShotRange::Time { .. }))
        .collect();

    assert_eq!(time_markers.len(), 1);
    assert_eq!(time_markers[0].label, "avoid");
}

#[test]
fn filter_by_source_and_label() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let src3 = list_markers(tmp.path(), "src-003").unwrap();
    let selected: Vec<_> = src3
        .markers
        .iter()
        .filter(|m| m.label == "select")
        .collect();

    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].id, "mark-001");
    assert_eq!(selected[1].id, "mark-002");
    assert_eq!(selected[1].note.as_deref(), Some("Great energy"));
}

#[test]
fn markers_with_notes_only() {
    let tmp = setup_project();
    seed_markers(&tmp);

    let all = list_all_markers(tmp.path()).unwrap();
    let with_notes: Vec<_> = all
        .iter()
        .flat_map(|sm| sm.markers.iter())
        .filter(|m| m.note.is_some())
        .collect();

    assert_eq!(with_notes.len(), 3); // "Good opening", "Bad audio", "Great energy"
}
