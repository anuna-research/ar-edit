//! TEST-051 / TEST-052 / TEST-053 / TEST-053b: Undo, redo, fork, and history
//!
//! Integration tests that exercise `ar-edit undo`, `ar-edit redo`, and
//! `ar-edit edit history` via the compiled binary.  Each test creates a
//! temporary project directory with an edit document, runs the CLI, and
//! asserts on exit codes and output.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

use ar_edit_core::models::{EditDocument, ShotRange};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ar_edit() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("ar-edit").expect("binary ar-edit should be built")
}

/// Create a temporary directory with an `edits/` subdirectory and return
/// the temp dir handle plus the path to the edit file.
fn setup_project(name: &str) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let edits_dir = tmp.path().join("edits");
    fs::create_dir(&edits_dir).unwrap();
    let edit_path = edits_dir.join(format!("{name}.edit.json"));
    (tmp, edit_path)
}

/// Build a document with three add_shot ops (head=2).
fn doc_with_three_shots() -> EditDocument {
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();
    doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 })
        .unwrap();
    doc
}

/// Assert a string contains a substring (with a nice error message).
fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected output to contain {needle:?}, got:\n{haystack}"
    );
}

// ---------------------------------------------------------------------------
// TEST-051: Undo reverts last operation
// ---------------------------------------------------------------------------

#[test]
fn undo_reverts_last_op_text() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(&stdout, "Undone");
    assert_contains(&stdout, "add_shot");
    assert_contains(&stdout, "shot-003");

    // Verify on-disk state
    let loaded = EditDocument::load(&edit_path).unwrap();
    assert_eq!(loaded.head, 1);
    assert_eq!(loaded.snapshot.shots.len(), 2);
    assert_eq!(loaded.ops.len(), 3); // ops preserved for redo
}

#[test]
fn undo_reverts_last_op_json() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "undo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["head"], 1);
    assert!(json["undone_op"].is_object());
    assert_eq!(json["undone_op"]["id"], 2);
    assert_eq!(json["undone_op"]["op"], "add_shot");
}

#[test]
fn undo_multiple_times() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    doc.save(&edit_path).unwrap();

    // Undo three times
    for _ in 0..3 {
        ar_edit()
            .current_dir(tmp.path())
            .args(["undo", "rough-cut"])
            .assert()
            .success();
    }

    let loaded = EditDocument::load(&edit_path).unwrap();
    assert_eq!(loaded.head, -1);
    assert!(loaded.snapshot.shots.is_empty());
    assert_eq!(loaded.ops.len(), 3); // all ops preserved
}

#[test]
fn undo_nothing_to_undo_exits_1() {
    let (tmp, edit_path) = setup_project("empty");
    let doc = EditDocument::create("empty");
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "empty"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(&stderr, "nothing to undo");
}

#[test]
fn undo_nothing_to_undo_json() {
    let (tmp, edit_path) = setup_project("empty");
    let doc = EditDocument::create("empty");
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "undo", "empty"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_contains(json["error"].as_str().unwrap(), "nothing to undo");
}

#[test]
fn undo_nonexistent_edit_exits_1() {
    let tmp = TempDir::new().unwrap();
    let edits_dir = tmp.path().join("edits");
    fs::create_dir(&edits_dir).unwrap();

    ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "nonexistent"])
        .assert()
        .code(1);
}

// ---------------------------------------------------------------------------
// TEST-052: Redo re-applies after undo
// ---------------------------------------------------------------------------

#[test]
fn redo_after_undo_text() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    doc.save(&edit_path).unwrap();

    // Undo once
    ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "rough-cut"])
        .assert()
        .success();

    // Redo
    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["redo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(&stdout, "Redone");
    assert_contains(&stdout, "add_shot");
    assert_contains(&stdout, "shot-003");

    let loaded = EditDocument::load(&edit_path).unwrap();
    assert_eq!(loaded.head, 2);
    assert_eq!(loaded.snapshot.shots.len(), 3);
}

#[test]
fn redo_after_undo_json() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    doc.save(&edit_path).unwrap();

    // Undo once
    ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "rough-cut"])
        .assert()
        .success();

    // Redo with JSON
    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "redo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["head"], 2);
    assert_eq!(json["redone_op"]["id"], 2);
    assert_eq!(json["redone_op"]["op"], "add_shot");
}

