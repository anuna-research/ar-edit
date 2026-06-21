use std::fs;
use std::path::Path;

use chrono::Utc;
use thiserror::Error;

use crate::models::{Poi, PoiCategory, PoiPoint, SourcePois};

#[derive(Debug, Error)]
pub enum PoiError {
    #[error("source not found in manifest: {0}")]
    SourceNotFound(String),
    #[error("POI not found: {0}")]
    PoiNotFound(String),
    #[error("invalid POI category: {0}")]
    InvalidCategory(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse POIs JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Add a POI to a source. Creates the POIs file if it doesn't exist.
///
/// Returns the newly created POI.
pub fn add_poi(
    project_dir: &Path,
    source_id: &str,
    point: PoiPoint,
    category: PoiCategory,
    note: Option<&str>,
) -> Result<Poi, PoiError> {
    let path = pois_path(project_dir, source_id);
    let mut doc = load_or_create(source_id, &path)?;

    let next_id = next_poi_id(&doc);
    let poi = Poi {
        id: format!("poi-{next_id:03}"),
        point,
        category,
        note: note.map(|s| s.to_string()),
        author: String::new(), // attribution is stamped by the CLI via the CRDT store
        created: Utc::now(),
    };

    doc.pois.push(poi.clone());
    save_pois(&path, &doc)?;

    Ok(poi)
}

/// Load all POIs for a source. Returns an empty SourcePois if the file doesn't exist.
pub fn list_pois(project_dir: &Path, source_id: &str) -> Result<SourcePois, PoiError> {
    let path = pois_path(project_dir, source_id);
    load_or_create(source_id, &path)
}

/// Load all POIs across all sources by scanning `annotations/*.pois.json`.
pub fn list_all_pois(project_dir: &Path) -> Result<Vec<SourcePois>, PoiError> {
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
                .map(|n| n.ends_with(".pois.json"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let content = fs::read_to_string(entry.path())?;
        let doc: SourcePois = serde_json::from_str(&content)?;
        if !doc.pois.is_empty() {
            results.push(doc);
        }
    }

    Ok(results)
}

/// Remove a single POI by ID. Returns the removed POI's ID.
///
/// The ID is not reused; `next_poi_id` is still based on the max existing ID.
pub fn remove_poi(project_dir: &Path, source_id: &str, poi_id: &str) -> Result<String, PoiError> {
    let path = pois_path(project_dir, source_id);
    let mut doc = load_or_create(source_id, &path)?;

    let idx = doc
        .pois
        .iter()
        .position(|p| p.id == poi_id)
        .ok_or_else(|| PoiError::PoiNotFound(poi_id.to_string()))?;

    let removed = doc.pois.remove(idx);
    save_pois(&path, &doc)?;

    Ok(removed.id)
}

/// Remove all POIs matching a category. Returns the IDs of removed POIs.
pub fn remove_pois_by_category(
    project_dir: &Path,
    source_id: &str,
    category: PoiCategory,
) -> Result<Vec<String>, PoiError> {
    let path = pois_path(project_dir, source_id);
    let mut doc = load_or_create(source_id, &path)?;

    let mut removed_ids = Vec::new();
    let mut kept = Vec::new();

    for poi in doc.pois {
        if poi.category == category {
            removed_ids.push(poi.id);
        } else {
            kept.push(poi);
        }
    }

    doc.pois = kept;
    save_pois(&path, &doc)?;

    Ok(removed_ids)
}

/// Build the path to a source's POIs file: `annotations/<source_id>.pois.json`.
fn pois_path(project_dir: &Path, source_id: &str) -> std::path::PathBuf {
    project_dir
        .join("annotations")
        .join(format!("{source_id}.pois.json"))
}

/// Load POIs from disk, or create an empty SourcePois if the file doesn't exist.
fn load_or_create(source_id: &str, path: &Path) -> Result<SourcePois, PoiError> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let doc: SourcePois = serde_json::from_str(&content)?;
        Ok(doc)
    } else {
        Ok(SourcePois {
            source_id: source_id.to_string(),
            pois: vec![],
        })
    }
}

/// Save POIs to disk as pretty-printed JSON.
fn save_pois(path: &Path, doc: &SourcePois) -> Result<(), PoiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(doc)?;
    fs::write(path, json)?;
    Ok(())
}

