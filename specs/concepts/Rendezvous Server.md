# Rendezvous Server

A lightweight relay whose only job is to let two peers, who know a shared short
code, **find each other** and exchange a handshake — after which they communicate
directly. It is a meeting point, not a data hub.

In [[SPEC-003-realtime-collaborative-editing]] (the *magic-wormhole* model) the
rendezvous server:

1. matches peers on a **channel** encoded by the leading `<num>` of the pairing
   phrase;
2. relays the opaque [[SPAKE2]] handshake messages and the [[iroh]]
   [[NodeTicket]]s the peers need to connect directly;
3. is then dropped — all project data (CRDT deltas, source blobs, presence)
   flows over the direct peer-to-peer [[iroh]] connection
   ([[SPEC-003-realtime-collaborative-editing#REQ-071]]).

Critically, the server **never learns** the pairing phrase, the derived session
key, or any project content — the [[SPAKE2]] and ticket payloads are opaque to
it ([[SPEC-003-realtime-collaborative-editing#CON-014]]). Whether it is
self-hosted, Anuna-operated, or pluggable is
[[SPEC-003-realtime-collaborative-editing#OQ-3]].
