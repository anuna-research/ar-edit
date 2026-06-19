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
use crate::pairing::{self, SessionKey};
use crate::recognise::phrase::Phrase;
use crate::recognise::wire::{self, SyncEnvelope, PROTOCOL_VERSION};
use super::discovery::Discovery;
use iroh::endpoint::{Builder, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr};
use std::net::SocketAddr;
use std::sync::Arc;

/// ALPN for the ar-edit collaboration protocol.
pub const COLLAB_ALPN: &[u8] = b"ar-edit/collab/0";

/// Hard ceiling on a single received source blob (REQ-075). Bounds the memory a
/// peer can force us to buffer before integrity verification — a sender that
/// never finishes, or advertises a 2 GiB media file but streams forever, is cut
/// off here instead of exhausting the process. Production callers should pass
/// the manifest-advertised size via [`Transport::fetch_blob_capped`]; this is
/// the fallback bound for [`Transport::fetch_blob`].
pub const MAX_BLOB_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// DELTA tag in the CON-015 envelope.
const TAG_DELTA: u8 = 0x11;
/// CON-014 pairing-handshake tags (over the direct iroh connection).
const TAG_PAKE: u8 = 0x30;
const TAG_CONFIRM: u8 = 0x31;

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
    #[error("blob exceeds the {0}-byte cap — rejected before exhausting memory")]
    TooLarge(usize),
    #[error("pairing: {0}")]
    Pairing(String),
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
        self.fetch_blob_capped(expected, MAX_BLOB_BYTES).await
    }

    /// As [`Self::fetch_blob`], but bound to `max_bytes` (e.g. the
    /// manifest-advertised source size plus a small margin). The transfer is
    /// aborted fail-closed the moment the running total would exceed the cap, so
    /// neither an oversized blob nor a peer that never sends EOF can exhaust
    /// memory before the BLAKE3 check completes (REQ-075/076).
    pub async fn fetch_blob_capped(
        &self,
        expected: &[u8; 32],
        max_bytes: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| TransportError::Iroh("endpoint closed".into()))?;
        let conn = incoming.await.map_err(iroh_err)?;
        let (_send, mut recv) = conn.accept_bi().await.map_err(iroh_err)?;
        // Stream the blob in chunks, hashing incrementally (BLAKE3 verified
        // streaming), but enforce a finite ceiling: arbitrarily large media is
        // fine up to the cap, beyond which we stop reading rather than buffer an
        // unbounded Vec.
        let mut hasher = blake3::Hasher::new();
        let mut data: Vec<u8> = Vec::new();
        while let Some(chunk) = recv.read_chunk(1024 * 1024).await.map_err(iroh_err)? {
            if data.len().saturating_add(chunk.len()) > max_bytes {
                return Err(TransportError::TooLarge(max_bytes));
            }
            hasher.update(&chunk);
            data.extend_from_slice(&chunk);
        }
        if hasher.finalize().as_bytes() == expected {
            Ok(data)
        } else {
            Err(TransportError::Integrity)
        }
    }

    /// Run the SPAKE2 pairing handshake as the **dialer**, over a fresh direct
    /// iroh connection to `to` (SPEC-003 REQ-070, CON-014). Returns the agreed
    /// [`SessionKey`] only if key confirmation succeeds; a wrong phrase fails
    /// closed. ⚠ NO-GO crypto path (ADR-009) — pending the mandated review.
    pub async fn pair_as_initiator(
        &self,
        to: EndpointAddr,
        phrase: &Phrase,
    ) -> Result<(Connection, SessionKey), TransportError> {
        let conn = self.endpoint.connect(to, COLLAB_ALPN).await.map_err(iroh_err)?;

        // PAKE message exchange on one bi-stream.
        let (pending, msg_a) = pairing::start(phrase);
        let (mut s, mut r) = conn.open_bi().await.map_err(iroh_err)?;
        write_tagged(&mut s, TAG_PAKE, &msg_a).await?;
        let msg_b = read_tagged(&mut r, TAG_PAKE).await?;
        let key = pending
            .finish(&msg_b)
            .map_err(|e| TransportError::Pairing(e.to_string()))?;

        // Key-confirmation exchange on a second bi-stream.
        let (mut s2, mut r2) = conn.open_bi().await.map_err(iroh_err)?;
        write_tagged(&mut s2, TAG_CONFIRM, &key.confirm_tag()).await?;
        let peer_tag = read_confirm(&mut r2).await?;
        key.verify_peer(&peer_tag)
            .map_err(|e| TransportError::Pairing(e.to_string()))?;
        // Keep the connection open for the session (both peers hold it, so
        // neither drops the endpoint mid-handshake).
        Ok((conn, key))
    }

    /// Run the SPAKE2 pairing handshake as the **accepter** (REQ-070, CON-014).
    pub async fn pair_as_responder(
        &self,
        phrase: &Phrase,
    ) -> Result<(Connection, SessionKey), TransportError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| TransportError::Iroh("endpoint closed".into()))?;
        let conn = incoming.await.map_err(iroh_err)?;

        let (pending, msg_b) = pairing::start(phrase);
        let (mut s, mut r) = conn.accept_bi().await.map_err(iroh_err)?;
        let msg_a = read_tagged(&mut r, TAG_PAKE).await?;
        write_tagged(&mut s, TAG_PAKE, &msg_b).await?;
        let key = pending
            .finish(&msg_a)
            .map_err(|e| TransportError::Pairing(e.to_string()))?;

        let (mut s2, mut r2) = conn.accept_bi().await.map_err(iroh_err)?;
        let peer_tag = read_confirm(&mut r2).await?;
        write_tagged(&mut s2, TAG_CONFIRM, &key.confirm_tag()).await?;
        key.verify_peer(&peer_tag)
            .map_err(|e| TransportError::Pairing(e.to_string()))?;
        Ok((conn, key))
    }

    /// **Host** a collaborative session (SPEC-003 REQ-068): publish a discovery
    /// record under `phrase`, accept a joining peer, complete SPAKE2, and return
    /// the paired [`Session`].
    pub async fn host_session(
        &self,
        discovery: &Discovery,
        phrase: &Phrase,
    ) -> Result<Session, TransportError> {
        discovery
            .publish(phrase, &self.dial_addr()?)
            .await
            .map_err(|e| TransportError::Pairing(e.to_string()))?;
        let (conn, key) = self.pair_as_responder(phrase).await?;
        Ok(Session { conn, key })
    }

    /// **Join** a collaborative session (SPEC-003 REQ-069): discover the host by
    /// `phrase`, dial, complete SPAKE2, and return the paired [`Session`].
    /// Retries discovery briefly to absorb publish/lookup propagation.
    pub async fn join_session(
        &self,
        discovery: &Discovery,
        phrase: &Phrase,
    ) -> Result<Session, TransportError> {
        let mut addr = None;
        for _ in 0..50 {
            if let Some(a) = discovery
                .lookup(phrase)
                .await
                .map_err(|e| TransportError::Pairing(e.to_string()))?
            {
                addr = Some(a);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let addr = addr.ok_or_else(|| TransportError::Pairing("peer not found".into()))?;
        let (conn, key) = self.pair_as_initiator(addr, phrase).await?;
        Ok(Session { conn, key })
    }

    /// Close the endpoint.
    pub async fn close(self) {
        self.endpoint.close().await;
    }
}

/// Write a CON-014 `[u32 len][tag][payload]` handshake message on a bi-stream
/// and finish the send half so the peer's `read_to_end` sees EOF. The `u32`
/// length prefix is mandated by CON-014; emitting only `[tag][payload]` makes a
/// conforming peer read the first length byte as the tag and fail pairing.
async fn write_tagged(s: &mut SendStream, tag: u8, payload: &[u8]) -> Result<(), TransportError> {
    let mut body = Vec::with_capacity(1 + payload.len());
    body.push(tag);
    body.extend_from_slice(payload);
    s.write_all(&wire::frame(&body)).await.map_err(iroh_err)?;
    s.finish().map_err(iroh_err)?;
    Ok(())
}

/// Read a CON-014 `[u32 len][tag][payload]` message, enforcing the length cap
/// and checking the expected tag.
async fn read_tagged(r: &mut RecvStream, expect: u8) -> Result<Vec<u8>, TransportError> {
    let bytes = r
        .read_to_end(4 + wire::MAX_FRAME)
        .await
        .map_err(iroh_err)?;
    if bytes.len() < 4 {
        return Err(TransportError::Pairing("handshake frame missing length prefix".into()));
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if len > wire::MAX_FRAME {
        return Err(TransportError::Pairing("handshake frame exceeds MAX_FRAME".into()));
    }
    let body = &bytes[4..];
    if body.len() != len {
        return Err(TransportError::Pairing(
            "handshake frame length mismatch".into(),
        ));
    }
    let (tag, payload) = body
        .split_first()
        .ok_or_else(|| TransportError::Pairing("empty handshake frame".into()))?;
    if *tag != expect {
        return Err(TransportError::Pairing(format!(
            "unexpected handshake tag {tag:#04x}"
        )));
    }
    Ok(payload.to_vec())
}

async fn read_confirm(r: &mut RecvStream) -> Result<[u8; 32], TransportError> {
    let payload = read_tagged(r, TAG_CONFIRM).await?;
    payload
        .try_into()
        .map_err(|_| TransportError::Pairing("confirmation tag must be 32 bytes".into()))
}

/// A paired collaborative session: SPAKE2 has agreed a key over the direct iroh
/// connection, which now carries CON-015 CRDT sync (SPEC-003 REQ-070 + REQ-084).
pub struct Session {
    conn: Connection,
    key: SessionKey,
}

impl Session {
    /// The agreed SPAKE2 session key.
    pub fn key(&self) -> &SessionKey {
        &self.key
    }

    /// Push a CRDT delta to the peer over the paired connection (REQ-084).
    pub async fn push_delta(&self, delta: &[u8]) -> Result<(), TransportError> {
        let (mut s, _r) = self.conn.open_bi().await.map_err(iroh_err)?;
        s.write_all(&encode_delta(delta)).await.map_err(iroh_err)?;
        s.finish().map_err(iroh_err)?;
        Ok(())
    }

    /// Receive one CRDT delta from the peer and apply it to `doc` (REQ-084).
    pub async fn pull_delta_into(&self, doc: &CollabDoc) -> Result<(), TransportError> {
        let (_s, mut r) = self.conn.accept_bi().await.map_err(iroh_err)?;
        let bytes = r.read_to_end(64 * 1024 * 1024).await.map_err(iroh_err)?;
        match wire::parse_sync_envelope(&bytes)
            .map_err(|e| TransportError::Envelope(e.to_string()))?
        {
            SyncEnvelope::Delta(payload) => doc
                .import(payload)
                .map_err(|e| TransportError::Import(e.to_string())),
            other => Err(TransportError::Envelope(format!("expected DELTA, got {other:?}"))),
        }
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
