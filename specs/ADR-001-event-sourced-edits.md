---
title: "ADR-001: Event-Sourced Edit Document"
type: architecture-decision-record
status: accepted
parent: SPEC-001
---

# ADR-001: Event-Sourced Edit Document

## Context

The edit document is the only mutable data structure in the project. Users (human and agent) need undo/redo, operation history, and non-destructive editing. The edit document is small (kilobytes, not megabytes) even for complex edits.

## Decision

Use an **append-only operation log with a head pointer and cached snapshot** as the edit document format.

Every mutation (add, remove, move, trim) appends an operation to the log. Undo/redo moves the head pointer. The snapshot is a materialized view of `ops[0..=head]`, recomputed on undo and cached on disk for fast reads.

## Alternatives Considered

### A. Mutable JSON + external .history file

Store the edit as a plain shots array. On each mutation, copy the previous state to a `.history/` directory with a timestamp.

- **Pro**: Simpler file format; agent reads a plain shots array
- **Con**: Undo requires reading from a separate directory; history management is a separate concern; no atomic undo/redo — two files to update

### B. Git-backed versioning

Use an embedded git repository (libgit2) to version the edit document. Each mutation is a commit. Undo = `git checkout HEAD~1`.

- **Pro**: Full branching, diffing, merging for free; familiar model
- **Con**: Heavy dependency (libgit2); overkill for a single small file; agent would need to understand git; slow for rapid undo/redo sequences

### C. In-memory undo stack only (no persistence)

Keep an undo stack in the TUI process. Lost on exit.

- **Pro**: Simplest implementation
- **Con**: No undo across sessions; agent cannot undo; no audit trail

## Consequences

- The edit document file is larger than a plain shots array (includes ops), but still small (~10KB for 200 operations)
- Snapshot recomputation on undo is O(n) where n = number of ops, but n is small (< 1000 for any realistic edit)
- The agent can ignore the operation log entirely and read only `snapshot.shots`
- The agent can also write snapshot-only documents (`ops: [], head: -1`) which are treated as a fresh starting point with no history
- Every operation is timestamped, providing a full audit trail
- The TUI maps ctrl-z/ctrl-y directly to head pointer movement

## Trace

- REQ-046 (Undo)
- REQ-047 (Redo)
- REQ-048 (Operation History)
