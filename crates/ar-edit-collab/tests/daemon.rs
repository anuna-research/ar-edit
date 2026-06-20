//! Session daemon / IPC tests (SPEC-003 REQ-089/090, CON-018; task s9).
//! Hermetic: Unix socket in the temp dir, tokio only (no iroh). Empty without
//! the `daemon` feature.
#![cfg(all(feature = "daemon", unix))]

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::materialise::materialise;
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

/// Helper: the live shot ids, in order, as seen by a Snapshot request.
async fn snapshot_ids(c: &mut DaemonClient) -> Vec<String> {
    match c.request(&Request::Snapshot).await.unwrap() {
        Response::Snapshot { shots } => shots.into_iter().map(|s| s.id).collect(),
        other => panic!("expected snapshot, got {other:?}"),
    }
}

async fn add(c: &mut DaemonClient) -> String {
    match c.request(&Request::AddShot { shot: shot("ignored") }).await.unwrap() {
        Response::Added { shot_id } => shot_id,
        other => panic!("expected added, got {other:?}"),
    }
}

/// Regression (P1, REQ-086): undo/redo go through the daemon's CRDT-aware
/// LocalUndo, reverting only this actor's most recent change.
#[tokio::test]
async fn daemon_undo_redo_reverts_last_local_change() {
    let daemon = Daemon::bind(sock("undo"), CollabDoc::new(ActorId(5))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    let a = add(&mut c).await;
    let _b = add(&mut c).await;
    assert_eq!(snapshot_ids(&mut c).await.len(), 2);

    assert!(matches!(c.request(&Request::Undo).await.unwrap(), Response::Ok));
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone()], "undo reverts only the last add");

    assert!(matches!(c.request(&Request::Redo).await.unwrap(), Response::Ok));
    assert_eq!(snapshot_ids(&mut c).await.len(), 2, "redo reinstates it");
}

/// Regression (P1): undoing the removal of a NON-final shot restores it at its
/// original position (the CRDT-aware undo reinserts in place — no reordering).
#[tokio::test]
async fn daemon_undo_of_remove_restores_position() {
    let daemon = Daemon::bind(sock("undopos"), CollabDoc::new(ActorId(6))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    let a = add(&mut c).await;
    let b = add(&mut c).await;
    let d = add(&mut c).await;
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone(), b.clone(), d.clone()]);

    // Remove the MIDDLE shot, then undo.
    assert!(matches!(
        c.request(&Request::RemoveShot { shot_id: b.clone() }).await.unwrap(),
        Response::Ok
    ));
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone(), d.clone()]);

    assert!(matches!(c.request(&Request::Undo).await.unwrap(), Response::Ok));
    assert_eq!(
        snapshot_ids(&mut c).await,
        vec![a, b, d],
        "undo of a middle removal restores the shot at its original position"
    );
}

/// Regression (P2, CON-018): a direct IPC client cannot persist an invalid range
/// — both AddShot and TrimShot validate before mutating and return an error.
#[tokio::test]
async fn daemon_rejects_invalid_range() {
    let daemon = Daemon::bind(sock("badrange"), CollabDoc::new(ActorId(8))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    // Zero-length range on add → rejected, no mutation.
    let zero = Shot {
        id: String::new(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 5, to: 5 },
        notes: vec![],
    };
    assert!(
        matches!(c.request(&Request::AddShot { shot: zero }).await.unwrap(), Response::Error { .. }),
        "zero-length add must be rejected"
    );
    assert_eq!(snapshot_ids(&mut c).await.len(), 0, "no shot was added");

    // Inverted range on trim of a valid shot → rejected, range unchanged.
    let id = add(&mut c).await;
    assert!(
        matches!(
            c.request(&Request::TrimShot { shot_id: id, range: ShotRange::Words { from: 10, to: 3 } })
                .await
                .unwrap(),
            Response::Error { .. }
        ),
        "inverted trim must be rejected"
    );
    assert_eq!(snapshot_ids(&mut c).await.len(), 1, "the shot still exists");
}

/// Regression (P2, REQ-080/084): a remote delta applied via the persisting
/// import path is durable even with no following local IPC mutation.
#[tokio::test]
async fn daemon_import_remote_persists() {
    let snap = std::env::temp_dir().join(format!("ar-edit-import-{}.loro", std::process::id()));
    let _ = std::fs::remove_file(&snap);

    let daemon =
        Daemon::bind_persisting(sock("import"), CollabDoc::new(ActorId(9)), &snap).unwrap();

    // A peer produces a delta containing a new shot.
    let peer = CollabDoc::new(ActorId(99));
    peer.add_shot(&shot("remote-001"));
    daemon.import_remote(&peer.export_snapshot()).unwrap();

    // The snapshot on disk already reflects the remote edit — a restart that
    // reloads it would keep the shot (no local mutation was needed to flush it).
    let reloaded = CollabDoc::new(ActorId(9));
    reloaded.import(&std::fs::read(&snap).unwrap()).unwrap();
    let ids: Vec<String> = materialise(&reloaded).shots.into_iter().map(|s| s.id).collect();
    assert!(
        ids.contains(&"remote-001".to_string()),
        "remote import must be persisted without a following IPC mutation: {ids:?}"
    );
    let _ = std::fs::remove_file(&snap);
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
