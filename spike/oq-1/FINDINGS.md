# Findings — OQ-1 De-risking Spike

"After" gate for [[SPEC-003-realtime-collaborative-editing#OQ-1]]. Pairs with
BRIEF.md. Reproduce: `cd spike/oq-1 && cargo run --release`.

- **Model:** claude-opus-4-8[1m] (Claude Code) · **Date:** 2026-06-18
- **Host:** macOS, 10 cores, 16 GB RAM, rustc 1.88.0
- **Libraries:** loro 1.13.1, spake2 0.4.0, blake3 1.8.5
- **Result:** H-a PASS · H-b PASS · H-c PASS (process exit 0)

## H-a — Loro `MovableList` convergence (ADR-007 / REQ-080, REQ-081, REQ-083)

**Confirmed.** Edit doc modelled as `MovableList<shot-id>` for order + `Map`
keyed by shot-id for ranges.

| Case | Concurrent ops | Observed | Verdict |
|------|----------------|----------|---------|
| 1 | Two peers `mov` the **same** shot (s3) to different positions | Both converge to `[s1,s3,s2]`; s3 present exactly once; count stable at 3 | Identity-preserving, no dup/loss ✓ (REQ-080) |
| 2 | Peer A `mov`s s2 to front; peer B trims s2's range | Converge identical; `order[0]==s2` **and** `range==210..265` | Move + field-edit both survive on one identity ✓ (REQ-080+081) |
| 3 | 3 peers (insert s4, move s1, edit s3) delivered to two observers in **different orders** | Byte-identical deep value on both observers | Order-independent merge ✓ (REQ-083 SEC) |

**Conclusion:** Loro's `MovableList` delivers identity-preserving concurrent
moves and Strong Eventual Consistency out of the box — the single riskiest novel
claim in SPEC-003 is retired. This is the decisive evidence behind
[[SPEC-003-realtime-collaborative-editing#ADR-007]] choosing Loro over Automerge
(whose move-as-delete+insert would have failed case 1/2). **Confidence: High.**

## H-b — SPAKE2 handshake (ADR-009 / REQ-070, NFR-010, NFR-014)

**Confirmed (feasibility).**
- Same phrase ⇒ equal 32-byte keys; **mismatched phrase ⇒ unequal keys**
  (fail-closed, REQ-070).
- Full symmetric handshake compute: **0.219 ms** (n=200).
- Loopback TCP RTT floor (one relay hop): **0.047 ms** median.

**Conclusion:** SPAKE2 crypto is ~0.2 ms — three to four orders of magnitude
below the [[SPEC-003-realtime-collaborative-editing#NFR-010]] 5 s pairing
budget. Pairing latency will be dominated by network round-trips through the
[[Rendezvous Server]] and iroh path-finding, **not** by the cryptography.
**Confidence: High for feasibility; Medium for end-to-end NFR-010** (real WAN
rendezvous + iroh hole-punching latency not measured — see Deferred). The
single-guess property (NFR-014) is an analytic property of SPAKE2, not measured
here; it remains a no-go-area item requiring audited-impl + expert review.

## H-c — BLAKE3 content-addressing (ADR-010 / REQ-075, REQ-076, NFR-011)

**Confirmed.** 2 GiB hashed in RAM (no disk):
- One-shot single-thread: **1.16 s → 1.85 GB/s**.
- Streamed in 1 MiB chunks: **0.83 s → 2.58 GB/s**, and the streamed digest
  **equals** the one-shot digest (verified-streaming sanity for REQ-076).

**Conclusion:** Even single-threaded, BLAKE3 hashes a 2 GB source in ~1 s —
~15× a 1 Gbps LAN and far above any WAN link. Content-addressing and integrity
verification are **not** the replication bottleneck; bandwidth is. Supports
[[SPEC-003-realtime-collaborative-editing#ADR-010]] and the dedup/integrity path
of [[SPEC-003-realtime-collaborative-editing#REQ-075]]/REQ-076.
**Confidence: High** (hashing ceiling); the network transfer ceiling is separate
(Deferred). `blake3`'s `rayon` feature would raise the multi-core ceiling
further if ever needed.

## Deferred (host disk-constrained, ~700 MB free)

The iroh / iroh-blobs transport tree (quinn/rustls) needs multiple GB of build
output and a 2 GB on-disk transfer; **not run** this session. Specifically:
- **Real pairing latency** (NFR-010): rendezvous + iroh hole-punch /
  relay-fallback round-trips on a real network.
- **Real media throughput** (NFR-011): iroh-blobs over a direct iroh connection
  for a 2 GB set; resumability and dedup-skip behaviour.

Tracked as a follow-up spike (SPEC-003 OQ-7). These are
transport-characterisation, lower-risk than the CRDT-semantics question this
spike answered, and well-bounded by the floors measured above.

## Recommendation

Promote [[SPEC-003-realtime-collaborative-editing#ADR-007]],
[[SPEC-003-realtime-collaborative-editing#ADR-009]], and
[[SPEC-003-realtime-collaborative-editing#ADR-010]] from "pending spike" to
**accepted on the merge/crypto/hashing claims**, with transport-latency/throughput
NFRs (NFR-010, NFR-011) carrying a "lower-bounded, full empirical run deferred to
OQ-7" caveat. The Phase-0 Experiment-vs-Specify gate is satisfied for the
specify path: the High-novelty risk (CRDT convergence) is empirically retired.

## Decommission

Source (BRIEF.md, FINDINGS.md, Cargo.toml, src/main.rs) checked in as the
governed-experiment record. Build artefacts (`target/`) cleaned to reclaim disk;
re-run with `cargo run --release`.