#[test]
fn redo_nothing_to_redo_exits_1() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["redo", "rough-cut"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(&stderr, "nothing to redo");
}

#[test]
fn undo_redo_roundtrip_preserves_snapshot() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let doc = doc_with_three_shots();
    let original_snapshot = doc.snapshot.clone();
    doc.save(&edit_path).unwrap();

    // Undo all three
    for _ in 0..3 {
        ar_edit()
            .current_dir(tmp.path())
            .args(["undo", "rough-cut"])
            .assert()
            .success();
    }

    // Redo all three
    for _ in 0..3 {
        ar_edit()
            .current_dir(tmp.path())
            .args(["redo", "rough-cut"])
            .assert()
            .success();
    }

    let loaded = EditDocument::load(&edit_path).unwrap();
    assert_eq!(loaded.head, 2);
    assert_eq!(loaded.snapshot, original_snapshot);
}

// ---------------------------------------------------------------------------
// TEST-053: New edit after undo forks history
// ---------------------------------------------------------------------------

#[test]
fn new_op_after_undo_truncates_redo_history() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let mut doc = doc_with_three_shots();

    // Undo two ops (head moves to 0)
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.head, 0);

    // Add a new shot — this forks: ops[1] and ops[2] are discarded
    doc.add_shot(
        "src-004",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();
    doc.save(&edit_path).unwrap();

    // Verify fork happened
    let loaded = EditDocument::load(&edit_path).unwrap();
    assert_eq!(loaded.ops.len(), 2);
    assert_eq!(loaded.head, 1);
    assert_eq!(loaded.snapshot.shots.len(), 2);
    assert_eq!(loaded.snapshot.shots[0].id, "shot-001");
    assert_eq!(loaded.snapshot.shots[1].id, "shot-004");

    // Redo should fail since the redo history was discarded
    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["redo", "rough-cut"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_contains(&stderr, "nothing to redo");
}

// ---------------------------------------------------------------------------
// TEST-053b: Operation history display
// ---------------------------------------------------------------------------

#[test]
fn history_text_shows_all_ops_with_head_marker() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let mut doc = doc_with_three_shots();
    // Undo once so head=1, ops[2] is "undone"
    doc.undo().unwrap();
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["edit", "history", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // All three ops should be listed
    assert_contains(&stdout, "shot-001");
    assert_contains(&stdout, "shot-002");
    assert_contains(&stdout, "shot-003");

    // Head marker (→) should appear on op at index 1
    assert_contains(&stdout, "\u{2192}");
    // Op at index 2 should be marked as undone
    assert_contains(&stdout, "(undone)");
}

#[test]
fn history_json_output() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let mut doc = doc_with_three_shots();
    doc.undo().unwrap(); // head=1
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "edit", "history", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["head"], 1);
    let ops = json["ops"].as_array().unwrap();
    assert_eq!(ops.len(), 3);
    assert_eq!(ops[0]["op"], "add_shot");
    assert_eq!(ops[1]["op"], "add_shot");
    assert_eq!(ops[2]["op"], "add_shot");
}

#[test]
fn history_empty_edit() {
    let (tmp, edit_path) = setup_project("empty");
    let doc = EditDocument::create("empty");
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["edit", "history", "empty"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(&stdout, "No operations");
}

#[test]
fn history_json_empty_edit() {
    let (tmp, edit_path) = setup_project("empty");
    let doc = EditDocument::create("empty");
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "edit", "history", "empty"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["head"], -1);
    assert_eq!(json["ops"].as_array().unwrap().len(), 0);
}

#[test]
fn history_shows_mixed_op_types() {
    let (tmp, edit_path) = setup_project("rough-cut");
    let mut doc = doc_with_three_shots();
    doc.move_shot("shot-003", 0).unwrap();
    doc.trim_shot("shot-001", ShotRange::Words { from: 5, to: 45 })
        .unwrap();
    doc.save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["edit", "history", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_contains(&stdout, "add_shot");
    assert_contains(&stdout, "move_shot");
    assert_contains(&stdout, "trim_shot");
}
