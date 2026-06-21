# ar-edit (CLI)

The `ar-edit` command-line binary: the **effectful shell** that turns user
commands into edits, renders, and collaboration sessions. It recognises
arguments at the trust boundary, dispatches to the pure core
([`ar-edit-core`](../ar-edit-core/)) and the collaboration layer
([`ar-edit-collab`](../ar-edit-collab/)), and orchestrates external processes
(`ffmpeg`, `whisper.cpp`, a player).

For the full project overview, install steps, and the complete command surface,
see the [project README](../../README.md).

## Usage

```sh
# Edit (one-shot commands; structured output with --json)
ar-edit edit add-segment rough-cut --source src-001 --from-word 0 --to-word 52
ar-edit edit show rough-cut --json
ar-edit undo rough-cut            # durable undo over the CRDT cursor
ar-edit validate rough-cut
ar-edit render rough-cut -o cut.mp4

# Collaboration (SPEC-003)
ar-edit share                     # print a pairing phrase
ar-edit pair 7-saturn-pioneer     # join a session
ar-edit daemon --edit rough-cut   # background host that owns the live edit
```

## Architecture

- **Argument recognition** — commands are recognised before any action
  (fail-closed on malformed input); the pairing phrase grammar is `CON-013`.
- **Edit commands** route through an `EditSession` that either operates the
  canonical file store directly or, when a live daemon **owns** the edit,
  attaches to it over IPC (`CON-018`) — one store, so the live session and the
  file never diverge (`REQ-089`/`REQ-090`, `ADR-011`/`ADR-014`). A daemon socket
  that is present but unresponsive makes a mutation **fail closed**.
- **Render/play** shell out to `ffmpeg` / a player from a plan computed in the
  pure core.

Design decisions: dual-mode TUI + CLI (`ADR-004`), subprocess architecture
(`ADR-003`). The collaboration contracts are `CON-012`…`CON-018` in
[`SPEC-003`](../../specs/SPEC-003-realtime-collaborative-editing.md).

## Development

```sh
cargo test -p ar-edit                               # CLI integration tests
cargo build -p ar-edit --features collab-transport  # live P2P session (rustup stable ≥ 1.91)
```

`ffmpeg` must be on `PATH` to render. The collaboration daemon/transport are
opt-in build features; the default binary is the local single-machine editor.
