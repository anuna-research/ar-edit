# BIP39

A Bitcoin Improvement Proposal that defines a standard **mnemonic wordlist** for
encoding entropy as human-transcribable words. The English list has exactly
**2048 words**, each uniquely identified by its first four letters and chosen to
be distinct and easy to read aloud or type.

In [[SPEC-003-realtime-collaborative-editing#REQ-067]] ar-edit draws the two
`<word>` tokens of a `<num>-<word>-<word>` pairing phrase from the BIP39 English
list. The **whole phrase** is the shared low-entropy secret: both the [[SPAKE2]]
password and the seed for the phrase-derived [[pkarr]] discovery key
([[SPEC-003-realtime-collaborative-editing#ADR-013]]). Two BIP39 words give
2048² ≈ 2^22 combinations, plus ~2^10 from `<num>`.

Chosen over the PGP word list for being a single audited standard with broad
tooling, and for consistency with the in-house `cbcl-bus` SPEC-007 agent-auth
pairing, which also uses BIP39 mnemonics.
