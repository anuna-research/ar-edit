//! TEST-011: Edit creation
//!
//! Verifies that `EditDocument::create()` produces a correctly initialised
//! document and that the document survives a save → load roundtrip.

use ar_edit_core::models::EditDocument;
use tempfile::TempDir;

#[test]
fn create_sets_name() {
    let doc = EditDocument::create("rough-cut");
    assert_eq!(doc.name, "rough-cut");
}

#[test]
fn create_starts_with_empty_state() {
    let doc = EditDocument::create("test");
    assert_eq!(doc.head, -1);
    assert!(doc.ops.is_empty());
    assert!(doc.snapshot.shots.is_empty());
    assert_eq!(doc.next_shot_id, 1);
}

#[test]
fn create_timestamp_is_recent() {
    let before = chrono::Utc::now();
    let doc = EditDocument::create("test");
    let after = chrono::Utc::now();
    assert!(doc.created >= before && doc.created <= after);
}

#[test]
fn create_accepts_string_types() {
    let _from_str = EditDocument::create("name-a");
    let _from_string = EditDocument::create(String::from("name-b"));
    let _from_ref = EditDocument::create(String::from("name-c"));
}

#[test]
fn empty_document_save_load_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("empty.edit.json");

    let doc = EditDocument::create("empty");
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.name, "empty");
    assert_eq!(loaded.head, -1);
    assert!(loaded.ops.is_empty());
    assert!(loaded.snapshot.shots.is_empty());
    assert_eq!(loaded.next_shot_id, 1);
}

#[test]
fn saved_json_has_expected_structure() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.edit.json");

    let doc = EditDocument::create("structured");
    doc.save(&path).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(json["name"], "structured");
    assert_eq!(json["head"], -1);
    assert_eq!(json["next_shot_id"], 1);
    assert!(json["ops"].as_array().unwrap().is_empty());
    assert!(json["snapshot"]["shots"].as_array().unwrap().is_empty());
    assert!(json.get("created").is_some());
}

#[test]
fn multiple_creates_are_independent() {
    let a = EditDocument::create("alpha");
    let b = EditDocument::create("beta");

    assert_eq!(a.name, "alpha");
    assert_eq!(b.name, "beta");
    assert_ne!(a.name, b.name);
}