/// Compute the next POI ID number from existing POIs.
fn next_poi_id(doc: &SourcePois) -> u32 {
    doc.pois
        .iter()
        .filter_map(|p| {
            p.id.strip_prefix("poi-")
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .map(|n| n + 1)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PoiCategory, PoiPoint};
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("annotations")).unwrap();
        tmp
    }

    #[test]
    fn add_first_poi() {
        let tmp = setup_project();
        let poi = add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(45),
            PoiCategory::Highlight,
            Some("Perfect delivery"),
        )
        .unwrap();

        assert_eq!(poi.id, "poi-001");
        assert_eq!(poi.point, PoiPoint::Word(45));
        assert_eq!(poi.category, PoiCategory::Highlight);
        assert_eq!(poi.note.as_deref(), Some("Perfect delivery"));

        // Verify persisted to disk
        let loaded = list_pois(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.pois.len(), 1);
        assert_eq!(loaded.pois[0].id, "poi-001");
    }

    #[test]
    fn add_multiple_pois_sequential_ids() {
        let tmp = setup_project();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(10),
            PoiCategory::Highlight,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::TimeMs(62500),
            PoiCategory::Issue,
            Some("Bad audio"),
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Scene(3),
            PoiCategory::Transition,
            None,
        )
        .unwrap();

        let loaded = list_pois(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.pois.len(), 3);
        assert_eq!(loaded.pois[0].id, "poi-001");
        assert_eq!(loaded.pois[1].id, "poi-002");
        assert_eq!(loaded.pois[2].id, "poi-003");
    }

    #[test]
    fn pois_without_note() {
        let tmp = setup_project();
        let poi = add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::TimeMs(1000),
            PoiCategory::Cue,
            None,
        )
        .unwrap();

        assert_eq!(poi.note, None);
    }

    #[test]
    fn list_pois_empty_source() {
        let tmp = setup_project();
        let loaded = list_pois(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.source_id, "src-001");
        assert!(loaded.pois.is_empty());
    }

    #[test]
    fn pois_per_source_isolation() {
        let tmp = setup_project();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(10),
            PoiCategory::Highlight,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-002",
            PoiPoint::Word(20),
            PoiCategory::Issue,
            None,
        )
        .unwrap();

        let src1 = list_pois(tmp.path(), "src-001").unwrap();
        let src2 = list_pois(tmp.path(), "src-002").unwrap();
        assert_eq!(src1.pois.len(), 1);
        assert_eq!(src2.pois.len(), 1);
        assert_eq!(src1.pois[0].category, PoiCategory::Highlight);
        assert_eq!(src2.pois[0].category, PoiCategory::Issue);
        // Each source starts its own ID sequence
        assert_eq!(src1.pois[0].id, "poi-001");
        assert_eq!(src2.pois[0].id, "poi-001");
    }

    #[test]
    fn pois_file_roundtrip() {
        let tmp = setup_project();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(45),
            PoiCategory::Highlight,
            Some("Perfect delivery of the key statistic"),
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::TimeMs(62500),
            PoiCategory::Issue,
            None,
        )
        .unwrap();

        // Read raw JSON and verify structure
        let path = tmp.path().join("annotations/src-001.pois.json");
        let content = fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(json["source_id"], "src-001");
        assert_eq!(json["pois"].as_array().unwrap().len(), 2);
        assert_eq!(json["pois"][0]["id"], "poi-001");
        assert_eq!(json["pois"][0]["category"], "highlight");
        assert_eq!(json["pois"][0]["point"], serde_json::json!({ "word": 45 }));
        assert_eq!(json["pois"][1]["id"], "poi-002");
        assert_eq!(json["pois"][1]["category"], "issue");
        assert!(json["pois"][1]["note"].is_null());
    }

    #[test]
    fn remove_single_poi() {
        let tmp = setup_project();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(10),
            PoiCategory::Highlight,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(20),
            PoiCategory::Issue,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(30),
            PoiCategory::Cue,
            None,
        )
        .unwrap();

        // Remove the middle one
        let removed_id = remove_poi(tmp.path(), "src-001", "poi-002").unwrap();
        assert_eq!(removed_id, "poi-002");

        let loaded = list_pois(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.pois.len(), 2);
        assert_eq!(loaded.pois[0].id, "poi-001");
        assert_eq!(loaded.pois[1].id, "poi-003");

        // Next ID should be 4, not 3 (ID not reused)
        let poi = add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(40),
            PoiCategory::Note,
            None,
        )
        .unwrap();
        assert_eq!(poi.id, "poi-004");
    }

    #[test]
    fn remove_pois_by_category_bulk() {
        let tmp = setup_project();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(10),
            PoiCategory::Issue,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(20),
            PoiCategory::Highlight,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(30),
            PoiCategory::Issue,
            None,
        )
        .unwrap();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(40),
            PoiCategory::Cue,
            None,
        )
        .unwrap();

        let removed = remove_pois_by_category(tmp.path(), "src-001", PoiCategory::Issue).unwrap();
        assert_eq!(removed, vec!["poi-001", "poi-003"]);

        let loaded = list_pois(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded.pois.len(), 2);
        assert_eq!(loaded.pois[0].id, "poi-002");
        assert_eq!(loaded.pois[0].category, PoiCategory::Highlight);
        assert_eq!(loaded.pois[1].id, "poi-004");
        assert_eq!(loaded.pois[1].category, PoiCategory::Cue);
    }

    #[test]
    fn remove_poi_not_found() {
        let tmp = setup_project();

        add_poi(
            tmp.path(),
            "src-001",
            PoiPoint::Word(10),
            PoiCategory::Highlight,
            None,
        )
        .unwrap();

        let err = remove_poi(tmp.path(), "src-001", "poi-999").unwrap_err();
        assert!(matches!(err, PoiError::PoiNotFound(_)));
        assert!(err.to_string().contains("poi-999"));
    }

    #[test]
    fn next_poi_id_computation() {
        let doc = SourcePois {
            source_id: "src-001".into(),
            pois: vec![
                Poi {
                    id: "poi-001".into(),
                    point: PoiPoint::Word(10),
                    category: PoiCategory::Highlight,
                    note: None,
                    author: String::new(),
                    created: Utc::now(),
                },
                Poi {
                    id: "poi-005".into(),
                    point: PoiPoint::Word(20),
                    category: PoiCategory::Issue,
                    note: None,
                    author: String::new(),
                    created: Utc::now(),
                },
            ],
        };
        assert_eq!(next_poi_id(&doc), 6);
    }
}
