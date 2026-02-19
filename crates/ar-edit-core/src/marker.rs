use std::fs;
use std::path::Path;

use chrono::Utc;
use thiserror::Error;

use crate::models::{Marker, ShotRange, SourceMarkers};

#[derive(Debug, Error)]
pub enum MarkerError {
    #[error("source not found in manifest: {0}")]
    SourceNotFound(String),
    #[error("no range specified (use --from-word/--to-word, --from-scene/--to-scene, or --from-ms/--to-ms)")]
    NoRange,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse markers JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Add a marker to a source. Creates the markers file if it doesn't exist.
///
/// Returns the newly created marker.
pub fn add_marker(
    project_dir: &Path,
    source_id: &str,
    range: ShotRange,
    label: &str,
    note: Option<&str>,
) -> Result<Marker, MarkerError> {
    let path = markers_path(project_dir, source_id);
    let mut doc = load_or_create(source_id, &path)?;

    let next_id = next_marker_id(&doc);
    let marker = Marker {
        id: format!("mark-{:03}", next_id),
        range,
        label: label.to_string(),
        note: note.map(|s| s.to_string()),
        created: Utc::now(),
    };

    doc.markers.push(marker.clone());
    save_markers(&path, &doc)?;

    Ok(marker)
}

/// Load all markers for a source. Returns an empty SourceMarkers if the file doesn't exist.
pub fn list_markers(
    project_dir: &Path,
    source_id: &str,
) -> Result<SourceMarkers, MarkerError> {
    let path = markers_path(project_dir, source_id);
    load_or_create(source_id, &path)
}

/// Load all markers across all sources by scanning `annotations/*.markers.json`.
pub fn list_all_markers(project_dir: &Path) -> Result<Vec<SourceMarkers>, MarkerError> {
    let annotations_dir = project_dir.join("annotations");
    if !annotations_dir.exists() {
        return Ok(vec![]);
    }

    let mut results = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(&annotations_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".markers.json"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let content = fs::read_to_string(entry.path())?;
        let doc: SourceMarkers = serde_json::from_str(&content)?;
        if !doc.markers.is_empty() {
            results.push(doc);
        }
    }

    Ok(results)
}

/// Build the path to a source's markers file: `annotations/<source_id>.markers.json`.
fn markers_path(project_dir: &Path, source_id: &str) -> std::path::PathBuf {
    project_dir
        .join("annotations")
        .join(format!("{source_id}.markers.json"))
}

/// Load markers from disk, or create an empty SourceMarkers if the file doesn't exist.
fn load_or_create(source_id: &str, path: &Path) -> Result<SourceMarkers, MarkerError> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let doc: SourceMarkers = serde_json::from_str(&content)?;
        Ok(doc)
    } else {
        Ok(SourceMarkers {
            source_id: source_id.to_string(),
            markers: vec![],
        })
    }
}

/// Save markers to disk as pretty-printed JSON.
fn save_markers(path: &Path, doc: &SourceMarkers) -> Result<(), MarkerError> {
    let json = serde_json::to_string_pretty(doc)?;
    fs::write(path, json)?;
    Ok(())
}

/// Compute the next marker ID number from existing markers.
fn next_marker_id(doc: &SourceMarkers) -> u32 {
    doc.markers
        .iter()
        .filter_map(|m| {
            m.id.strip_prefix("mark-")
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .map(|n| n + 1)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ShotRange;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("annotations")).unwrap();
        tmp
    }

    #[test]
    fn add_first_marker() {
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

        // Verify persisted to disk
        let loaded = list_markers(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.markers.len(), 1);
        assert_eq!(loaded.markers[0].id, "mark-001");
    }

    #[test]
    fn add_multiple_markers_sequential_ids() {
        let tmp = setup_project();

        add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Words { from: 0, to: 50 },
            "select",
            None,
        )
        .unwrap();

        add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Time { from_ms: 62000, to_ms: 68000 },
            "avoid",
            Some("Bad audio"),
        )
        .unwrap();

        add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Scenes { from: 1, to: 2 },
            "hero",
            None,
        )
        .unwrap();

        let loaded = list_markers(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.markers.len(), 3);
        assert_eq!(loaded.markers[0].id, "mark-001");
        assert_eq!(loaded.markers[1].id, "mark-002");
        assert_eq!(loaded.markers[2].id, "mark-003");
    }

    #[test]
    fn markers_without_note() {
        let tmp = setup_project();
        let marker = add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Time { from_ms: 1000, to_ms: 2000 },
            "maybe",
            None,
        )
        .unwrap();

        assert_eq!(marker.note, None);
    }

    #[test]
    fn list_markers_empty_source() {
        let tmp = setup_project();
        let loaded = list_markers(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.source_id, "src-001");
        assert!(loaded.markers.is_empty());
    }

    #[test]
    fn markers_per_source_isolation() {
        let tmp = setup_project();

        add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Words { from: 0, to: 10 },
            "select",
            None,
        )
        .unwrap();

        add_marker(
            tmp.path(),
            "src-002",
            ShotRange::Words { from: 0, to: 20 },
            "hero",
            None,
        )
        .unwrap();

        let src1 = list_markers(tmp.path(), "src-001").unwrap();
        let src2 = list_markers(tmp.path(), "src-002").unwrap();
        assert_eq!(src1.markers.len(), 1);
        assert_eq!(src2.markers.len(), 1);
        assert_eq!(src1.markers[0].label, "select");
        assert_eq!(src2.markers[0].label, "hero");
        // Each source starts its own ID sequence
        assert_eq!(src1.markers[0].id, "mark-001");
        assert_eq!(src2.markers[0].id, "mark-001");
    }

    #[test]
    fn markers_file_roundtrip() {
        let tmp = setup_project();

        add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Words { from: 45, to: 120 },
            "select",
            Some("Best take of the climate answer"),
        )
        .unwrap();

        add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Time { from_ms: 62000, to_ms: 68000 },
            "avoid",
            None,
        )
        .unwrap();

        // Read raw JSON and verify structure
        let path = tmp.path().join("annotations/src-001.markers.json");
        let content = fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(json["source_id"], "src-001");
        assert_eq!(json["markers"].as_array().unwrap().len(), 2);
        assert_eq!(json["markers"][0]["id"], "mark-001");
        assert_eq!(json["markers"][0]["label"], "select");
        assert_eq!(
            json["markers"][0]["range"],
            serde_json::json!({ "words": { "from": 45, "to": 120 } })
        );
        assert_eq!(json["markers"][1]["id"], "mark-002");
        assert_eq!(json["markers"][1]["label"], "avoid");
        assert!(json["markers"][1]["note"].is_null());
    }

    #[test]
    fn freeform_label_accepted() {
        let tmp = setup_project();
        let marker = add_marker(
            tmp.path(),
            "src-001",
            ShotRange::Words { from: 0, to: 10 },
            "great-energy",
            None,
        )
        .unwrap();

        assert_eq!(marker.label, "great-energy");
    }

    #[test]
    fn next_marker_id_computation() {
        let doc = SourceMarkers {
            source_id: "src-001".into(),
            markers: vec![
                Marker {
                    id: "mark-001".into(),
                    range: ShotRange::Words { from: 0, to: 10 },
                    label: "select".into(),
                    note: None,
                    created: Utc::now(),
                },
                Marker {
                    id: "mark-005".into(),
                    range: ShotRange::Words { from: 20, to: 30 },
                    label: "hero".into(),
                    note: None,
                    created: Utc::now(),
                },
            ],
        };
        assert_eq!(next_marker_id(&doc), 6);
    }
}
