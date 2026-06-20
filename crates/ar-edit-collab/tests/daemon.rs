//! Session daemon / IPC tests (SPEC-003 REQ-089/090, CON-018; task s9).
//!
//! The daemon owns the canonical [`PersistentEdit`] store (ADR-011): mutations
//! apply to it and are persisted back to the edit file, undo/redo use its
//! durable cursor, and there is no op-id/gating protocol (one store, nothing to
//! reconcile). Hermetic: Unix socket + edit file in the temp dir, tokio only.
#![cfg(all(feature = "daemon", unix))]

use ar_edit_collab::ids::ActorId;
use ar_edit_collab::store::PersistentEdit;
use ar_edit_collab::shell::daemon::{Daemon, DaemonClient, DaemonError, Request, Response};
use ar_edit_core::models::{Shot, ShotRange};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn shot() -> Shot {
    Shot {
        id: String::new(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 10 },
        notes: vec![],
    }
}

fn paths(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    (
        dir.join(format!("ar-edit-d-{tag}-{pid}.sock")),
        dir.join(format!("ar-edit-d-{tag}-{pid}.edit.json")),
    )
}

/// Bind a daemon for a fresh empty edit, returning the running socket path and
/// the edit-file path.
fn bind(tag: &str, actor: u64) -> (std::path::PathBuf, std::path::PathBuf, Daemon) {
    let (sock, edit) = paths(tag);
    let _ = std::fs::remove_file(&edit);
    let store = PersistentEdit::create("edit", ActorId(actor));
    let daemon = Daemon::bind(&sock, &edit, store).unwrap();
    (sock, edit, daemon)
}

async fn add(c: &mut DaemonClient) -> String {
    match c.request(&Request::AddShot { shot: shot() }).await.unwrap() {
        Response::Added { shot_id } => shot_id,
        other => panic!("expected added, got {other:?}"),
    }
}

async fn snapshot_ids(c: &mut DaemonClient) -> Vec<String> {
    match c.request(&Request::Snapshot).await.unwrap() {
        Response::Snapshot { shots } => shots.into_iter().map(|s| s.id).collect(),
        other => panic!("expected snapshot, got {other:?}"),
    }
}

/// The materialised shot ids in the on-disk edit file (what a restart would see).
fn file_ids(edit: &std::path::Path) -> Vec<String> {
    let bytes = std::fs::read(edit).unwrap();
    PersistentEdit::from_bytes(&bytes, ActorId(1))
        .unwrap()
        .snapshot()
        .shots
        .into_iter()
        .map(|s| s.id)
        .collect()
}

/// REQ-089: the daemon owns one live edit; a mutation from one client is visible
/// to a separate client AND persisted to the canonical edit file.
#[tokio::test]
async fn daemon_holds_live_state_and_persists() {
    let (sock, edit, daemon) = bind("live", 1);
    tokio::spawn(daemon.run());

    let minted = {
        let mut a = DaemonClient::connect(&sock).await.unwrap();
        add(&mut a).await
    };
    assert!(minted.starts_with("shot-"));

    // A separate client sees the live shot...
    let mut b = DaemonClient::connect(&sock).await.unwrap();
    assert_eq!(snapshot_ids(&mut b).await, vec![minted.clone()]);
    match b.request(&Request::Status).await.unwrap() {
        Response::Status { shot_count, edit } => {
            assert_eq!(shot_count, 1);
            assert_eq!(edit, "edit");
        }
        other => panic!("expected status, got {other:?}"),
    }
    // ...and it was persisted to the canonical file.
    assert_eq!(file_ids(&edit), vec![minted]);
}

/// REQ-086: undo/redo go through the store's durable cursor and are persisted —
/// the live edit and the file stay in lock-step (single store).
#[tokio::test]
async fn daemon_undo_redo_is_durable() {
    let (sock, edit, daemon) = bind("undo", 2);
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&sock).await.unwrap();

    let a = add(&mut c).await;
    let _b = add(&mut c).await;
    assert_eq!(snapshot_ids(&mut c).await.len(), 2);

    assert!(matches!(
        c.request(&Request::Undo).await.unwrap(),
        Response::Reverted { reverted: true }
    ));
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone()]);
    assert_eq!(file_ids(&edit), vec![a], "undo persisted to the file");

    assert!(matches!(
        c.request(&Request::Redo).await.unwrap(),
        Response::Reverted { reverted: true }
    ));
    assert_eq!(snapshot_ids(&mut c).await.len(), 2);

    // Nothing left to undo past the start.
    assert!(matches!(
        c.request(&Request::Undo).await.unwrap(),
        Response::Reverted { .. }
    ));
}

