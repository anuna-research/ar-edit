# ar-edit-collab

Realtime, multiplayer collaborative editing for `ar-edit` — the edit document
reformulated as a collection of CRDTs, plus peer-to-peer transport, pairing, and
content-addressed source sync. Implements [SPEC-003](../../specs/SPEC-003-realtime-collaborative-editing.md).

## Quick Start

```rust
use ar_edit_collab::{CollabDoc, ids::ActorId, materialise::materialise, migrate};

// Migrate an existing event-sourced edit document into the CRDT form, owned by
// this node's actor for subsequent edits.
let actor = ActorId::from_public_key(node_public_key);
let doc = migrate::from_event_sourced(&edit_document, actor);

// Concurrent edits from peers merge automatically; read the materialised view.
let snapshot = materialise(&doc);
```

Run the tests:

```bash
cargo test -p ar-edit-collab                                   # pure core (23)
cargo test -p ar-edit-collab --features rendezvous --test rendezvous   # relay (2)
# iroh transport needs rustc >= 1.91 (iroh 1.0 MSRV):
RUSTC=~/.rustup/toolchains/stable-*/bin/rustc \
  ~/.rustup/toolchains/stable-*/bin/cargo test -p ar-edit-collab --features transport --test transport
```

## Architecture

Strict purity boundary (see SPEC-003 Purity Boundary Map): everything except
`shell` is pure (deterministic, no I/O, no `iroh`), which is what makes
convergence property-testable.

**Pure core**
- `crdt` — the edit document as a [Loro](../../specs/concepts/Loro.md) doc: a
  `MovableList` of shot ids + keyed maps for fields/notes/markers/POIs.
  Identity-preserving moves, LWW fields, observed-remove sets, grow-only notes.
- `materialise` — deterministic `CRDT → EditSnapshot/EditDocument` (the JSON view
  the rest of `ar-edit` consumes unchanged).
- `migrate` — event-sourced edit document → one-actor CRDT, snapshot-identical.
- `undo` — per-actor undo/redo via Loro's `UndoManager`.
- `recognise` — LangSec recognisers for the pairing phrase (CON-013) and wire
  frames (CON-014/015/016); fail-closed.
- `reconcile` / `ids` — source-manifest reconciliation + BLAKE3 integrity; actor
  ids derived from node public keys.
- `pairing` — SPAKE2 handshake + key confirmation + lockout. **⚠ NO-GO crypto
  area (ADR-009): implemented and tested, but requires audited + human-expert +
  cross-model review before production acceptance.**

**Effectful shell** (feature-gated)
- `shell::transport` (`transport`) — iroh delta transport + content-addressed
  blob transfer with a BLAKE3 integrity gate.
- `shell::rendezvous` (`rendezvous`) — TCP relay matching peers on a channel and
  relaying opaque handshake frames.

## Features

| Feature | Enables | Notes |
|---------|---------|-------|
| *(none)* | pure core + pairing + presence | builds on any rustc; no iroh/tokio |
| `rendezvous` | TCP rendezvous relay | pulls `tokio` only |
| `transport` | iroh delta + blob transport | pulls `iroh`; **needs rustc ≥ 1.91** |
| `blobsync` | (reserved) iroh-blobs dedup/resume | optimization layer, deferred |

## Status

See [IMPL-003 status](../../plans/IMPL-003-STATUS.md). The CRDT engine, transport,
pairing, sync, presence, and rendezvous are implemented and tested; the live
SPAKE2 pairing is pending its security review and the live CLI session is behind
the `collab-transport` feature in the `ar-edit` binary.
