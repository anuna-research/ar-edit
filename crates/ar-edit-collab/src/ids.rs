//! Actor / site identifiers (SPEC-003 REQ-072).
//!
//! An actor id is derived from a peer's node public key and used as the Loro
//! site identifier for every change that peer originates. The derived order is
//! **total and deterministic** (it breaks ties between otherwise-concurrent
//! changes), but it carries no happens-before meaning.

/// CRDT actor/site identifier for a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId(pub u64);

impl ActorId {
    /// Derive a stable actor id from a node public key (e.g. an iroh/Ed25519
    /// public key). Uses BLAKE3 of the key; masks the high bit so the value is
    /// a valid Loro peer id.
    pub fn from_public_key(key: &[u8]) -> Self {
        let digest = blake3::hash(key);
        let mut n = [0u8; 8];
        n.copy_from_slice(&digest.as_bytes()[..8]);
        ActorId(u64::from_be_bytes(n) & 0x7fff_ffff_ffff_ffff)
    }

    /// Generate a fresh, random actor id (a valid Loro peer id). A session
    /// daemon with no node key derives its site identity this way and persists
    /// it, so two daemons never share an actor — sharing one would make their
    /// Loro operation ids collide and the documents fail to converge (REQ-072,
    /// REQ-080). The high bit is masked (Loro peer-id requirement) and zero is
    /// avoided so the value is always a usable, distinct site id.
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut n = rand::rngs::OsRng.next_u64() & 0x7fff_ffff_ffff_ffff;
        if n == 0 {
            n = 1;
        }
        ActorId(n)
    }
}

/// Deterministic total order over actor ids, for concurrent-change tie-breaks
/// (REQ-072). A thin wrapper over `Ord` to make the intent explicit at call
/// sites.
pub fn total_order(a: ActorId, b: ActorId) -> std::cmp::Ordering {
    a.cmp(&b)
}
