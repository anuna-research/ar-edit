//! Full serverless session capstone (SPEC-003): host + join + sync.
//! Discover by phrase (pkarr/DHT) → dial over iroh → SPAKE2 → CRDT sync.
//! Hermetic (in-process discovery backend). Empty without `transport`.
#![cfg(feature = "transport")]

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::materialise::materialise;
use ar_edit_collab::recognise::phrase;
use ar_edit_collab::shell::discovery::Discovery;
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

/// The whole vertical: the host opens a session under a phrase, a joiner finds
/// it by that phrase alone, pairs (SPAKE2 over iroh), and a CRDT edit syncs —
/// no rendezvous server. Both peers agree on the session key.
#[tokio::test]
async fn host_join_and_sync_full_session() {
    let store = Arc::new(Mutex::new(HashMap::new()));
    let phrase = phrase::generate_secure();

    let host_t = Transport::bind_loopback().await.unwrap();
    let join_t = Transport::bind_loopback().await.unwrap();
    let disc_host = Discovery::in_process(store.clone());
    let disc_join = Discovery::in_process(store.clone());
    let host_actor = host_t.actor();

    let p = phrase.clone();
    let host_task = tokio::spawn(async move {
        let session = host_t.host_session(&disc_host, &p).await.unwrap();
        let doc = CollabDoc::new(host_actor);
        session.pull_delta_into(&doc).await.unwrap();
        (session, doc)
    });

    // Joiner discovers the host by the phrase, pairs, and pushes a change.
    let session_j = join_t.join_session(&disc_join, &phrase).await.unwrap();
    let jdoc = CollabDoc::new(join_t.actor());
    jdoc.add_shot(&shot("shot-001"));
    session_j.push_delta(&jdoc.export_snapshot()).await.unwrap();

    let (session_h, host_doc) = host_task.await.unwrap();

    // The edit synced over the paired connection.
    assert!(
        materialise(&host_doc)
            .shots
            .iter()
            .any(|s| s.id == "shot-001"),
        "edit must sync over the paired session"
    );
    // Both peers agreed on the same SPAKE2 session key.
    assert_eq!(
        session_j.key().bytes(),
        session_h.key().bytes(),
        "session keys must match"
    );
}
