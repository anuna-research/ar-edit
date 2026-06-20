//! Phase 4: one-shot CLI commands ATTACH to a live host (SPEC-003 REQ-090, OQ-8).
//!
//! When `ar-edit daemon --edit <name>` owns an edit, a one-shot `ar-edit edit …`
//! / `ar-edit undo` routes through the host (the daemon mutates and persists the
//! canonical file) rather than writing the file behind the host's back — one
//! store, no divergence. Spawns a real daemon child and drives the CLI against
//! it. Unix-only (the daemon uses a Unix socket).
#![cfg(unix)]
#![allow(deprecated)] // assert_cmd::cargo_bin — matches the rest of the suite

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
    // Wait for the socket to come up (generous, to tolerate parallel-test load).
    let sock = tmp.path().join(".ar-edit/session.sock");
    for _ in 0..500 {
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

/// Fail-closed (REQ-090): if a daemon socket is present but unresponsive, a
/// one-shot MUTATION must abort rather than write the file behind a possibly-live
/// host's back. A reviewer flagged this exact class for the old undo path; it
/// must not reappear through `attach_if_hosted`/`host_status`.
#[test]
fn one_shot_mutation_fails_closed_on_dead_socket() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("edits")).unwrap();
    let edit_path = tmp.path().join("edits/rc.edit.json");

    let mut store = PersistentEdit::create("rc", ActorId(1));
    let drop_id = store.add_shot("src-001", words(), None).unwrap();
    fs::write(&edit_path, store.to_bytes()).unwrap();
    let before = shot_ids(&edit_path);

    // A socket file that exists but is NOT a live daemon (connecting fails).
    let ar = tmp.path().join(".ar-edit");
    fs::create_dir_all(&ar).unwrap();
    fs::write(ar.join("session.sock"), b"not a socket").unwrap();

    // A mutation must fail closed (non-zero) and leave the file untouched.
    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args(["edit", "remove-segment", "rc", "--shot", &drop_id])
        .assert()
        .failure();
    assert_eq!(shot_ids(&edit_path), before, "file untouched after fail-closed mutation");

    // ...and undo too.
    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .args(["undo", "rc"])
        .assert()
        .failure();
}

/// Fail-closed on an UNRESPONSIVE host: a process that accepts the socket but
/// never replies must not hang the command forever — the round-trip times out
/// and the mutation aborts (file untouched). Uses a stalling listener + a short
/// AR_EDIT_DAEMON_TIMEOUT_MS so the test stays fast.
#[test]
fn one_shot_mutation_fails_closed_on_unresponsive_daemon() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("edits")).unwrap();
    let edit_path = tmp.path().join("edits/rc.edit.json");
    let mut store = PersistentEdit::create("rc", ActorId(1));
    let drop_id = store.add_shot("src-001", words(), None).unwrap();
    fs::write(&edit_path, store.to_bytes()).unwrap();
    let before = shot_ids(&edit_path);

    // A listener that accepts a connection and then stalls (never responds).
    let ar = tmp.path().join(".ar-edit");
    fs::create_dir_all(&ar).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(ar.join("session.sock")).unwrap();
    let _stall = std::thread::spawn(move || {
        if let Ok((conn, _)) = listener.accept() {
            std::thread::sleep(Duration::from_secs(3)); // outlive the client timeout
            drop(conn);
        }
    });

    // The probe connects but the Status round-trip never completes → timeout →
    // Failed → the mutation aborts without touching the file.
    Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp.path())
        .env("AR_EDIT_DAEMON_TIMEOUT_MS", "300")
        .args(["edit", "remove-segment", "rc", "--shot", &drop_id])
        .assert()
        .failure();
    assert_eq!(shot_ids(&edit_path), before, "file untouched when the host stalled");
}
