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
        let r = a
            .request(&Request::AddShot { op_id: "op-1".into(), shot: shot("shot-001") })
            .await
            .unwrap();
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
        let minted = match c
            .request(&Request::AddShot { op_id: "op-p1".into(), shot: shot("ignored") })
            .await
            .unwrap()
        {
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
    let minted2 = match c2
        .request(&Request::AddShot { op_id: "op-p2".into(), shot: shot("ignored") })
        .await
        .unwrap()
    {
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

/// Add a shot recorded under the unique `op_id`; returns the minted shot id.
async fn add(c: &mut DaemonClient, op_id: &str) -> String {
    match c
        .request(&Request::AddShot { op_id: op_id.into(), shot: shot("ignored") })
        .await
        .unwrap()
    {
        Response::Added { shot_id } => shot_id,
        other => panic!("expected added, got {other:?}"),
    }
}

/// Send a guarded undo for the operation `op_id`; returns whether it reverted.
async fn undo_op(c: &mut DaemonClient, op_id: &str) -> bool {
    match c.request(&Request::Undo { tag: op_id.into() }).await.unwrap() {
        Response::Reverted { reverted } => reverted,
        other => panic!("expected reverted, got {other:?}"),
    }
}

async fn redo_op(c: &mut DaemonClient, op_id: &str) -> bool {
    match c.request(&Request::Redo { tag: op_id.into() }).await.unwrap() {
        Response::Reverted { reverted } => reverted,
        other => panic!("expected reverted, got {other:?}"),
    }
}

/// Regression (P1, REQ-086): a guarded undo/redo reverts only the matching op,
/// and an op id that is NOT on top of the stack is refused (no mutation) so the
/// daemon never pops an unrelated operation (REQ-090).
#[tokio::test]
async fn daemon_guarded_undo_redo() {
    let daemon = Daemon::bind(sock("undo"), CollabDoc::new(ActorId(5))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    let a = add(&mut c, "op-a").await;
    let _b = add(&mut c, "op-b").await;
    assert_eq!(snapshot_ids(&mut c).await.len(), 2);

    // An op id that is not on top (the older add `op-a`) must be refused.
    assert!(!undo_op(&mut c, "op-a").await, "non-top op id must not undo");
    assert_eq!(snapshot_ids(&mut c).await.len(), 2, "refused undo changed nothing");

    // The matching top op id (`op-b`) reverts exactly that op.
    assert!(undo_op(&mut c, "op-b").await, "matching op id reverts");
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone()]);

    // Redo of the same op reinstates it.
    assert!(redo_op(&mut c, "op-b").await, "matching op id redoes");
    assert_eq!(snapshot_ids(&mut c).await.len(), 2);
}

/// Regression (P1): two operations of the SAME kind on the SAME shot (e.g. two
/// trims) carry DISTINCT op ids, so undoing the durable (deeper) one is refused
/// while an unrelated trim is on top — a "<kind>:<shot>" tag would wrongly match
/// the top and revert the unrelated op, recreating the divergence (REQ-090).
#[tokio::test]
async fn daemon_distinct_op_ids_prevent_wrong_undo() {
    let daemon = Daemon::bind(sock("collide"), CollabDoc::new(ActorId(15))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    let s = add(&mut c, "op-add").await;
    // The caller's trim, then an unrelated client's trim of the SAME shot.
    let trim = |op: &str, to: u32| Request::TrimShot {
        op_id: op.into(),
        shot_id: s.clone(),
        range: ShotRange::Words { from: 0, to },
    };
    assert!(matches!(c.request(&trim("op-trim-1", 5)).await.unwrap(), Response::Ok));
    assert!(matches!(c.request(&trim("op-trim-2", 7)).await.unwrap(), Response::Ok));

    // Undoing op-trim-1 (NOT on top) is refused — op-trim-2 is not touched.
    assert!(!undo_op(&mut c, "op-trim-1").await, "deeper trim must not undo out of order");
    // The top op id reverts correctly.
    assert!(undo_op(&mut c, "op-trim-2").await, "top trim reverts");
}

/// Regression (P1): undoing the removal of a NON-final shot restores it at its
/// original position (the CRDT-aware undo reinserts in place — no reordering).
#[tokio::test]
async fn daemon_undo_of_remove_restores_position() {
    let daemon = Daemon::bind(sock("undopos"), CollabDoc::new(ActorId(6))).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    let a = add(&mut c, "op-a").await;
    let b = add(&mut c, "op-b").await;
    let d = add(&mut c, "op-d").await;
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone(), b.clone(), d.clone()]);

    // Remove the MIDDLE shot, then undo it by op id.
    assert!(matches!(
        c.request(&Request::RemoveShot { op_id: "op-rm-b".into(), shot_id: b.clone() })
            .await
            .unwrap(),
        Response::Ok
    ));
    assert_eq!(snapshot_ids(&mut c).await, vec![a.clone(), d.clone()]);

    assert!(undo_op(&mut c, "op-rm-b").await);
    assert_eq!(
        snapshot_ids(&mut c).await,
        vec![a, b, d],
        "undo of a middle removal restores the shot at its original position"
    );
}

