//! Phase 4: one-shot CLI commands ATTACH to a live host (SPEC-003 REQ-090, OQ-8).
//!
//! When `ar-edit daemon --edit <name>` owns an edit, a one-shot `ar-edit edit …`
//! / `ar-edit undo` routes through the host (the daemon mutates and persists the
//! canonical file) rather than writing the file behind the host's back — one
//! store, no divergence. Spawns a real daemon child and drives the CLI against
//! it. Unix-only (the daemon uses a Unix socket).
#![cfg(unix)]

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use std::fs;
use std::path::Path;
use std::process::{Child, Command as StdCommand};
use std::time::Duration;
use tempfile::TempDir;

use ar_edit_collab::ids::ActorId;
use ar_edit_collab::store::PersistentEdit;
use ar_edit_core::models::ShotRange;

/// Kills the spawned daemon on drop, even if the test panics.
struct DaemonGuard(Child);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn words() -> ShotRange {
    ShotRange::Words { from: 0, to: 10 }
}

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

#[test]
fn one_shot_commands_route_through_a_live_host() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("edits")).unwrap();
    let edit_path = tmp.path().join("edits/rc.edit.json");

    // Seed an edit with two shots.
    let mut store = PersistentEdit::create("rc", ActorId(1));
    store.add_shot("src-001", words(), None).unwrap();
    let drop_id = store.add_shot("src-002", words(), None).unwrap();
    fs::write(&edit_path, store.to_bytes()).unwrap();
    let original = shot_ids(&edit_path);
    assert_eq!(original.len(), 2);

    // Start a daemon hosting "rc".
    let _daemon = DaemonGuard(
        StdCommand::new(cargo_bin("ar-edit"))
            .args(["daemon", "--edit", "rc"])
            .current_dir(tmp.path())
            .spawn()
            .expect("spawn daemon"),
    );
    // Wait for the socket to come up.
    let sock = tmp.path().join(".ar-edit/session.sock");
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sock.exists(), "daemon socket should appear");
    std::thread::sleep(Duration::from_millis(50));

    // One-shot remove — must attach to the host, which mutates + persists.
    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args(["edit", "remove-segment", "rc", "--shot", &drop_id])
        .assert()
        .success();
    assert_eq!(
        shot_ids(&edit_path).len(),
        1,
        "remove routed through the host and was persisted to the canonical file"
    );

    // One-shot undo — also attaches; the host reverts and persists.
    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args(["undo", "rc"])
        .assert()
        .success();
    assert_eq!(
        shot_ids(&edit_path),
        original,
        "undo routed through the host restored the shot in the file"
    );
}