/// CON-018: a direct IPC client cannot persist an invalid range — AddShot and
/// TrimShot validate and return an error before mutating.
#[tokio::test]
async fn daemon_rejects_invalid_range() {
    let (sock, _edit, daemon) = bind("badrange", 3);
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&sock).await.unwrap();

    let mut zero = shot();
    zero.range = ShotRange::Words { from: 5, to: 5 };
    assert!(matches!(
        c.request(&Request::AddShot { shot: zero }).await.unwrap(),
        Response::Error { .. }
    ));
    assert_eq!(snapshot_ids(&mut c).await.len(), 0);

    let id = add(&mut c).await;
    assert!(matches!(
        c.request(&Request::TrimShot { shot_id: id, range: ShotRange::Words { from: 10, to: 3 } })
            .await
            .unwrap(),
        Response::Error { .. }
    ));
    assert_eq!(snapshot_ids(&mut c).await.len(), 1);
}

/// A restart reloads the canonical file: live state AND the undo cursor survive,
/// so undo still works after the daemon is restarted (the cursor is in the file).
#[tokio::test]
async fn daemon_restart_reloads_store_and_cursor() {
    let (sock1, edit) = paths("restart");
    let (sock2, _) = paths("restart-b"); // a fresh socket for the restarted daemon
    let _ = std::fs::remove_file(&edit);

    // First run: two adds.
    {
        let store = PersistentEdit::create("edit", ActorId(4));
        let daemon = Daemon::bind(&sock1, &edit, store).unwrap();
        let h = tokio::spawn(daemon.run());
        let mut c = DaemonClient::connect(&sock1).await.unwrap();
        add(&mut c).await;
        add(&mut c).await;
        assert_eq!(snapshot_ids(&mut c).await.len(), 2);
        h.abort();
    }

    // Restart: reload the store from the canonical file, re-bind on a fresh
    // socket (the previous daemon's socket may still be cleaning up).
    let store = PersistentEdit::from_bytes(&std::fs::read(&edit).unwrap(), ActorId(4)).unwrap();
    let daemon = Daemon::bind(&sock2, &edit, store).unwrap();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&sock2).await.unwrap();

    assert_eq!(snapshot_ids(&mut c).await.len(), 2, "state survived restart");
    // The durable cursor survived too: undo still works.
    assert!(matches!(
        c.request(&Request::Undo).await.unwrap(),
        Response::Reverted { reverted: true }
    ));
    assert_eq!(snapshot_ids(&mut c).await.len(), 1);
    let _ = std::fs::remove_file(&edit);
}

/// Regression: a second daemon on a *live* socket refuses rather than unlinking
/// the running daemon's socket (which would split the session).
#[tokio::test]
async fn daemon_refuses_when_already_running() {
    let (sock, _edit, d1) = bind("running", 5);
    let path = d1.socket_path().to_path_buf();
    tokio::spawn(d1.run());

    let (_s2, e2) = paths("running2");
    let second = Daemon::bind(&path, &e2, PersistentEdit::create("edit", ActorId(6)));
    assert!(matches!(second, Err(DaemonError::AlreadyRunning)));

    let mut c = DaemonClient::connect(&path).await.unwrap();
    assert!(matches!(
        c.request(&Request::Status).await.unwrap(),
        Response::Status { .. }
    ));
    let _ = std::fs::remove_file(&sock);
}

/// CON-018 / TEST-120: a malformed request frame is rejected with an `error` and
/// performs no mutation (LangSec, fail-closed).
#[tokio::test]
async fn daemon_rejects_malformed_request() {
    let (sock, _edit, daemon) = bind("bad", 7);
    tokio::spawn(daemon.run());

    let mut raw = tokio::net::UnixStream::connect(&sock).await.unwrap();
    let bad = b"{not valid json";
    raw.write_all(&(bad.len() as u32).to_be_bytes()).await.unwrap();
    raw.write_all(bad).await.unwrap();
    let mut len = [0u8; 4];
    raw.read_exact(&mut len).await.unwrap();
    let n = u32::from_be_bytes(len) as usize;
    let mut body = vec![0u8; n];
    raw.read_exact(&mut body).await.unwrap();
    let resp: Response = serde_json::from_slice(&body).unwrap();
    assert!(matches!(resp, Response::Error { .. }));

    let mut c = DaemonClient::connect(&sock).await.unwrap();
    assert_eq!(snapshot_ids(&mut c).await.len(), 0, "no mutation on bad input");
}
