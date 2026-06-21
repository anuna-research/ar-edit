//! LangSec recogniser for the pairing phrase (SPEC-003 CON-013, REQ-067/069).
//!
//! Grammar (ABNF):
//! ```text
//! phrase  = channel "-" word "-" word
//! channel = 1*3DIGIT          ; 0..=999, leading zeros allowed
//! word    = 1*( %x61-7A )     ; lowercase ASCII, AND member of the wordlist
//! ```
//! Full recognition before any value is emitted; no normalisation, trimming,
//! case-folding, or fuzzy matching — malformed input is rejected, never
//! repaired (Constitutional Principle 14, fail-closed). The caller performs no
//! network action on a `ParseError`.

use rand::Rng;
use std::collections::HashSet;
use std::sync::OnceLock;

/// The pairing wordlist is the BIP39 English list (2048 words). The whole
/// `<num>-<word>-<word>` phrase is the shared secret — both the SPAKE2 password
/// and the seed for the pkarr discovery key (SPEC-003 REQ-067 / ADR-013).
fn wordlist() -> &'static [&'static str; 2048] {
    bip39::Language::English.word_list()
}

fn wordset() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| wordlist().iter().copied().collect())
}

/// Number of words in the wordlist (entropy basis for REQ-067): 2048 (BIP39).
pub fn wordlist_len() -> usize {
    wordlist().len()
}

/// A recognised pairing phrase. Only this typed value — never the raw string —
/// crosses into the pairing logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    pub channel: u16,
    pub words: [String; 2],
}

impl Phrase {
    pub fn render(&self) -> String {
        format!("{}-{}-{}", self.channel, self.words[0], self.words[1])
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("phrase must be <channel>-<word>-<word>")]
    Shape,
    #[error("channel must be 0..=999")]
    ChannelRange,
    #[error("word contains non-lowercase-ascii characters")]
    BadWordChars,
    #[error("word is not in the wordlist")]
    UnknownWord,
}

/// Recognise a phrase. Returns a typed [`Phrase`] only on full success.
pub fn parse(s: &str) -> Result<Phrase, ParseError> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(ParseError::Shape);
    }
    let (ch, w1, w2) = (parts[0], parts[1], parts[2]);

    if ch.is_empty() || ch.len() > 3 || !ch.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::Shape);
    }
    let channel: u16 = ch.parse().map_err(|_| ParseError::ChannelRange)?;
    if channel > 999 {
        return Err(ParseError::ChannelRange);
    }

    for w in [w1, w2] {
        if w.is_empty() || !w.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(ParseError::BadWordChars);
        }
        if !wordset().contains(w) {
            return Err(ParseError::UnknownWord);
        }
    }

    Ok(Phrase {
        channel,
        words: [w1.to_string(), w2.to_string()],
    })
}

/// Generate a grammar-conformant phrase from a (preferably cryptographic) RNG
/// (REQ-067).
pub fn generate<R: Rng + ?Sized>(rng: &mut R) -> Phrase {
    let wl = wordlist();
    let channel = rng.gen_range(0..=999u16);
    let w1 = wl[rng.gen_range(0..wl.len())];
    let w2 = wl[rng.gen_range(0..wl.len())];
    Phrase {
        channel,
        words: [w1.to_string(), w2.to_string()],
    }
}

/// Generate a phrase from the OS CSPRNG (the production entry point).
pub fn generate_secure() -> Phrase {
    generate(&mut rand::rngs::OsRng)
}