/// Regression (P1, REQ-086): after a restart the undo history is empty, so a
/// guarded undo reports `reverted: false` instead of silently doing nothing
/// while claiming success — the CLI relies on this to avoid committing one side.
#[tokio::test]
async fn daemon_undo_after_restart_reports_no_revert() {
    let snap = std::env::temp_dir().join(format!("ar-edit-undorestart-{}.loro", std::process::id()));
    let _ = std::fs::remove_file(&snap);

    let added = {
        let daemon = Daemon::bind_persisting(sock("ur1"), CollabDoc::new(ActorId(12)), &snap).unwrap();
        let path = daemon.socket_path().to_path_buf();
        let h = tokio::spawn(daemon.run());
        let mut c = DaemonClient::connect(&path).await.unwrap();
        let _id = add(&mut c, "op-x").await;
        h.abort();
        "op-x".to_string()
    };

    // Restart: history is empty, even though the shot is restored.
    let doc = CollabDoc::new(ActorId(12));
    doc.import(&std::fs::read(&snap).unwrap()).unwrap();
    let daemon = Daemon::bind_persisting(sock("ur2"), doc, &snap).unwrap();
    let path = daemon.socket_path().to_path_buf();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    assert!(
        !undo_op(&mut c, &added).await,
        "an undo with no live history must report reverted: false"
    );
    let _ = std::fs::remove_file(&snap);
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
        matches!(
            c.request(&Request::AddShot { op_id: "op-zero".into(), shot: zero }).await.unwrap(),
            Response::Error { .. }
        ),
        "zero-length add must be rejected"
    );
    assert_eq!(snapshot_ids(&mut c).await.len(), 0, "no shot was added");

    // Inverted range on trim of a valid shot → rejected, range unchanged.
    let id = add(&mut c, "op-valid").await;
    assert!(
        matches!(
            c.request(&Request::TrimShot {
                op_id: "op-bad-trim".into(),
                shot_id: id,
                range: ShotRange::Words { from: 10, to: 3 }
            })
            .await
            .unwrap(),
            Response::Error { .. }
        ),
        "inverted trim must be rejected"
    );
    assert_eq!(snapshot_ids(&mut c).await.len(), 1, "the shot still exists");
}

/// Regression (P1, REQ-080/084): the cloneable importer is usable *while the
/// daemon's IPC loop runs* (run(self) consumes the daemon), applies into the
/// same live doc an IPC client sees, and persists — so deltas received during
/// operation are not lost on restart.
#[tokio::test]
async fn daemon_importer_services_live_sync() {
    let snap = std::env::temp_dir().join(format!("ar-edit-import-{}.loro", std::process::id()));
    let _ = std::fs::remove_file(&snap);

    let daemon =
        Daemon::bind_persisting(sock("import"), CollabDoc::new(ActorId(9)), &snap).unwrap();
    let path = daemon.socket_path().to_path_buf();
    // Obtain the importer BEFORE run() takes ownership, then start the IPC loop.
    let importer = daemon.importer();
    tokio::spawn(daemon.run());
    let mut c = DaemonClient::connect(&path).await.unwrap();

    // A peer's delta is applied through the importer while IPC is live.
    let peer = CollabDoc::new(ActorId(99));
    peer.add_shot(&shot("remote-001"));
    importer.import(&peer.export_snapshot()).unwrap();

    // The running daemon's own IPC clients see the merged remote shot...
    assert!(
        snapshot_ids(&mut c).await.contains(&"remote-001".to_string()),
        "importer must apply into the same live document IPC serves"
    );
    // ...and it was persisted, so a restart would keep it (no IPC mutation followed).
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
