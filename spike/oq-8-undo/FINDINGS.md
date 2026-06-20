# Spike — OQ-8 / ADR-011 Phase 0: durable single-writer undo over the CRDT

**Question.** Can the unified single-store on-disk format (CRDT as canonical, per
ADR-011) support `ar-edit undo` / `redo` across one-shot CLI invocations — with
**no daemon and no in-memory undo manager** — using only what is persisted?

**Verdict: YES.** Proven by `crates/ar-edit-collab/tests/spike_durable_undo.rs`
(2 tests, passing). The head-over-oplog model is viable and durable.

## Mechanism

Model `head` as a cursor over **checkpoints** (Loro frontiers, persisted as
opaque bytes), and move between them with **`LoroDoc::revert_to`**. The persisted
state is exactly:

```
(snapshot_bytes, history: Vec<checkpoint_bytes>, head: usize)
```

- After each user edit: truncate any redo branch, `commit`, push
  `state_frontiers()` (encoded), `head += 1`.
- `undo`: `revert_to(history[head-1])`, `head -= 1`.
- `redo`: `revert_to(history[head+1])`, `head += 1`.
- `history[0]` is the empty baseline, so undo of the first edit reverts to empty.

This is the offline analog of the per-actor `UndoManager`; the collaborative
`UndoManager` path is unchanged when a daemon/peers are present.

## Why `revert_to`, not `checkout`

This was the key discovery and it changes the design:

- **`checkout(frontiers)` detaches the doc** — "the document is not editable…
  any import operations will be recorded in the OpLog without being applied".
  Editing after a checkout needs `set_detached_editing(true)`, which **uses a
  different PeerID per checkout** — churning the actor id (and our
  `shot-<actor>-<n>` minting) on every undo-then-edit. Rejected.
- **`revert_to(frontiers)` stays attached** — it "generate[s] a series of local
  operations… apply[s] the diff to the current state." So:
  - the doc remains editable (edit-after-undo just works, no forking);
  - undo is **durable** (the revert ops are in the oplog/snapshot, so the
    reloaded latest state *is* the post-undo state — no re-checkout on load);
  - undo **propagates to peers as an ordinary CRDT update** — directly satisfying
    REQ-086's "the inverse change propagates to all peers".

Consequence: undo/redo are *forward* ops. "Latest" always equals the current
visible state, so persistence round-trips with zero extra reconciliation; only
the `(history, head)` cursor needs storing.

## What the tests prove

1. `durable_undo_redo_survives_persistence` — 3 adds → undo×2 → **persist + reload**
   → still 1 shot (undo survived) → redo×2 restores 3 shots **with identical
   minted ids** → undo + new edit **truncates redo** (SPEC-001 fork parity) →
   doc attached throughout.
2. `undo_cursor_stable_across_multiple_restarts` — undo stays durable across
   **three** process boundaries; a checkpoint recorded in process 1 is a valid
   `revert_to` target in process 3 (frontiers stay valid because reverts only
   *append* to the oplog).

## API added (minimal, spike-marked in `crdt.rs`)

- `CollabDoc::checkpoint() -> Vec<u8>` — encoded `state_frontiers()`.
- `CollabDoc::revert_to(&[u8])` — decode + `revert_to` + counter resync.
- `CollabDoc::is_attached() -> bool`.

Byte-based on purpose: that is what the on-disk format persists, and it keeps
Loro types out of the public surface.

## Implications for the phased plan

- **Phase 1 (persistence).** The on-disk edit file becomes `(crdt snapshot,
  history, head)`. The JSON `snapshot`/`ops`/`head` view is *derived* via
  `materialise` for `--json`/`show`/agents (ADR-011 already promises this).
- **Phase 3 (undo unification).** Offline undo = this cursor; in-session undo =
  the existing collaborative `UndoManager`. Both are attached, both durable in
  the snapshot. Single source of truth ⇒ the review #5–#8 guard apparatus
  (`daemon_op_id`, op-tag stacks, gating, rollbacks) is **deleted**.
- **`edit history`.** Frontiers give the linear cursor; a richer per-op history
  (SPEC-001 REQ-046/047) maps from the Loro change log — confirm shape in Phase 2.

## Open follow-ups (not blockers)

- **Compaction.** Undo/redo append revert ops, so the oplog grows like the old
  `ops` log. Same order of magnitude as today; Loro shallow-snapshot / gc is a
  later optimisation, not Phase 1.
- **`history` size.** One frontier per edit; encode is small. Cap/trim policy is
  a Phase-3 detail (mirror the old head-pointer unboundedness, which was fine).
- **Collaborative + offline interplay.** When a doc has both a durable cursor and
  live peers, decide whether offline-cursor undo or `UndoManager` undo is used
  while attached to a session (likely: daemon present ⇒ `UndoManager`; absent ⇒
  cursor). Phase 4 wiring.

**Bottom line:** Phase 0 de-risked. `revert_to` + a persisted frontier cursor
gives durable, attached, peer-propagating single-writer undo. The rest of the
unification is mechanical (persistence format + command cutover + deleting the
guards), per the incremental rollout.
