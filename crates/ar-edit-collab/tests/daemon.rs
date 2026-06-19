//! Session daemon / IPC tests (SPEC-003 REQ-089/090, CON-018; task s9).
//! Hermetic: Unix socket in the temp dir, tokio only (no iroh). Empty without
//! the `daemon` feature.
#![cfg(all(feature = "daemon", unix))]

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::shell::daemon::{Daemon, DaemonClient, DaemonError, Request, Response};
use ar_edit_core::models::{Shot, ShotRange};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn shot(id: &str) -> Shot {
    Shot {
        id: id.into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 10 },
        notes: vec![],
    }
}

fn sock(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ar-edit-daemon-{}-{}.sock", tag, std::process::id()))
}

/// REQ-089 / TEST-119: the daemon holds one live document; a mutation from one
/// discrete client is visible to a *separate* client — the persistent process
/// maintains shared live state between one-shot invocations.
#[tokio::test]
async fn daemon_holds_live_state_across_clients() {
    let daemon = Daemon::bind(sock("live"), CollabDoc::new(ActorId(1))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());

    // Client A applies an edit, then disconnects (drops).
    {
        let mut a = DaemonClient::connect(&path).await.unwrap();
        let r = a.request(&Request::AddShot { shot: shot("shot-001") }).await.unwrap();
        // The daemon mints its own actor-scoped id (it never preserves the
        // client's sequential id, which would collide across peer daemons).
        let minted = match r {
            Response::Added { shot_id } => shot_id,
            other => panic!("expected added, got {other:?}"),
        };
        assert!(minted.starts_with("shot-"), "daemon minted an actor-scoped id: {minted}");
    }

    // Client B — a fresh connection — sees A's shot in the live session.
    let mut b = DaemonClient::connect(&path).await.unwrap();
    match b.request(&Request::Snapshot).await.unwrap() {
        Response::Snapshot { shots } => {
            assert_eq!(shots.len(), 1, "second client sees the one live shot: {shots:?}");
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
    match b.request(&Request::Status).await.unwrap() {
        Response::Status { shot_count } => assert_eq!(shot_count, 1),
        other => panic!("expected status, got {other:?}"),
    }
}

/// Regression (P2): a second daemon on a *live* socket refuses to start rather
/// than unlinking the running daemon's socket (which would split the session).
#[tokio::test]
async fn daemon_refuses_when_already_running() {
    let d1 = Daemon::bind(sock("running"), CollabDoc::new(ActorId(1))).unwrap();
    let path = d1.socket_path().to_path_buf();
    tokio::spawn(d1.run());

    // Second bind on the same live socket must refuse.
    let second = Daemon::bind(&path, CollabDoc::new(ActorId(2)));
    assert!(
        matches!(second, Err(DaemonError::AlreadyRunning)),
        "second daemon must refuse a live socket"
    );

    // The original daemon is still reachable.
    let mut c = DaemonClient::connect(&path).await.unwrap();
    assert!(matches!(
        c.request(&Request::Status).await.unwrap(),
        Response::Status { .. }
    ));
}

/// Regression (P1, REQ-080): a persisting daemon writes its live snapshot after
/// each mutation; a restart that reloads it restores both the content AND the
/// minted-id counters, so the next insert never re-mints an already-issued id.
#[tokio::test]
async fn daemon_persists_and_restores_counters_across_restart() {
    let snap =
        std::env::temp_dir().join(format!("ar-edit-snap-{}.loro", std::process::id()));
    let _ = std::fs::remove_file(&snap);
    let actor = ActorId(0x1234);

    // First run: one add, which persists a snapshot before the response returns.
    let minted1 = {
        let daemon = Daemon::bind_persisting(sock("persist1"), CollabDoc::new(actor), &snap).unwrap();
        let path = daemon.socket_path().to_path_buf();
        let handle = tokio::spawn(daemon.run());
        let mut c = DaemonClient::connect(&path).await.unwrap();
        let minted = match c.request(&Request::AddShot { shot: shot("ignored") }).await.unwrap() {
            Response::Added { shot_id } => shot_id,
            other => panic!("expected added, got {other:?}"),
        };
        handle.abort();
        minted
    };
    assert!(std::fs::metadata(&snap).is_ok(), "a snapshot was persisted");

    // Restart: load the snapshot into a fresh doc with the SAME persisted actor.
    let doc2 = CollabDoc::new(actor);
    doc2.import(&std::fs::read(&snap).unwrap()).unwrap();
    let daemon2 = Daemon::bind_persisting(sock("persist2"), doc2, &snap).unwrap();
    let path2 = daemon2.socket_path().to_path_buf();
    let handle2 = tokio::spawn(daemon2.run());
    let mut c2 = DaemonClient::connect(&path2).await.unwrap();

    match c2.request(&Request::Snapshot).await.unwrap() {
        Response::Snapshot { shots } => {
            assert_eq!(shots.len(), 1, "the shot survived the restart: {shots:?}");
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
    let minted2 = match c2.request(&Request::AddShot { shot: shot("ignored") }).await.unwrap() {
        Response::Added { shot_id } => shot_id,
        other => panic!("expected added, got {other:?}"),
    };
    assert_ne!(
        minted1, minted2,
        "the restored counter must not re-mint an already-issued id"
    );
    handle2.abort();
    let _ = std::fs::remove_file(&snap);
}

/// Regression (P1): `RestoreShot` re-inserts a shot preserving its id (the
/// undo-of-remove / redo-of-add mirror), unlike `AddShot` which always mints —
/// so the live document keeps referring to the same shot as the on-disk edit.
#[tokio::test]
async fn daemon_restore_shot_preserves_id() {
    let daemon = Daemon::bind(sock("restore"), CollabDoc::new(ActorId(7))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    let resp = c
        .request(&Request::RestoreShot { shot: shot("shot-keep-0001") })
        .await
        .unwrap();
    assert!(matches!(resp, Response::Ok), "restore acknowledged");

    match c.request(&Request::Snapshot).await.unwrap() {
        Response::Snapshot { shots } => {
            assert_eq!(shots.len(), 1);
            assert_eq!(shots[0].id, "shot-keep-0001", "RestoreShot preserves the id");
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
}

/// CON-018 / TEST-120: a malformed request frame is rejected with an `error`
/// and performs no mutation (LangSec, fail-closed).
#[tokio::test]
async fn daemon_rejects_malformed_request() {
    let daemon = Daemon::bind(sock("bad"), CollabDoc::new(ActorId(2))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());

    // Send a frame with invalid JSON directly.
    let mut raw = tokio::net::UnixStream::connect(&path).await.unwrap();
    let bad = b"{not valid json";
    raw.write_all(&(bad.len() as u32).to_be_bytes()).await.unwrap();
    raw.write_all(bad).await.unwrap();
    let mut len = [0u8; 4];
    raw.read_exact(&mut len).await.unwrap();
    let n = u32::from_be_bytes(len) as usize;
    let mut body = vec![0u8; n];
    raw.read_exact(&mut body).await.unwrap();
    let resp: Response = serde_json::from_slice(&body).unwrap();
    assert!(matches!(resp, Response::Error { .. }), "malformed frame must error");

    // The document was not mutated.
    let mut client = DaemonClient::connect(&path).await.unwrap();
    match client.request(&Request::Status).await.unwrap() {
        Response::Status { shot_count } => assert_eq!(shot_count, 0, "no mutation on bad input"),
        other => panic!("expected status, got {other:?}"),
    }
}
