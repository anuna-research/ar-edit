//! SPAKE2 pairing tests (SPEC-003 REQ-070, NFR-014; task s2).
//! NB: these verify the handshake mechanism; production acceptance still
//! requires the mandated crypto review (ADR-009).

use ar_edit_collab::pairing::{self, ChannelGuard, PairingError};
use ar_edit_collab::recognise::phrase;

fn a_phrase() -> phrase::Phrase {
    phrase::generate_secure()
}

fn variant(p: &phrase::Phrase) -> phrase::Phrase {
    // A definitely-different but valid phrase (bump the channel).
    let ch = (p.channel + 1) % 1000;
    phrase::parse(&format!("{}-{}-{}", ch, p.words[0], p.words[1])).unwrap()
}

/// Matching phrases agree on a key and confirm successfully (REQ-070 happy).
#[test]
fn matching_phrases_agree() {
    let p = a_phrase();
    let (pa, msg_a) = pairing::start(&p);
    let (pb, msg_b) = pairing::start(&p);
    let ka = pa.finish(&msg_b).unwrap();
    let kb = pb.finish(&msg_a).unwrap();

    assert!(ka.agrees_with(&kb), "shared key must match");
    assert_eq!(ka.bytes(), kb.bytes());
    // Mutual key confirmation succeeds.
    assert_eq!(ka.verify_peer(&kb.confirm_tag()), Ok(()));
    assert_eq!(kb.verify_peer(&ka.confirm_tag()), Ok(()));
}

/// A wrong phrase fails key confirmation — fail closed, no agreement
/// (REQ-070 negative).
#[test]
fn wrong_phrase_fails_confirmation() {
    let p = a_phrase();
    let wrong = variant(&p);
    let (pa, msg_a) = pairing::start(&p);
    let (pw, msg_w) = pairing::start(&wrong);

    let ka = pa.finish(&msg_w).unwrap();
    let kw = pw.finish(&msg_a).unwrap();

    assert!(!ka.agrees_with(&kw), "different phrases must not agree");
    assert_eq!(
        ka.verify_peer(&kw.confirm_tag()),
        Err(PairingError::ConfirmFailed)
    );
}

/// Channel locks out after the configured number of failed attempts (NFR-014).
#[test]
fn channel_locks_out() {
    let mut guard = ChannelGuard::new(3);
    assert_eq!(guard.record(false), Ok(())); // 1
    assert_eq!(guard.record(false), Ok(())); // 2
    assert_eq!(guard.record(false), Err(PairingError::LockedOut)); // 3 -> locked
    assert!(guard.is_locked());
    assert_eq!(guard.record(true), Err(PairingError::LockedOut));
}
