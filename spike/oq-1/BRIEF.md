# Exploration Brief — OQ-1 De-risking Spike

Governed experiment under PROTO-001 Experiment Governance, for
[[SPEC-003-realtime-collaborative-editing#OQ-1]]. This brief is the "Before"
gate; FINDINGS.md is the "After" gate.

## Hypothesis

The three highest-novelty claims of SPEC-003 are technically sound on the chosen
libraries:

- **H-a (ADR-007 / REQ-080, REQ-081, REQ-083):** Loro's `MovableList` converges
  under concurrent **move + move** and **move + field-edit** of the same shot to
  a single deterministic state on all replicas, with no duplication and no lost
  field edit — i.e. it gives Strong Eventual Consistency with
  identity-preserving moves out of the box.
- **H-b (ADR-009 / REQ-070, NFR-010, NFR-014):** A SPAKE2 handshake derives a
  matching key from a shared phrase, fails on mismatch, and its *compute* cost is
  negligible relative to the 5 s pairing budget (NFR-010); the relay round-trip,
  not the crypto, dominates.
- **H-c (ADR-010 / REQ-075, REQ-076, NFR-011):** BLAKE3 content-addressing of a
  2 GB source set is fast enough not to bottleneck replication on a fast link.

## Approach

Isolated standalone crate (`spike/oq-1`, its own `[workspace]`, not a member of
the ar-edit workspace). Three experiments in `src/main.rs`, run with
`cargo run --release`:

- **exp_a_loro** — build replicas, apply concurrent ops, cross-merge in varying
  orders, assert deep-value equality. Correctness assertions (panic ⇒ fail).
- **exp_b_spake2** — run matched and mismatched handshakes (assert key
  match/mismatch), time N handshakes; measure a loopback TCP RTT as the
  rendezvous-relay floor.
- **exp_c_blake3** — hash a 2 GB in-RAM buffer, report GB/s (single-thread and,
  if available, multi-thread).

## Metrics & Exit Criteria

| Claim | Metric | Pass threshold |
|-------|--------|----------------|
| H-a | Replica deep-value equality after concurrent merge | 100% across all orderings; element count stable (no dup/loss) |
| H-b | SPAKE2 full-handshake compute time | ≪ 100 ms (so it is a rounding error vs NFR-010's 5 s) |
| H-b | Key match (same phrase) / mismatch (diff phrase) | match ⇒ equal keys; mismatch ⇒ unequal/err |
| H-c | BLAKE3 throughput on 2 GB | ≥ 1 GB/s single-thread (≫ typical LAN/WAN bandwidth ⇒ not the bottleneck) |

A claim **fails** the spike if its metric misses threshold or an assertion
trips; failure feeds back into the relevant ADR before SPEC-003 → `approved`.

## Risks / Isolation

- **No production writes.** Standalone crate; touches nothing under `crates/`.
  No project data read or written. Decommission: `cargo clean` + remove
  `spike/` after findings are recorded.
- **Disk-capped.** Host has ~700 MB free, so the full iroh / iroh-blobs
  transport experiments are **out of scope for this run** and explicitly
  deferred (see FINDINGS "Deferred"). Dependencies chosen (loro, spake2, blake3)
  avoid the iroh/quinn/rustls tree.
- **Loopback ≠ WAN.** RTT/throughput floors measured locally are lower bounds;
  real pairing/transfer latency is bounded below, not predicted, by them.

## AI Trust Boundary metadata

- Detecting/authoring model: claude-opus-4-8[1m] (Claude Code), 2026-06-18.
- Method: governed experiment (code + benchmark), human-reviewed.
- SPAKE2 touches a no-go cryptographic area; this spike validates feasibility
  only — production integration still requires audited-impl + human expert +
  cross-model review per ADR-009.
