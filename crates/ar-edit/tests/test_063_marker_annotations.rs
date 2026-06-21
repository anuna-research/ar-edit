//! Markers route through the per-source annotation CRDT store (ADR-015):
//! `mark` writes `annotations/<source>.annot.json`, `markers` reads it, and a
//! first write migrates any legacy `annotations/<source>.markers.json`.
#![allow(deprecated)] // assert_cmd::cargo_bin — matches the rest of the suite

use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

use ar_edit_collab::annotations::AnnotationStore;
use ar_edit_collab::ids::ActorId;
use ar_edit_core::models::ShotRange;

/// A minimal two-source project manifest (Time ranges need only a duration).
fn manifest_json() -> String {
    let source = |id: &str| {
        format!(
            r#"{{"id":"{id}","path":"sources/{id}.mp4","original_filename":"{id}.mp4",
            "duration_ms":100000,"video_codec":"h264","audio_codec":"aac",
            "resolution":[1920,1080],"frame_rate":30.0,"audio_channels":2,
            "audio_sample_rate":48000,"added":"2020-01-01T00:00:00Z",
            "transcribed":false,"indexed":false}}"#
        )
    };
    format!(
        r#"{{"version":"1","name":"t","created":"2020-01-01T00:00:00Z",
        "sources":[{},{}],"next_source_id":3,
        "defaults":{{"whisper_model":"base","thumbnail_interval_sec":10,
        "render_codec":"h264","render_container":"mp4"}}}}"#,
        source("src-001"),
        source("src-002"),
    )
}

fn annot_labels(project: &Path, source_id: &str) -> Vec<String> {
    let bytes = fs::read(project.join(format!("annotations/{source_id}.annot.json"))).unwrap();
    AnnotationStore::from_bytes(&bytes, ActorId(1))
        .unwrap()
        .markers()
        .into_iter()
        .map(|m| m.label)
        .collect()
}

fn annot_poi_ids(project: &Path, source_id: &str) -> Vec<String> {
    let bytes = fs::read(project.join(format!("annotations/{source_id}.annot.json"))).unwrap();
    AnnotationStore::from_bytes(&bytes, ActorId(1))
        .unwrap()
        .pois()
        .into_iter()
        .map(|p| p.id)
        .collect()
}

fn annot_marker_authors(project: &Path, source_id: &str) -> Vec<String> {
    let bytes = fs::read(project.join(format!("annotations/{source_id}.annot.json"))).unwrap();
    AnnotationStore::from_bytes(&bytes, ActorId(1))
        .unwrap()
        .markers()
        .into_iter()
        .map(|m| m.author)
        .collect()
}

fn annot_poi_authors(project: &Path, source_id: &str) -> Vec<String> {
    let bytes = fs::read(project.join(format!("annotations/{source_id}.annot.json"))).unwrap();
    AnnotationStore::from_bytes(&bytes, ActorId(1))
        .unwrap()
        .pois()
        .into_iter()
        .map(|p| p.author)
        .collect()
}

