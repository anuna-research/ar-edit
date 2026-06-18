# IMPL-003 Status — Realtime Collaborative Editing (SPEC-003)

Implementation of [[SPEC-003-realtime-collaborative-editing]] via
`plans/IMPL-003-collab.spl`. Crate: `crates/ar-edit-collab` (+ CLI in
`crates/ar-edit`).

**Status: 17/17 tasks done. 33 tests passing.**

## Test matrix

| Suite | Build | Tests |
|-------|-------|-------|
| Pure core (crdt, materialise, migrate, undo, recognise, reconcile, pairing, presence) | default (any rustc) | 23 |
| Rendezvous relay | `--features rendezvous` (tokio) | 2 |
| iroh transport + blob integrity + **pkarr discovery** | `--features transport` (rustc ≥ 1.91) | 5 |
| Collaboration CLI | `ar-edit` binary | 3 |

Reproduce:
```
cargo test -p ar-edit-collab                                   # pure core (23)
cargo test -p ar-edit-collab --features rendezvous --test rendezvous
cargo test -p ar-edit -p ar-edit --test collab_cli             # CLI (3)
# transport needs rustc >= 1.91 (iroh 1.0); this host: rustup `stable` = 1.93
RUSTC=~/.rustup/toolchains/stable-*/bin/rustc \
  ~/.rustup/toolchains/stable-*/bin/cargo test -p ar-edit-collab --features transport --test transport
```

## What each task delivered

| Task | Module / file | REQs | Verified by |
|------|---------------|------|-------------|
| p0 | crate + features (`rendezvous`/`transport`/`blobsync`) | — | builds; pure core has **0 iroh/tokio** in normal deps |
| p1 | `crdt.rs` (Loro `MovableList` + keyed maps) | 079/080/081/082 | convergence (move-same, move+trim, SEC, inserts, markers, notes) |
| p2 | `materialise.rs` | 079 | JSON-identical to single-player |
| p3 | `migrate.rs` | 088 | snapshot-identity |
| p4 | `undo.rs` (per-actor `UndoManager`) | 086 | A's undo spares B's change |
| p5 | `recognise/phrase.rs` (256-word list) | 067/069 | parse + fuzz-style negatives |
| p6 | `recognise/wire.rs` | CON-014/015/016 | fail-closed frame parsing |
| p7 | `reconcile.rs`, `ids.rs` | 072/074/076 | manifest merge/conflict, BLAKE3 gate |
| p8 | `tests/*` | 083 (SEC) etc. | property/example suite |
| s1 | `shell/transport.rs` (iroh 1.0) | 071/084 | **delta propagates over a real iroh QUIC loopback connection** |
| s2 | `pairing.rs` (SPAKE2) | 070/NFR-014 | key agreement, wrong-phrase fail-closed, lockout — **⚠ pending crypto review** |
| s3 | `shell/transport.rs` (blob xfer) | 075/076 | content-addressed transfer; **tampered blob rejected fail-closed** |
| s4 | `presence.rs` | 073/085 | ephemeral peer/cursor table |
| s5 | delta-sync test | 087 | offline edits reconcile delta-only, no loss |
| s6 | `ar-edit` CLI (`share`/`pair`/`session`) | CON-012 | **malformed phrase → exit 1, no network** (TEST-078) |
| s7 | `shell/rendezvous.rs` (TCP relay) | 071 (CON-014) | opaque relay + channel isolation |
| s8 | `shell/discovery.rs` (pkarr / Mainline DHT) | 068/069/071 (ADR-013, CON-017) | **deterministic phrase key; CON-017 record roundtrip; end-to-end discover-by-phrase → dial → delta propagates (hermetic in-process backend)** |

> **SPEC-003 v1.1.0 (discovery):** the dedicated rendezvous server is replaced by
> serverless phrase-keyed pkarr / Mainline DHT discovery
> ([ADR-013](../specs/SPEC-003-realtime-collaborative-editing.md)); the s7 TCP
> relay is demoted to an optional DHT-blocked fallback. The pkarr discovery
> module (`shell/discovery.rs`, task s8) is now **implemented and tested**:
> `derive_keypair` (phrase→Ed25519), CON-017 record build/parse, and an
> in-process backend (hermetic) + an HTTP pkarr-relay backend (production, over
> the Mainline DHT). The wordlist is BIP39 (2048 words, `bip39` crate). Remaining:
> a real-DHT/relay round-trip (the in-process backend stands in for the DHT in
> tests — see OQ-7) and the live SPAKE2 crypto review.

## Outstanding gates (recorded, not hidden)

1. **s2 SPAKE2 — mandated crypto review (ADR-009 / AI Trust Boundaries).** The
   handshake is implemented and tested, but is a NO-GO area: it MUST get
   audited-impl + human-expert + cross-model review before production
   acceptance. Open review items are listed in `pairing.rs` (constant-time tag
   compare, KDF choice, session/replay binding). Live-session CLI join is
   therefore behind the `collab-transport` build feature.
2. **iroh transport requires rustc ≥ 1.91** (iroh 1.0 MSRV); the Homebrew 1.88
   on PATH cannot build it — use the rustup `stable` (1.93) toolchain.
3. **iroh-blobs dedup/resume** (s3 optimization) deferred — the content-addressed
   *verified transfer* requirement (REQ-075/076) is met over the iroh stream;
   block-level dedup and resumability are a follow-up (SPEC-003 OQ-2/OQ-7).
4. **NFR-010/011 real-network numbers** (pairing latency, 2 GB throughput) still
   want a non-loopback run (SPEC-003 OQ-7); loopback proves correctness, not WAN
   performance.
5. **Adversarial review of SPEC-003** (CP11–12) before the spec → `approved`.
