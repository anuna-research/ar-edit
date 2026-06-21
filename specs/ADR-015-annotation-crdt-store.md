---
id: ADR-015
title: "ADR-015: Markers & POIs as a Per-Source Annotation CRDT Store"
type: adr
status: accepted
parent: SPEC-003
---

# ADR-015: Markers & POIs as a Per-Source Annotation CRDT Store

## Status

Accepted (2026-06-21). Refines [[SPEC-003-realtime-collaborative-editing#ADR-007]]
and [[SPEC-003-realtime-collaborative-editing#REQ-082]]; complements
[[SPEC-003-realtime-collaborative-editing#ADR-011]] (the per-edit CRDT store).

## Context

[[SPEC-003-realtime-collaborative-editing#REQ-082]] requires source **markers**
and **points of interest (POIs)** to be collaborative CRDT sets that converge
across peers. Two facts about the data shape the design:

1. **Markers/POIs are per-*source*, project-wide.** A marker on `src-001`
   (e.g. "words 45–120 is the best take") annotates the *source material*, not a
   particular edit. It is the same marker whether you are viewing `rough-cut` or
   `trailer`. The legacy storage reflects this: `annotations/<source>.markers.json`
   and `annotations/<source>.pois.json`, one file per source, shared across edits.
2. **Edits are per-*edit*.** [[SPEC-003-realtime-collaborative-editing#ADR-011]]
   gives each edit its own CRDT store (`edits/<name>.edit.json`, one `CollabDoc`).

[[SPEC-003-realtime-collaborative-editing#ADR-007]] as originally written says
"one [[Loro]] document … Map sets for markers/POIs", implying a single document
holds the edit *and* its annotations. But under the per-edit model of ADR-011
that would put markers inside *each* edit's document — duplicating project-wide
annotations across edits and letting them diverge (a marker removed in one edit's
copy but not another's). That contradicts fact (1).

## Decision

**Markers and POIs live in their own per-source annotation CRDT store, separate
from the per-edit edit store.** Each source has one `AnnotationStore`
(`crates/ar-edit-collab/src/annotations.rs`) wrapping a `CollabDoc` whose
`markers` and `pois` observed-remove sets (REQ-082) hold that source's
annotations, persisted to `annotations/<source>.annot.json` and synced between
peers exactly like an edit.

- **Migration** from the legacy plain-JSON `annotations/<source>.{markers,pois}.json`
  is deterministic: seeding ops are written under the fixed `MIGRATION_ACTOR`
  (then re-keyed to the local actor), so two peers migrating the same legacy
  files independently produce identical ops and merge idempotently — the same
  scheme ADR-011 uses for edit migration (REQ-088).
- **Sync parity:** the annotation store is a `CollabDoc` like the edit store, so
  the remote-sync transport (still gated, OQ-7) carries it with no new machinery.

This **supersedes the "one Loro document" wording of
[[SPEC-003-realtime-collaborative-editing#ADR-007]]**: the project's
collaborative state is a *collection* of CRDT documents — one per edit, one per
source's annotations — not a single document. The convergence and OR-set
guarantees of ADR-007/REQ-082 are unchanged; only the document boundary moves.

## Consequences

- **Positive:** matches the per-source data model; no duplication or cross-edit
  divergence of annotations; the working, mutation-hardened per-edit store is
  untouched (lower risk); annotation sync rides the existing CRDT transport.
- **Negative:** more documents to sync (one per source's annotations) rather than
  one monolith; the spec's ADR-007 wording needed reconciling (done here).
- **Follow-up:** the `mark` / `poi` CLI commands route through the
  `AnnotationStore` (replacing direct legacy-file writes); wiring the annotation
  store into the live daemon/remote-sync path is part of the broader OQ-7 work,
  shared with edits.

## Alternatives considered

- **One project-wide Loro document for everything** (edits + all annotations) —
  most faithful to ADR-007's literal wording, and markers are naturally
  project-wide there. Rejected: it would require collapsing the per-edit store
  (ADR-011) into a monolith, a large refactor of working, tested code, for no
  data-model benefit over separate documents.
- **Keep markers/POIs as local files, sync via blobs** — rejected: markers/POIs
  are *mutable* collaborative state needing merge (REQ-082), unlike the immutable
  derived artefacts (transcripts, indices) that
  [[SPEC-003-realtime-collaborative-editing#REQ-077]] replicates as blobs.
