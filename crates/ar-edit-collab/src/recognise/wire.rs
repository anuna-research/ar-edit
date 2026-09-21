//! LangSec recognisers for the on-wire frames (SPEC-003 CON-014/015/016).
//!
//! Every frame is length-prefixed (`u32` big-endian) and externally tagged.
//! Recognition is full and fail-closed: a declared length over [`MAX_FRAME`] is
//! rejected *before* the body is required (DoS guard), an unknown tag or
//! protocol version is rejected with no partial dispatch, and opaque payloads
//! (SPAKE2 messages, tickets, CRDT deltas) are returned as byte slices for the
//! holder of the session key to interpret — never parsed here.

pub const MAX_FRAME: usize = 64 * 1024;
/// Cap for CRDT sync envelopes (CON-015): a full snapshot or an offline batch
/// of deltas can far exceed the small control-frame cap, so this matches the
/// transport read cap.
pub const MAX_SYNC_FRAME: usize = 64 * 1024 * 1024;
pub const PROTOCOL_VERSION: u8 = 1;

/// CON-014 direct-connection handshake tags. The live pairing
/// ([`crate::shell::transport`]) tags its SPAKE2 message and key-confirmation
/// frames with these; they are defined here — the single recogniser of record —
/// so the emitter and [`parse_rendezvous`] can never drift apart. The relay
/// validates every post-BIND frame with [`parse_rendezvous`], so the
/// DHT-blocked fallback can only tunnel the handshake if these are recognised.
pub const TAG_PAKE: u8 = 0x30;
pub const TAG_CONFIRM: u8 = 0x31;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WireError {
    #[error("frame shorter than its declared length")]
    TooShort,
    #[error("declared length exceeds MAX_FRAME")]
    TooLong,
    #[error("trailing bytes after frame")]
    Trailing,
    #[error("unknown message tag {0:#04x}")]
    UnknownTag(u8),
    #[error("unsupported protocol version {0}")]
    UnknownVersion(u8),
    #[error("hash list length is not a multiple of 32")]
    BadHashLen,
    #[error("empty body")]
    EmptyBody,
}

