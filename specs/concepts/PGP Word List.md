# PGP Word List

A list of 256 carefully chosen English words designed to be **read aloud and
transcribed unambiguously**, even over a noisy voice channel. The words are
selected to be phonetically distinct (minimising confusion between similar-
sounding words) and of varied length. Each byte value (0–255) maps to a word,
so a short sequence of words encodes a number compactly and human-friendly.

In [[SPEC-003-realtime-collaborative-editing#REQ-067]] the pairing phrase draws
its two words from the PGP Word List (or an audited equivalent of ≥ 256
phonetically distinct words), so a user can say a `<num>-<word>-<word>` phrase
to a collaborator and have it transcribed correctly. The words are the
low-entropy secret consumed by the [[SPAKE2]] exchange; the grammar that
recognises them is [[SPEC-003-realtime-collaborative-editing#CON-013]].
