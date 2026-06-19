//! Session daemon / IPC tests (SPEC-003 REQ-089/090, CON-018; task s9).
//! Hermetic: Unix socket in the temp dir, tokio only (no iroh). Empty without
//! the `daemon` feature.
#![cfg(all(feature = "daemon", unix))]

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::shell::daemon::{Daemon, DaemonClient, Request, Response};
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
        assert!(matches!(r, Response::Ok));
    }

    // Client B — a fresh connection — sees A's shot in the live session.
    let mut b = DaemonClient::connect(&path).await.unwrap();
    match b.request(&Request::Snapshot).await.unwrap() {
        Response::Snapshot { shots } => {
            assert!(
                shots.iter().any(|s| s.id == "shot-001"),
                "second client must see the live state: {shots:?}"
            );
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
    match b.request(&Request::Status).await.unwrap() {
        Response::Status { shot_count } => assert_eq!(shot_count, 1),
        other => panic!("expected status, got {other:?}"),
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
