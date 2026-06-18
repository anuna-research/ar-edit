//! Effectful shell (SPEC-003 Phase S) — interfaces only.
//!
//! Concrete impls (iroh transport, rendezvous + SPAKE2, iroh-blobs sync,
//! presence) are gated on SPEC-003 OQ-7 (the iroh build tree + a 2 GB media
//! transfer exceed this host's disk). The traits below pin the boundary the
//! pure core is built against, so the shell can be dropped in without touching
//! core logic.

#[cfg(feature = "rendezvous")]
pub mod rendezvous;

#[cfg(feature = "transport")]
pub mod discovery;

#[cfg(feature = "transport")]
pub mod transport;

use crate::crdt::CollabDoc;

/// Transport for CRDT deltas between peers (SPEC-003 REQ-084). Impl in Phase S
/// over iroh; the pure core only ever sees encoded delta bytes.
pub trait DeltaTransport {
    type Error;
    /// Broadcast a locally-produced CRDT delta to all connected peers.
    fn broadcast(&self, delta: &[u8]) -> Result<(), Self::Error>;
    /// Apply a delta received from a peer into the local document.
    fn apply_remote(&self, doc: &mut CollabDoc, delta: &[u8]) -> Result<(), Self::Error>;
}

/// Content-addressed media replication (SPEC-003 REQ-075/076). Impl in Phase S
/// over iroh-blobs.
pub trait BlobSync {
    type Error;
    /// Fetch a blob by its BLAKE3 hash, verifying integrity before return.
    fn fetch(&self, hash: &[u8; 32]) -> Result<Vec<u8>, Self::Error>;
}
