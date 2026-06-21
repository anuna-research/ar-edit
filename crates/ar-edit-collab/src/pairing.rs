//! SPAKE2 pairing handshake (SPEC-003 REQ-070, NFR-014; task s2).
//!
//! # ⚠️ NO-GO cryptographic area (ADR-009 / PROTO-001 AI Trust Boundaries)
//!
//! This module is **implemented and tested**, but it MUST receive
//! audited-implementation + human-domain-expert + cross-model review before it
//! is accepted for production use. Open items flagged for that review:
//! - constant-time comparison of confirmation tags (currently `==`);
//! - the key-derivation/transcript binding choice (BLAKE3 over the raw SPAKE2
//!   key) vs a standard KDF;
//! - replay / session-binding of confirmation tags to the iroh channel.
//!
//! ## Alignment with `cbcl-bus` SPEC-007 (the in-house reference)
//!
//! `cbcl-bus` ships a more mature SPAKE2 agent-auth handshake (also "not yet
//! through Tier-1 review"). Compared against it, this module should adopt:
//! - **Proof-of-possession** binding the session to the peer's long-term key.
//!   cbcl signs `Ed25519(sk, "pop" ‖ id ‖ SHA256(K))` inside the authenticated
//!   session. ar-edit's identity *is* the iroh node key (see [`ActorId`]), so
//!   the pairing MUST bind `K` to the iroh node pubkey to stop key-substitution
//!   MITM — currently absent here.
//! - **Transcript binding**: register the peer's freshly-presented pubkey
//!   *inside* the handshake transcript (SPAKE2 gives this for free; we don't
//!   yet feed the pubkey into the transcript).
//! - **RFC 9382 / ristretto255** (cbcl's `cbcl-crypto-spake2`) vs this crate's
//!   `Ed25519Group` — decide/justify the group choice.
//! - **Labeled AEAD + KDF**: cbcl derives `K` and AEAD-encrypts payloads with
//!   domain-separated context strings; replace the raw `blake3(key)` here.
//! - **Burn-on-first-wrong-guess** (cbcl consumes the invite on the first
//!   failed confirm) vs this module's `ChannelGuard` N-attempt lockout.
//! See `../../cbcl-bus/docs/AGENT-AUTH.md` and
//! `cbcl-bus/docs/decisions/SPEC-012-tier1-gate-decisions.md`.
//!
//! Mechanism: each peer runs symmetric [`SPAKE2`] keyed by the shared pairing
//! phrase, exchanges one message, and derives a shared key — equal iff the
//! phrases match. A key-confirmation tag is then exchanged so a wrong phrase
//! fails closed *before* any project bytes flow (REQ-070). An active attacker
//! gets one online guess per attempt; repeated failures lock the channel
//! (NFR-014).

use crate::recognise::phrase::Phrase;
use spake2::{Ed25519Group, Identity, Password, Spake2};

const APP_ID: &[u8] = b"ar-edit/spake2/v1";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("SPAKE2 finish failed")]
    Finish,
    #[error("key confirmation failed (wrong phrase)")]
    ConfirmFailed,
    #[error("channel locked out after too many failed attempts")]
    LockedOut,
}

/// In-progress handshake awaiting the peer's message.
pub struct PendingPairing {
    state: Spake2<Ed25519Group>,
}

/// A derived shared session key. Equal on both peers iff their phrases matched.
pub struct SessionKey {
    raw: Vec<u8>,
}

/// Begin a symmetric SPAKE2 handshake from the pairing phrase. Returns the
/// pending state and the single outbound message to send to the peer.
pub fn start(phrase: &Phrase) -> (PendingPairing, Vec<u8>) {
    let pw = phrase.render();
    let (state, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(pw.as_bytes()),
        &Identity::new(APP_ID),
    );
    (PendingPairing { state }, outbound)
}

impl PendingPairing {
    /// Complete the handshake with the peer's message, deriving the shared key.
    pub fn finish(self, peer_msg: &[u8]) -> Result<SessionKey, PairingError> {
        let raw = self
            .state
            .finish(peer_msg)
            .map_err(|_| PairingError::Finish)?;
        Ok(SessionKey { raw })
    }
}

impl SessionKey {
    /// A 32-byte key for downstream symmetric encryption (derived from the
    /// SPAKE2 output). REVIEW: replace with a standard KDF (HKDF) at audit.
    pub fn bytes(&self) -> [u8; 32] {
        *blake3::hash(&self.raw).as_bytes()
    }

    /// The confirmation tag this peer broadcasts. Both peers compute the same
    /// tag iff their keys (and hence phrases) match.
    pub fn confirm_tag(&self) -> [u8; 32] {
        *blake3::keyed_hash(&self.bytes(), b"ar-edit/pair/confirm").as_bytes()
    }

    /// Verify the peer's confirmation tag. A mismatch means the phrases
    /// differed: fail closed (REQ-070). REVIEW: use constant-time comparison.
    pub fn verify_peer(&self, peer_tag: &[u8; 32]) -> Result<(), PairingError> {
        if &self.confirm_tag() == peer_tag {
            Ok(())
        } else {
            Err(PairingError::ConfirmFailed)
        }
    }

    /// Test/debug: whether two keys are identical.
    pub fn agrees_with(&self, other: &SessionKey) -> bool {
        self.raw == other.raw
    }
}

/// Per-channel failed-attempt limiter (NFR-014): an active attacker gets one
/// online guess per attempt, and the channel is invalidated after `max`
/// failures.
pub struct ChannelGuard {
    failures: u32,
    max: u32,
}

impl ChannelGuard {
    pub fn new(max: u32) -> Self {
        Self { failures: 0, max }
    }

    pub fn is_locked(&self) -> bool {
        self.failures >= self.max
    }

    /// The configured failure threshold (NFR-014).
    pub fn max(&self) -> u32 {
        self.max
    }

    /// Record the outcome of an attempt; returns `LockedOut` once the limit is
    /// reached.
    pub fn record(&mut self, ok: bool) -> Result<(), PairingError> {
        if self.is_locked() {
            return Err(PairingError::LockedOut);
        }
        if !ok {
            self.failures += 1;
            if self.is_locked() {
                return Err(PairingError::LockedOut);
            }
        }
        Ok(())
    }
}
