//! Peer presence / awareness (SPEC-003 REQ-085; task s4).
//!
//! Presence is **ephemeral**: it is carried in CON-015 `PRESENCE` frames and
//! kept in this in-memory table, deliberately *separate* from the [`CollabDoc`]
//! CRDT so it can never be persisted into the edit document. A peer's entry
//! disappears when it disconnects.
//!
//! The table logic is pure (no I/O); the transport feeds it `PRESENCE` frames
//! and removes peers on disconnect.

use crate::ids::ActorId;
use std::collections::BTreeMap;

/// One peer's live presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPresence {
    pub actor: ActorId,
    pub name: String,
    /// The shot this peer currently has selected (cursor presence), if any.
    pub selected_shot: Option<String>,
}

/// The set of currently-connected peers and what each is looking at.
/// Keyed/ordered by actor id for deterministic display.
#[derive(Debug, Default)]
pub struct PresenceTable {
    peers: BTreeMap<ActorId, PeerPresence>,
}

impl PresenceTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a presence update (from a `PRESENCE` frame). Inserts or replaces.
    pub fn update(&mut self, p: PeerPresence) {
        self.peers.insert(p.actor, p);
    }

    /// Drop a peer's presence (on disconnect). Returns whether it was present.
    pub fn remove(&mut self, actor: ActorId) -> bool {
        self.peers.remove(&actor).is_some()
    }

    /// Currently-connected peers, in deterministic order.
    pub fn peers(&self) -> impl Iterator<Item = &PeerPresence> {
        self.peers.values()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Who currently has `shot_id` selected (for cursor-presence display).
    pub fn selectors_of<'a>(&'a self, shot_id: &'a str) -> impl Iterator<Item = &'a PeerPresence> {
        self.peers
            .values()
            .filter(move |p| p.selected_shot.as_deref() == Some(shot_id))
    }
}
