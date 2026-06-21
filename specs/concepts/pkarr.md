# pkarr (Public-Key Addressable Resource Records)

A scheme for publishing small, **signed DNS-shaped records** addressed by an
Ed25519 public key, stored on the [[Mainline DHT]] (BitTorrent's DHT) and/or
relayed over HTTP (e.g. `relay.pkarr.org`). Anyone who knows the public key can
look the record up; the signature proves authenticity. [[iroh]] uses pkarr for
its own node discovery.

In [[SPEC-003-realtime-collaborative-editing#ADR-013]] ar-edit uses pkarr as a
**serverless rendezvous**: a discovery keypair is derived from the pairing
phrase, the host publishes a record carrying its [[iroh]] NodeAddr under that
key, and the joiner — deriving the same key — looks it up, dials directly, and
runs [[SPAKE2]] over the connection. The record format and its fail-closed
recognition are [[SPEC-003-realtime-collaborative-editing#CON-017]].

Because the phrase is low-entropy the derived key is enumerable; the security
analysis (single-guess [[PAKE]], short TTL, burn-on-failure, publish opt-out)
is in [[SPEC-003-realtime-collaborative-editing#ADR-013]]. Pattern after
`../did-crdt` ADR-006 / CON-006.
