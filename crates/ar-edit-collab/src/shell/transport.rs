//! iroh delta transport (SPEC-003 REQ-084, REQ-071; task s1).
//!
//! Carries CRDT updates between peers over a direct iroh ([`QUIC`]) connection.
//! A locally-produced delta is wrapped in the CON-015 sync envelope
//! (`[u32 len][version][tag=DELTA][payload]`) — the same grammar the pure-core
//! recogniser in [`crate::recognise::wire`] validates — and applied to the
//! remote [`CollabDoc`] via Loro import. The transport never inspects the delta
//! payload beyond the envelope (REQ-084).
//!
//! Built behind the `transport` feature so the pure core stays iroh-free.

use crate::crdt::CollabDoc;
use crate::ids::ActorId;
use crate::recognise::wire::{self, SyncEnvelope, PROTOCOL_VERSION};
use iroh::endpoint::Builder;
use iroh::{Endpoint, EndpointAddr};
use std::net::SocketAddr;
use std::sync::Arc;

/// ALPN for the ar-edit collaboration protocol.
pub const COLLAB_ALPN: &[u8] = b"ar-edit/collab/0";

/// DELTA tag in the CON-015 envelope.
const TAG_DELTA: u8 = 0x11;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("iroh: {0}")]
    Iroh(String),
    #[error("malformed sync envelope: {0}")]
    Envelope(String),
    #[error("loro import: {0}")]
    Import(String),
    #[error("no bound socket")]
    NoSocket,
    #[error("blob failed BLAKE3 integrity check — rejected")]
    Integrity,
}

fn iroh_err<E: std::fmt::Display>(e: E) -> TransportError {
    TransportError::Iroh(e.to_string())
}

/// A bound iroh endpoint for collaboration.
pub struct Transport {
    endpoint: Endpoint,
}

impl Transport {
    /// Bind a hermetic, relay-free endpoint on loopback (test/LAN). Uses
    /// `Builder::empty()` (RelayMode::Disabled, no discovery) so no external
    /// service is contacted.
    pub async fn bind_loopback() -> Result<Self, TransportError> {
        // `Builder::empty()` configures no crypto provider; supply ring's
        // (the same rustls 0.23 iroh links, so it is compatible).
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("valid addr");
        let endpoint = Builder::empty()
            .crypto_provider(provider)
            .alpns(vec![COLLAB_ALPN.to_vec()])
            .bind_addr(addr)
            .map_err(iroh_err)?
            .bind()
            .await
            .map_err(iroh_err)?;
        Ok(Self { endpoint })
    }

    /// This peer's CRDT actor id, derived from its iroh node public key
    /// (REQ-072).
    pub fn actor(&self) -> ActorId {
        ActorId::from_public_key(self.endpoint.id().as_bytes())
    }

    /// A directly-dialable address for this endpoint (loopback direct address,
    /// no relay).
    pub fn dial_addr(&self) -> Result<EndpointAddr, TransportError> {
        let socket = self
            .endpoint
            .bound_sockets()
            .into_iter()
            .next()
            .ok_or(TransportError::NoSocket)?;
        Ok(EndpointAddr::new(self.endpoint.id()).with_ip_addr(socket))
    }

    /// Send `doc`'s full state (or a delta blob) to a peer as a CON-015 DELTA
    /// envelope over a fresh bi-stream.
    pub async fn send_delta(
        &self,
        to: EndpointAddr,
        delta: &[u8],
    ) -> Result<(), TransportError> {
        let conn = self
            .endpoint
            .connect(to, COLLAB_ALPN)
            .await
            .map_err(iroh_err)?;
        let (mut send, _recv) = conn.open_bi().await.map_err(iroh_err)?;
        send.write_all(&encode_delta(delta))
            .await
            .map_err(iroh_err)?;
        send.finish().map_err(iroh_err)?;
        // Give the stream time to flush before closing.
        conn.closed().await;
        Ok(())
    }

    /// Accept one incoming connection and apply its DELTA into `doc`
    /// (REQ-084: remote delta applied to local state).
    pub async fn accept_into(&self, doc: &CollabDoc) -> Result<(), TransportError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| TransportError::Iroh("endpoint closed".into()))?;
        let conn = incoming.await.map_err(iroh_err)?;
        let (_send, mut recv) = conn.accept_bi().await.map_err(iroh_err)?;
        let bytes = recv.read_to_end(64 * 1024 * 1024).await.map_err(iroh_err)?;
        match wire::parse_sync_envelope(&bytes).map_err(|e| TransportError::Envelope(e.to_string()))? {
            SyncEnvelope::Delta(payload) => {
                doc.import(payload)
                    .map_err(|e| TransportError::Import(e.to_string()))?;
                Ok(())
            }
            other => Err(TransportError::Envelope(format!("expected DELTA, got {other:?}"))),
        }
    }

    /// Send raw source-blob bytes to a peer (content-addressed transfer,
    /// REQ-075). The blob is identified out-of-band by its BLAKE3 hash; the
    /// receiver verifies it on receipt via [`Self::fetch_blob`].
    pub async fn send_blob(&self, to: EndpointAddr, blob: &[u8]) -> Result<(), TransportError> {
        let conn = self.endpoint.connect(to, COLLAB_ALPN).await.map_err(iroh_err)?;
        let (mut send, _recv) = conn.open_bi().await.map_err(iroh_err)?;
        send.write_all(blob).await.map_err(iroh_err)?;
        send.finish().map_err(iroh_err)?;
        conn.closed().await;
        Ok(())
    }

    /// Accept a source blob and admit it **only if** its recomputed BLAKE3 hash
    /// matches `expected` (REQ-076). A mismatch is rejected fail-closed — the
    /// bytes are never returned (the caller therefore never writes them to
    /// `sources/`).
    pub async fn fetch_blob(&self, expected: &[u8; 32]) -> Result<Vec<u8>, TransportError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| TransportError::Iroh("endpoint closed".into()))?;
        let conn = incoming.await.map_err(iroh_err)?;
        let (_send, mut recv) = conn.accept_bi().await.map_err(iroh_err)?;
        let bytes = recv
            .read_to_end(512 * 1024 * 1024)
            .await
            .map_err(iroh_err)?;
        if crate::reconcile::verify_blob(expected, &bytes) {
            Ok(bytes)
        } else {
            Err(TransportError::Integrity)
        }
    }

    /// Close the endpoint.
    pub async fn close(self) {
        self.endpoint.close().await;
    }
}

/// Wrap a delta blob in a CON-015 DELTA envelope.
fn encode_delta(delta: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + delta.len());
    body.push(PROTOCOL_VERSION);
    body.push(TAG_DELTA);
    body.extend_from_slice(delta);
    wire::frame(&body)
}
