//! iroh transport integration test (SPEC-003 REQ-084; task s1).
//! Hermetic: two relay-free endpoints on loopback. Empty without `transport`.
#![cfg(feature = "transport")]

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::materialise::materialise;
use ar_edit_collab::shell::transport::Transport;
use ar_edit_core::models::{Shot, ShotRange};

fn shot(id: &str) -> Shot {
    Shot {
        id: id.into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 10 },
        notes: vec![],
    }
}

/// A local edit on one peer propagates as a CRDT delta over a direct iroh
/// connection and applies to the remote peer's document (REQ-084).
#[tokio::test]
async fn delta_propagates_over_iroh() {
    let server = Transport::bind_loopback().await.expect("bind server");
    let client = Transport::bind_loopback().await.expect("bind client");
    let server_addr = server.dial_addr().expect("server addr");

    // Client makes a local change.
    let a = CollabDoc::new(client.actor());
    a.add_shot(&shot("shot-001"));
    let delta = a.export_snapshot();

    // Server's empty doc; accept in the background and apply the delta.
    let recv = tokio::spawn(async move {
        let b = CollabDoc::new(server.actor());
        server.accept_into(&b).await.expect("accept+apply");
        b
    });

    client
        .send_delta(server_addr, &delta)
        .await
        .expect("send delta");

    let b = recv.await.expect("join");
    let ids: Vec<String> = materialise(&b).shots.into_iter().map(|s| s.id).collect();
    assert!(
        ids.contains(&"shot-001".to_string()),
        "remote peer must have applied the delta: {ids:?}"
    );

    client.close().await;
}

/// REQ-075/076: a source blob transfers over iroh and is admitted only when its
/// BLAKE3 hash matches; a tampered blob is rejected fail-closed.
#[tokio::test]
async fn blob_transfer_with_integrity_gate() {
    use ar_edit_collab::reconcile::content_hash;

    // Happy path: matching hash admitted.
    {
        let server = Transport::bind_loopback().await.unwrap();
        let client = Transport::bind_loopback().await.unwrap();
        let addr = server.dial_addr().unwrap();
        let blob = b"pretend this is source-001.mp4 bytes".to_vec();
        let hash = content_hash(&blob);
        let recv = tokio::spawn(async move { server.fetch_blob(&hash).await });
        client.send_blob(addr, &blob).await.unwrap();
        assert_eq!(recv.await.unwrap().unwrap(), blob, "matching blob admitted");
        client.close().await;
    }

    // Tamper path: sender claims `hash` but sends different bytes → rejected.
    {
        let server = Transport::bind_loopback().await.unwrap();
        let client = Transport::bind_loopback().await.unwrap();
        let addr = server.dial_addr().unwrap();
        let claimed = content_hash(b"the original bytes");
        let recv = tokio::spawn(async move { server.fetch_blob(&claimed).await });
        client.send_blob(addr, b"TAMPERED bytes").await.unwrap();
        assert!(
            matches!(
                recv.await.unwrap(),
                Err(ar_edit_collab::shell::transport::TransportError::Integrity)
            ),
            "tampered blob must be rejected fail-closed"
        );
        client.close().await;
    }
}
