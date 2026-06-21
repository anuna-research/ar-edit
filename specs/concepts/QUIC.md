# QUIC

A general-purpose transport protocol that runs over UDP and provides
TLS-encrypted, multiplexed, connection-oriented streams without the
head-of-line blocking of TCP. A single QUIC connection carries many independent
bidirectional streams, supports connection migration across network changes, and
establishes secure sessions with a fast handshake.

[[iroh]] is built on QUIC (via the `quinn` implementation), which is why a
single iroh connection can multiplex frequent small [[CRDT]] deltas alongside
bulk [[iroh-blobs]] media transfer without one starving the other — relevant to
the propagation-latency target
[[SPEC-003-realtime-collaborative-editing#NFR-009]] holding even during a large
source sync.
