# LangSec (Language-Theoretic Security)

A discipline that treats every input as a formal language and every input
handler as a **recogniser** for that language. The rules: define the language of
valid inputs precisely (at the lowest grammatical complexity that suffices),
parse input **fully** against that grammar before taking any semantic action,
reject anything that does not match exactly (no permissive normalisation, no
"parse a bit, act, parse a bit more"), and pass only typed, validated values —
never raw strings — across trust boundaries.

It is Constitutional Principle 14 of [[PROTO-001]]: ad-hoc parsers are the
dominant source of injection, memory-corruption, and authentication-bypass bugs,
and divergent re-implementations of one format ("shotgun parsers") create
exploitable disagreements.

In [[SPEC-003-realtime-collaborative-editing]] every externally-facing input has
a declared grammar and a full-recognition rule: the pairing phrase
([[SPEC-003-realtime-collaborative-editing#CON-013]]), the rendezvous protocol
([[SPEC-003-realtime-collaborative-editing#CON-014]]), the CRDT sync envelope
([[SPEC-003-realtime-collaborative-editing#CON-015]]), and the source-sync
messages ([[SPEC-003-realtime-collaborative-editing#CON-016]]). Each fails
closed and is fuzz-tested.