/// Read one exact `[u32 len][len bytes]` frame, enforcing the length cap before
/// requiring the body. Returns the body slice.
fn read_len_prefixed_cap(buf: &[u8], max: usize) -> Result<&[u8], WireError> {
    if buf.len() < 4 {
        return Err(WireError::TooShort);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > max {
        return Err(WireError::TooLong);
    }
    let body = &buf[4..];
    if body.len() < len {
        return Err(WireError::TooShort);
    }
    if body.len() > len {
        return Err(WireError::Trailing);
    }
    Ok(body)
}

fn read_len_prefixed(buf: &[u8]) -> Result<&[u8], WireError> {
    read_len_prefixed_cap(buf, MAX_FRAME)
}

// ---- CON-014: Rendezvous protocol ----

#[derive(Debug, PartialEq, Eq)]
pub enum RendezvousFrame<'a> {
    Bind(u16),
    PeerJoined(&'a [u8]),
    PakeMsg(&'a [u8]),
    Ticket(&'a [u8]),
    /// Key-confirmation frame of the direct CON-014 handshake (tag
    /// [`TAG_CONFIRM`]), tunnelled when the DHT-blocked relay fallback carries
    /// the live pairing.
    Confirm(&'a [u8]),
    Close(&'a [u8]),
    Error(u8),
}

pub fn parse_rendezvous(buf: &[u8]) -> Result<RendezvousFrame<'_>, WireError> {
    let body = read_len_prefixed(buf)?;
    let (tag, payload) = body.split_first().ok_or(WireError::EmptyBody)?;
    // Match on the value (not `&u8`) so the named handshake-tag constants are
    // read as constants rather than fresh bindings.
    match *tag {
        0x01 => {
            if payload.len() < 2 {
                return Err(WireError::TooShort);
            }
            Ok(RendezvousFrame::Bind(u16::from_be_bytes([
                payload[0], payload[1],
            ])))
        }
        0x02 => Ok(RendezvousFrame::PeerJoined(payload)),
        // 0x03/0x04 are the rendezvous protocol's own PAKE/ticket tags;
        // TAG_PAKE (0x30) is the current direct-handshake SPAKE2 tag the live
        // pairing emits. Both are opaque PAKE payloads as far as the relay is
        // concerned, so accept either rather than dropping the first
        // current-format frame of the DHT-blocked fallback.
        0x03 | TAG_PAKE => Ok(RendezvousFrame::PakeMsg(payload)),
        0x04 => Ok(RendezvousFrame::Ticket(payload)),
        TAG_CONFIRM => Ok(RendezvousFrame::Confirm(payload)),
        0x05 => Ok(RendezvousFrame::Close(payload)),
        0x06 => {
            let code = *payload.first().ok_or(WireError::TooShort)?;
            Ok(RendezvousFrame::Error(code))
        }
        other => Err(WireError::UnknownTag(other)),
    }
}

// ---- CON-015: CRDT sync envelope ----

#[derive(Debug, PartialEq, Eq)]
pub enum SyncEnvelope<'a> {
    Hello(&'a [u8]),
    Delta(&'a [u8]),
    SyncReq(&'a [u8]),
    Presence(&'a [u8]),
    Bye(&'a [u8]),
}

pub fn parse_sync_envelope(buf: &[u8]) -> Result<SyncEnvelope<'_>, WireError> {
    let body = read_len_prefixed_cap(buf, MAX_SYNC_FRAME)?;
    if body.len() < 2 {
        return Err(WireError::TooShort);
    }
    let version = body[0];
    if version != PROTOCOL_VERSION {
        // No best-effort cross-version parse — protocol divergence guard.
        return Err(WireError::UnknownVersion(version));
    }
    let tag = body[1];
    let payload = &body[2..];
    match tag {
        0x10 => Ok(SyncEnvelope::Hello(payload)),
        0x11 => Ok(SyncEnvelope::Delta(payload)),
        0x12 => Ok(SyncEnvelope::SyncReq(payload)),
        0x13 => Ok(SyncEnvelope::Presence(payload)),
        0x14 => Ok(SyncEnvelope::Bye(payload)),
        other => Err(WireError::UnknownTag(other)),
    }
}

// ---- CON-016: Source-sync / blob request ----

#[derive(Debug, PartialEq, Eq)]
pub enum SourceSyncMsg<'a> {
    Manifest(&'a [u8]),
    Want(Vec<[u8; 32]>),
    Have(Vec<[u8; 32]>),
    Conflict(&'a [u8]),
}

fn parse_hashes(payload: &[u8]) -> Result<Vec<[u8; 32]>, WireError> {
    if !payload.len().is_multiple_of(32) {
        return Err(WireError::BadHashLen);
    }
    Ok(payload
        .chunks_exact(32)
        .map(|c| {
            let mut h = [0u8; 32];
            h.copy_from_slice(c);
            h
        })
        .collect())
}

pub fn parse_source_sync(buf: &[u8]) -> Result<SourceSyncMsg<'_>, WireError> {
    let body = read_len_prefixed(buf)?;
    let (tag, payload) = body.split_first().ok_or(WireError::EmptyBody)?;
    match tag {
        0x20 => Ok(SourceSyncMsg::Manifest(payload)),
        0x21 => Ok(SourceSyncMsg::Want(parse_hashes(payload)?)),
        0x22 => Ok(SourceSyncMsg::Have(parse_hashes(payload)?)),
        0x23 => Ok(SourceSyncMsg::Conflict(payload)),
        other => Err(WireError::UnknownTag(*other)),
    }
}

/// Helper to frame a body for tests/senders: `[u32 len][body]`.
pub fn frame(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}
