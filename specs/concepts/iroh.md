# iroh

A Rust library for **direct peer-to-peer connections** built on [[QUIC]] (via
the `quinn` stack). iroh gives each node a cryptographic identity (an Ed25519
keypair → node ID), authenticated and encrypted connections, NAT traversal with
hole-punching, and relay fallback for when a direct path cannot be established —
so two peers can usually connect by node ID regardless of network topology.

In [[SPEC-003-realtime-collaborative-editing#ADR-008]] iroh is the p2p transport:
small frequent [[CRDT]] deltas travel on a bidirectional control stream
([[SPEC-003-realtime-collaborative-editing#CON-015]]), and bulk source media is
transferred with [[iroh-blobs]]. A peer is addressed by a [[NodeTicket]]
exchanged during pairing. The peer's node public key also seeds its [[CRDT]]
actor ID ([[SPEC-003-realtime-collaborative-editing#REQ-072]]).
