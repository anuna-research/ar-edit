//! LangSec recogniser tests (SPEC-003 CON-013/014/015/016, TEST-074/075/078).

use ar_edit_collab::recognise::phrase::{self, ParseError};
use ar_edit_collab::recognise::wire::{
    self, RendezvousFrame, SourceSyncMsg, SyncEnvelope, WireError, MAX_FRAME,
};

// ---- CON-013 pairing phrase ----

#[test]
fn phrase_generate_roundtrips() {
    // TEST-074: generated phrases are grammar-conformant and parse back.
    assert!(phrase::wordlist_len() >= 256, "entropy basis (REQ-067)");
    for _ in 0..1000 {
        let p = phrase::generate_secure();
        let parsed = phrase::parse(&p.render()).expect("generated phrase must parse");
        assert_eq!(parsed, p);
        assert!(parsed.channel <= 999);
    }
}

#[test]
fn phrase_rejects_malformed() {
    // TEST-075/078: malformed input rejected, no value emitted.
    assert_eq!(phrase::parse("notaphrase"), Err(ParseError::Shape));
    assert_eq!(phrase::parse("7-onlyoneword"), Err(ParseError::Shape));
    assert_eq!(phrase::parse("7-a-b-c"), Err(ParseError::Shape));
    // 1*3DIGIT caps the channel at 999, so any 4+ digit number is a shape
    // error (ChannelRange is defensively present but unreachable via grammar).
    assert_eq!(phrase::parse("1000-x-y"), Err(ParseError::Shape));
    assert_eq!(phrase::parse("9999-x-y"), Err(ParseError::Shape));
    // Uppercase / non-ascii word chars.
    let good = phrase::generate_secure();
    let w = &good.words[0];
    assert_eq!(
        phrase::parse(&format!("7-{}-{}", w.to_uppercase(), w)),
        Err(ParseError::BadWordChars)
    );
    // Lowercase-ascii but not in the (4-6 letter) wordlist.
    assert_eq!(
        phrase::parse(&format!("7-zzzzzzz-{w}")),
        Err(ParseError::UnknownWord)
    );
}

// ---- CON-014 rendezvous ----

#[test]
fn rendezvous_frames() {
    assert_eq!(
        wire::parse_rendezvous(&wire::frame(&[0x01, 0x00, 0x07])),
        Ok(RendezvousFrame::Bind(7))
    );
    assert_eq!(
        wire::parse_rendezvous(&wire::frame(&[0x03, 0xde, 0xad])),
        Ok(RendezvousFrame::PakeMsg(&[0xde, 0xad]))
    );
    assert_eq!(
        wire::parse_rendezvous(&wire::frame(&[0x99])),
        Err(WireError::UnknownTag(0x99))
    );
}

/// Regression (P2): the direct CON-014 handshake tags its PAKE and
/// key-confirmation frames 0x30/0x31. The relay validates every post-BIND frame
/// with `parse_rendezvous`, so these current tags must be recognised — otherwise
/// the DHT-blocked fallback drops the first handshake frame.
#[test]
fn rendezvous_accepts_current_handshake_tags() {
    assert_eq!(
        wire::parse_rendezvous(&wire::frame(&[0x30, 0xbe, 0xef])),
        Ok(RendezvousFrame::PakeMsg(&[0xbe, 0xef]))
    );
    assert_eq!(
        wire::parse_rendezvous(&wire::frame(&[0x31, 0xca, 0xfe])),
        Ok(RendezvousFrame::Confirm(&[0xca, 0xfe]))
    );
}

// ---- CON-015 sync envelope ----

#[test]
fn sync_envelope() {
    // version 1, tag DELTA(0x11)
    assert_eq!(
        wire::parse_sync_envelope(&wire::frame(&[0x01, 0x11, 1, 2, 3])),
        Ok(SyncEnvelope::Delta(&[1, 2, 3]))
    );
    // version mismatch -> rejected, no partial parse
    assert_eq!(
        wire::parse_sync_envelope(&wire::frame(&[0x02, 0x11, 1, 2, 3])),
        Err(WireError::UnknownVersion(2))
    );
    // unknown tag
    assert_eq!(
        wire::parse_sync_envelope(&wire::frame(&[0x01, 0x7f])),
        Err(WireError::UnknownTag(0x7f))
    );
}

#[test]
fn sync_envelope_allows_large_payload() {
    // Regression (P2): a DELTA payload larger than the 64 KiB control-frame cap
    // must parse (real snapshots/deltas exceed it).
    let payload = vec![0xABu8; 128 * 1024];
    let mut body = vec![0x01, 0x11]; // version 1, DELTA
    body.extend_from_slice(&payload);
    match wire::parse_sync_envelope(&wire::frame(&body)) {
        Ok(SyncEnvelope::Delta(p)) => assert_eq!(p.len(), payload.len()),
        other => panic!("expected Delta, got {other:?}"),
    }
}

// ---- CON-016 source sync + frame guards ----

#[test]
fn source_sync_and_caps() {
    // WANT with one 32-byte hash
    let mut body = vec![0x21u8];
    body.extend_from_slice(&[7u8; 32]);
    match wire::parse_source_sync(&wire::frame(&body)) {
        Ok(SourceSyncMsg::Want(hs)) => {
            assert_eq!(hs.len(), 1);
            assert_eq!(hs[0], [7u8; 32]);
        }
        other => panic!("expected Want, got {other:?}"),
    }
    // WANT with a non-multiple-of-32 hash list -> rejected
    assert_eq!(
        wire::parse_source_sync(&wire::frame(&[0x21, 1, 2, 3])),
        Err(WireError::BadHashLen)
    );
    // Oversized declared length rejected before requiring the body (DoS guard).
    let forged = ((MAX_FRAME as u32) + 1).to_be_bytes().to_vec();
    assert_eq!(wire::parse_source_sync(&forged), Err(WireError::TooLong));
    // Declared length longer than the actual body -> TooShort.
    let truncated = vec![0, 0, 0, 10, 0x20, 1];
    assert_eq!(wire::parse_source_sync(&truncated), Err(WireError::TooShort));
}