/// TEST-121 — Validates: REQ-091 (author attribution autopopulates, persists in
/// the CRDT, and shows in output).
#[test]
fn markers_and_pois_record_the_author() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("manifest.json"), manifest_json()).unwrap();
    let ar = || {
        let mut c = Command::cargo_bin("ar-edit").unwrap();
        c.current_dir(tmp.path());
        c.env("AR_EDIT_AUTHOR", "alice"); // attribution autopopulation override
        c
    };

    let out = ar()
        .args([
            "mark",
            "src-001",
            "--label",
            "select",
            "--from-ms",
            "1000",
            "--to-ms",
            "2000",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("by alice"),
        "marker output shows author"
    );
    assert_eq!(annot_marker_authors(tmp.path(), "src-001"), vec!["alice"]);

    let out = ar()
        .args([
            "poi",
            "add",
            "src-001",
            "--at-ms",
            "5000",
            "--category",
            "highlight",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("by alice"),
        "POI output shows author"
    );
    assert_eq!(annot_poi_authors(tmp.path(), "src-001"), vec!["alice"]);
}

#[test]
fn poi_add_list_remove_through_crdt_store() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("manifest.json"), manifest_json()).unwrap();

    let ar = || {
        let mut c = Command::cargo_bin("ar-edit").unwrap();
        c.current_dir(tmp.path());
        c
    };

    // Add a POI — lands in the CRDT annotation store, shared with markers.
    ar().args([
        "poi",
        "add",
        "src-001",
        "--at-ms",
        "5000",
        "--category",
        "highlight",
    ])
    .assert()
    .success();
    assert_eq!(annot_poi_ids(tmp.path(), "src-001"), vec!["poi-001"]);

    // `poi list` reads it back.
    let out = ar()
        .args(["--json", "poi", "list", "src-001"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // Flat list, each POI tagged with its source_id (consistent with `markers --json`).
    let pois = v["pois"].as_array().unwrap();
    assert_eq!(pois.len(), 1);
    assert_eq!(pois[0]["id"], "poi-001");
    assert_eq!(pois[0]["source_id"], "src-001");

    // Remove it.
    ar().args(["poi", "remove", "src-001", "--id", "poi-001"])
        .assert()
        .success();
    assert!(annot_poi_ids(tmp.path(), "src-001").is_empty());
}

#[test]
fn markers_and_pois_share_one_annotation_store() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("manifest.json"), manifest_json()).unwrap();
    let ar = || {
        let mut c = Command::cargo_bin("ar-edit").unwrap();
        c.current_dir(tmp.path());
        c
    };

    ar().args([
        "mark",
        "src-001",
        "--label",
        "select",
        "--from-ms",
        "1000",
        "--to-ms",
        "2000",
    ])
    .assert()
    .success();
    ar().args([
        "poi",
        "add",
        "src-001",
        "--at-ms",
        "5000",
        "--category",
        "issue",
    ])
    .assert()
    .success();

    // One CRDT store holds both the marker and the POI for the source.
    assert_eq!(annot_labels(tmp.path(), "src-001"), vec!["select"]);
    assert_eq!(annot_poi_ids(tmp.path(), "src-001"), vec!["poi-001"]);
}

#[test]
fn mark_writes_crdt_store_and_markers_lists_it() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("manifest.json"), manifest_json()).unwrap();

    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args([
            "mark",
            "src-001",
            "--label",
            "select",
            "--from-ms",
            "1000",
            "--to-ms",
            "2000",
        ])
        .assert()
        .success();

    // The marker landed in the CRDT annotation store, not a plain markers file.
    assert!(tmp.path().join("annotations/src-001.annot.json").exists());
    assert_eq!(annot_labels(tmp.path(), "src-001"), vec!["select"]);

    // `markers` reads it back.
    let out = Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args(["--json", "markers", "src-001"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["markers"].as_array().unwrap().len(), 1);
}

#[test]
fn mark_migrates_legacy_markers_then_adds() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("manifest.json"), manifest_json()).unwrap();
    fs::create_dir_all(tmp.path().join("annotations")).unwrap();

    // A legacy plain-JSON marker file (written in the correct format by the core
    // helper) for a source with no CRDT store yet.
    ar_edit_core::marker::add_marker(
        tmp.path(),
        "src-002",
        ShotRange::Time {
            from_ms: 500,
            to_ms: 600,
        },
        "legacy",
        None,
    )
    .unwrap();

    // The next `mark` migrates the legacy file into the CRDT store and adds the
    // new marker — both must be present.
    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args([
            "mark",
            "src-002",
            "--label",
            "fresh",
            "--from-ms",
            "1000",
            "--to-ms",
            "2000",
        ])
        .assert()
        .success();

    let labels = annot_labels(tmp.path(), "src-002");
    assert!(
        labels.contains(&"legacy".to_string()),
        "legacy marker migrated: {labels:?}"
    );
    assert!(
        labels.contains(&"fresh".to_string()),
        "new marker added: {labels:?}"
    );
}
