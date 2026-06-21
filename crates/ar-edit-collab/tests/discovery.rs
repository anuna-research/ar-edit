//! Serverless pkarr/DHT discovery tests (SPEC-003 ADR-013, CON-017,
//! REQ-068/069/071; task s8). Hermetic: the in-process backend stands in for
//! the Mainline DHT, exercising the real key derivation + signed-record
//! build/parse. Empty without the `transport` feature.
#![cfg(feature = "transport")]

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::materialise::materialise;
use ar_edit_collab::recognise::phrase;
use ar_edit_collab::shell::discovery::{self, Discovery};
use ar_edit_collab::shell::transport::Transport;
use ar_edit_core::models::{Shot, ShotRange};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn shot(id: &str) -> Shot {
    Shot {
        author: String::new(),
        id: id.into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 10 },
        notes: vec![],
    }
}

fn variant(p: &phrase::Phrase) -> phrase::Phrase {
    phrase::parse(&format!(
        "{}-{}-{}",
        (p.channel + 1) % 1000,
        p.words[0],
        p.words[1]
    ))
    .unwrap()
}

/// ADR-013: the discovery keypair is a deterministic function of the phrase —
/// both peers derive the same key; a different phrase gives a different key.
#[test]
fn keypair_derivation_is_deterministic() {
    let p = phrase::generate_secure();
    let k1 = discovery::derive_keypair(&p).public_key().to_bytes();
    let k2 = discovery::derive_keypair(&p).public_key().to_bytes();
    assert_eq!(k1, k2, "same phrase must derive the same discovery key");
    let kx = discovery::derive_keypair(&variant(&p))
        .public_key()
        .to_bytes();
    assert_ne!(kx, k1, "different phrase must derive a different key");
}

/// CON-017: a built record parses back to the same dialable address.
#[tokio::test]
async fn record_roundtrips_to_dialable_addr() {
    let host = Transport::bind_loopback().await.unwrap();
    let addr = host.dial_addr().unwrap();
    let kp = discovery::derive_keypair(&phrase::generate_secure());
    let packet = discovery::build_record(&kp, &addr).unwrap();
    let parsed = discovery::parse_record(&packet).expect("record must parse");

    assert_eq!(parsed.id, addr.id, "NodeId must survive the record");
    let want: Vec<String> = addr.ip_addrs().map(|s| s.to_string()).collect();
    let got: Vec<String> = parsed.ip_addrs().map(|s| s.to_string()).collect();
    assert_eq!(got, want, "dialling hints must survive the record");
    host.close().await;
}

/// REQ-068/069/071 end-to-end: the host publishes its address under the phrase,
/// a joiner discovers it *by the phrase alone*, dials directly, and a CRDT delta
/// propagates — no rendezvous server in the loop.
#[tokio::test]
async fn discover_then_propagate_delta() {
    let store = Arc::new(Mutex::new(HashMap::new()));
    let phrase = phrase::generate_secure();

    // Host: bind, publish its dial address under the phrase, accept one peer.
    let host = Transport::bind_loopback().await.unwrap();
    Discovery::in_process(store.clone())
        .publish(&phrase, &host.dial_addr().unwrap())
        .await
        .unwrap();
    let host_actor = host.actor();
    let recv = tokio::spawn(async move {
        let doc = CollabDoc::new(host_actor);
        host.accept_into(&doc).await.unwrap();
        doc
    });

    // Joiner: make a local change, discover the host by phrase, dial, send.
    let joiner = Transport::bind_loopback().await.unwrap();
    let jdoc = CollabDoc::new(joiner.actor());
    jdoc.add_shot(&shot("shot-001"));
    let delta = jdoc.export_snapshot();

    let found = Discovery::in_process(store.clone())
        .lookup(&phrase)
        .await
        .unwrap()
        .expect("host must be discoverable by the phrase alone");
    joiner.send_delta(found, &delta).await.unwrap();

    let host_doc = recv.await.unwrap();
    assert!(
        materialise(&host_doc)
            .shots
            .iter()
            .any(|s| s.id == "shot-001"),
        "delta must propagate after pkarr-based discovery"
    );
    joiner.close().await;
}
