# ar-edit-core

The **pure core** of [`ar-edit`](../../README.md): the deterministic,
side-effect-free heart of the transcript-based video editor. It defines the edit
document and its data model, materialises an edit into a concrete shot list,
validates an edit against a project, and plans renders — with no I/O, no shared
state, and no external processes.

This is the inward apex of the project's
[Purity Boundary Map](../../specs/SPEC-003-realtime-collaborative-editing.md):
the effectful shell ([`ar-edit`](../ar-edit/), [`ar-edit-collab`](../ar-edit-collab/))
depends on this crate; this crate depends on neither.

## Usage

```rust
use ar_edit_core::models::{EditDocument, ShotRange};

// Build and inspect an edit document (pure, in-memory).
let mut doc = EditDocument::create("rough-cut");
doc.push_op(/* an EditOp */);
let snapshot = &doc.snapshot;            // materialised shot list
```

It is consumed as a library; end users interact with the `ar-edit` binary.

## Architecture

| Module | Responsibility |
|--------|----------------|
| `models` | The edit document, shots, tagged-union shot ranges (`ADR-002`), markers, POIs |
| `edit` | Edit operations and range validation (the operation algebra) |
| `materialise` / `display` | Derive the read-view shot list and human/JSON presentation |
| `validate` | Check an edit against the project manifest (`CON-005`) |
| `render` | Plan an `ffmpeg` render (the plan is pure; execution lives in the shell) |
| `project` | Project manifest model |

Design rationale is in the ADRs: event-sourced edits (`ADR-001`, now the legacy
on-disk form migrated to a CRDT by `ADR-011`) and tagged-union shot ranges
(`ADR-002`). See [`specs/`](../../specs/).

## API Reference

The public surface is the `models`, `edit`, `materialise`, `validate`, and
`render` modules. Generate docs with `cargo doc -p ar-edit-core --open`.

## Development

```sh
cargo test -p ar-edit-core      # pure unit tests, no external tools needed
```

This crate has no effectful dependencies by design — adding an I/O, network, or
subprocess dependency here is an architecture violation (it would break the
inward Dependency Rule). Such capabilities belong in the shell crates.
