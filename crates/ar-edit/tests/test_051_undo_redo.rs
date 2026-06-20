//! TEST-051 / TEST-052 / TEST-053 / TEST-053b: Undo, redo, fork, and history
//! over the CRDT-backed canonical store (ADR-011).
//!
//! Each test builds an edit (a legacy event-sourced doc is transparently
//! migrated on first open, REQ-088), drives `ar-edit undo`/`redo`/`history` via
//! the compiled binary, and asserts on the durable, single-store behaviour:
//! undo is durable over the CRDT oplog cursor and survives across one-shot CLI
//! invocations.

use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

use ar_edit_collab::ids::ActorId;
use ar_edit_collab::store::PersistentEdit;
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

/// A legacy event-sourced document with three add_shot ops (head=2). On first
/// open the CLI migrates it to the canonical CRDT store, reconstructing the undo
/// cursor from its op log so `undo` keeps working (REQ-088).
fn doc_with_three_shots() -> EditDocument {
    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 52 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();
    doc.add_shot("src-003", ShotRange::Words { from: 100, to: 200 }).unwrap();
    doc
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected output to contain {needle:?}, got:\n{haystack}"
    );
}

/// The materialised shot ids of an on-disk edit, read back through the canonical
/// store (the file is the CRDT format after the first CLI command saves it).
fn shot_ids(edit_path: &Path) -> Vec<String> {
    let bytes = fs::read(edit_path).unwrap();
    PersistentEdit::from_bytes(&bytes, ActorId(1))
        .unwrap()
        .snapshot()
        .shots
        .into_iter()
        .map(|s| s.id)
        .collect()
}

// ---------------------------------------------------------------------------
// TEST-051: Undo reverts the last operation, durably
// ---------------------------------------------------------------------------

#[test]
fn undo_reverts_last_op_text() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(&stdout, "Undone");
    assert_contains(&stdout, "shot-003"); // the reverted add's shot

    // Durable: the last shot is gone, the first two remain.
    let ids = shot_ids(&edit_path);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids, vec!["shot-001".to_string(), "shot-002".to_string()]);
}

#[test]
fn undo_reverts_last_op_json() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "undo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["undone"], true);
    assert_eq!(json["shots"], 2);
    assert_eq!(json["snapshot"]["shots"].as_array().unwrap().len(), 2);
}

#[test]
fn undo_is_durable_across_invocations() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();

    // Undo three times — each a SEPARATE process, proving durable cursor undo.
    for _ in 0..3 {
        ar_edit()
            .current_dir(tmp.path())
            .args(["undo", "rough-cut"])
            .assert()
            .success();
    }
    assert!(shot_ids(&edit_path).is_empty());

    // A fourth undo has nothing left.
    ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "rough-cut"])
        .assert()
        .code(1);
}

#[test]
fn undo_nothing_to_undo_exits_1() {
    let (tmp, edit_path) = setup_project("empty");
    EditDocument::create("empty").save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["undo", "empty"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_contains(&String::from_utf8_lossy(&output.stderr), "nothing to undo");
}

#[test]
fn undo_nothing_to_undo_json() {
    let (tmp, edit_path) = setup_project("empty");
    EditDocument::create("empty").save(&edit_path).unwrap();

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
    fs::create_dir(tmp.path().join("edits")).unwrap();
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
    doc_with_three_shots().save(&edit_path).unwrap();

    ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();
    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["redo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(&stdout, "Redone");
    assert_contains(&stdout, "shot-003");
    assert_eq!(shot_ids(&edit_path).len(), 3);
}

#[test]
fn redo_after_undo_json() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();

    ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();
    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "redo", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["redone"], true);
    assert_eq!(json["shots"], 3);
}

#[test]
fn redo_nothing_to_redo_exits_1() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["redo", "rough-cut"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_contains(&String::from_utf8_lossy(&output.stderr), "nothing to redo");
}

#[test]
fn undo_redo_roundtrip_preserves_shots() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();
    let original = shot_ids(&edit_path);

    for _ in 0..3 {
        ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();
    }
    for _ in 0..3 {
        ar_edit().current_dir(tmp.path()).args(["redo", "rough-cut"]).assert().success();
    }
    assert_eq!(shot_ids(&edit_path), original);
}

// ---------------------------------------------------------------------------
// TEST-053: A new edit after undo truncates the redo branch
// ---------------------------------------------------------------------------

#[test]
fn new_edit_after_undo_truncates_redo() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();

    // Undo twice (→ 1 shot), then make a NEW edit (remove the remaining shot —
    // no project manifest needed), forking the history.
    ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();
    ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();
    let remaining = shot_ids(&edit_path);
    assert_eq!(remaining.len(), 1);

    ar_edit()
        .current_dir(tmp.path())
        .args(["edit", "remove-segment", "rough-cut", "--shot", &remaining[0]])
        .assert()
        .success();
    assert!(shot_ids(&edit_path).is_empty());

    // Redo must now fail — the redo branch was discarded.
    ar_edit()
        .current_dir(tmp.path())
        .args(["redo", "rough-cut"])
        .assert()
        .code(1);
}

// ---------------------------------------------------------------------------
// TEST-053b: history shows the current timeline + undo/redo availability
// ---------------------------------------------------------------------------

#[test]
fn history_text_shows_timeline_and_undo_state() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();
    // Undo once so an undo is available and a redo is too.
    ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["edit", "history", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_contains(&stdout, "Timeline");
    assert_contains(&stdout, "shot-001");
    assert_contains(&stdout, "shot-002");
    assert_contains(&stdout, "undo available: true");
    assert_contains(&stdout, "redo available: true");
}

#[test]
fn history_json_output() {
    let (tmp, edit_path) = setup_project("rough-cut");
    doc_with_three_shots().save(&edit_path).unwrap();
    ar_edit().current_dir(tmp.path()).args(["undo", "rough-cut"]).assert().success();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "edit", "history", "rough-cut"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["shots"].as_array().unwrap().len(), 2);
    assert_eq!(json["can_undo"], true);
    assert_eq!(json["can_redo"], true);
}

#[test]
fn history_empty_edit() {
    let (tmp, edit_path) = setup_project("empty");
    EditDocument::create("empty").save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["edit", "history", "empty"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_contains(&String::from_utf8_lossy(&output.stdout), "No shots");
}

#[test]
fn history_json_empty_edit() {
    let (tmp, edit_path) = setup_project("empty");
    EditDocument::create("empty").save(&edit_path).unwrap();

    let output = ar_edit()
        .current_dir(tmp.path())
        .args(["--json", "edit", "history", "empty"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["shots"].as_array().unwrap().len(), 0);
    assert_eq!(json["can_undo"], false);
}
