//! Presence tests (SPEC-003 REQ-085; task s4).

use ar_edit_collab::ids::ActorId;
use ar_edit_collab::presence::{PeerPresence, PresenceTable};

fn peer(id: u64, name: &str, shot: Option<&str>) -> PeerPresence {
    PeerPresence {
        actor: ActorId(id),
        name: name.into(),
        selected_shot: shot.map(|s| s.into()),
    }
}

#[test]
fn presence_tracks_peers_and_cursors() {
    let mut t = PresenceTable::new();
    t.update(peer(10, "alice", Some("shot-001")));
    t.update(peer(20, "bob", Some("shot-003")));
    assert_eq!(t.len(), 2);

    // Cursor presence: who is on shot-001.
    let on_1: Vec<_> = t.selectors_of("shot-001").map(|p| p.name.clone()).collect();
    assert_eq!(on_1, vec!["alice".to_string()]);

    // Update alice's selection.
    t.update(peer(10, "alice", Some("shot-003")));
    let on_3: Vec<_> = t.selectors_of("shot-003").map(|p| p.actor.0).collect();
    assert_eq!(on_3, vec![10, 20]);
}

#[test]
fn presence_disappears_on_disconnect() {
    let mut t = PresenceTable::new();
    t.update(peer(10, "alice", None));
    t.update(peer(20, "bob", None));
    assert!(t.remove(ActorId(10)));
    assert_eq!(t.len(), 1);
    assert!(!t.remove(ActorId(10))); // already gone
    assert_eq!(t.peers().next().unwrap().name, "bob");
}
