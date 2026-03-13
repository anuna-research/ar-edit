---
id: SPEC-002
title: "SPEC-002: Points of Interest"
type: specification
version: 1.0.0
status: draft
---

# SPEC-002: Points of Interest

## Overview

Points of interest (POIs) are user- or agent-placed single-point annotations on source material that mark specific moments worth attention — a perfect reaction shot, a stumble to cut around, a topic transition, or a visual cue. Unlike markers (REQ-049), which annotate **ranges** of material, a POI marks a **single instant** in time. Unlike shot notes (REQ-051), which annotate shots in an edit document, POIs annotate **source material** and persist across edits.

POIs serve three workflows:

1. **Human review** — While watching raw footage, the editor drops POIs at moments of interest without breaking flow. These accumulate into a map of the source material that guides later assembly.
2. **Agent-driven analysis** — An LLM agent reviewing transcripts and scene descriptions places POIs to flag content landmarks (topic changes, key quotes, emotional peaks) that inform edit construction.
3. **Collaborative refinement** — During edit review, the editor drops POIs on the rendered output to mark moments that need attention. These POIs resolve back to source positions, giving the agent precise instructions.

### Relationship to Existing Concepts

| Concept | Scope | Granularity | Persistence |
|---------|-------|-------------|-------------|
| **Marker** (REQ-049) | Source | Range (from→to) | Source-level (`annotations/`) |
| **Shot Note** (REQ-051) | Edit | Free text on a shot | Edit-level (`edits/`) |
| **POI** (this spec) | Source | Single point | Source-level (`annotations/`) |

A POI can be thought of as a degenerate marker with zero duration — but the semantic distinction matters. Markers say "this **region** is interesting." POIs say "this **moment** is interesting." The different intent produces different workflows: markers drive shot selection (select/avoid ranges), while POIs drive navigation, annotation, and precise feedback.

---

## Functional Requirements

### POI Data Model

**REQ-054: POI Structure**

The system SHALL represent a point of interest as a data object containing: (a) a unique ID (format: `poi-NNN`, sequential per source), (b) a source ID, (c) a single point in time expressed as one of: a word index, a scene index, or a millisecond timestamp, (d) a category from a controlled vocabulary, (e) a freeform text note (optional), and (f) a creation timestamp WITH the POI stored in the source's annotations file.

Trace:
- TEST-059
- CON-011

**REQ-055: POI Point Types**

The system SHALL support three point types for POIs, mirroring the existing `ShotRange` variants but expressing a single point rather than a range:

- **Word**: A single word index (e.g., word 45) — resolves to that word's start timestamp
- **Scene**: A single scene index (e.g., scene 3) — resolves to that scene's start timestamp
- **Time**: A millisecond timestamp (e.g., 34500ms) — used when no transcript or index exists

WITH exactly one point type per POI. The choice of point type determines which data (transcript, index, or neither) is required.

Trace:
- TEST-060

**REQ-056: POI Categories**

The system SHALL enforce a controlled vocabulary for POI categories. The following categories SHALL be supported:

| Category | Semantic | Typical Use |
|----------|----------|-------------|
| `highlight` | A moment worth featuring | Best quote, peak emotion, visual payoff |
| `issue` | A moment to cut around | Stumble, noise, off-topic tangent |
| `transition` | A natural cut point | Topic change, pause, scene boundary |
| `cue` | A synchronisation anchor | Beat drop, gesture, visual cue for timing |
| `note` | General annotation | Any observation that doesn't fit above |

Custom categories SHALL be rejected by the CLI with an error listing the valid options. An agent or future spec may extend this vocabulary.

Trace:
- TEST-061

---

### POI Creation

**REQ-057: CLI POI Creation**

The system SHALL allow the user to create a POI on a source WHEN the user runs one of:

- `ar-edit poi add <source-id> --at-word <N> --category <cat> [--note <text>]`
- `ar-edit poi add <source-id> --at-scene <N> --category <cat> [--note <text>]`
- `ar-edit poi add <source-id> --at-ms <N> --category <cat> [--note <text>]`

WITH the POI assigned a sequential ID (`poi-001`, `poi-002`, ...) per source, stored in `annotations/<source-id>.pois.json`, and the creation timestamp recorded automatically.

Trace:
- TEST-062
- CON-011

**REQ-058: TUI POI Creation**

The system SHALL allow the user to drop a POI at the current cursor position in the transcript or scene panel WHEN the user presses `i` (for "interest") in the TUI, prompting for category selection via a single-key menu (`h`=highlight, `i`=issue, `t`=transition, `c`=cue, `n`=note) WITH the POI created at the word or scene index under the cursor. An optional note can be entered inline.

Trace:
- TEST-063

**REQ-059: POI Creation During Playback**

The system SHALL allow the user to drop a POI at the current playback position WHEN the user presses `i` during VLC/ffplay playback in the TUI, resolving the playback timestamp to the nearest word index (if a transcript exists) or storing as a millisecond timestamp WITH the POI category defaulting to `note` and editable after creation.

Trace:
- TEST-064

---

### POI Listing and Querying

**REQ-060: POI Listing**

The system SHALL list all POIs for a source or across all sources WHEN the user runs `ar-edit poi list [<source-id>] [--category <cat>]` WITH output showing: POI ID, source ID, point (word/scene/time), category, note, resolved timestamp, and surrounding transcript text (if available), in human-readable or `--json` format.

Trace:
- TEST-065
- CON-011

**REQ-061: POI in Transcript View**

The system SHALL interleave POI markers in transcript output WHEN the user runs `ar-edit transcripts read <source-id> --with-pois` or `ar-edit transcripts read <source-id> --with-markers --with-pois`, displaying each POI as an inline annotation at its word position WITH the category shown as a bracketed tag (e.g., `[highlight]`, `[issue]`).

Trace:
- TEST-066

**REQ-062: Agent Reads POIs**

The system SHALL include POIs in the structured JSON output of `ar-edit poi list --json` and `ar-edit transcripts read <source-id> --json --with-pois` WITH POIs positioned at their resolved timestamps so the agent can use them as landmarks when constructing or revising an edit.

Trace:
- TEST-067
- CON-011

---

### POI Deletion

**REQ-063: POI Removal**

The system SHALL remove a POI WHEN the user runs `ar-edit poi remove <source-id> --id <poi-id>` WITH the POI ID retired (not reused). Bulk removal by category SHALL be supported via `ar-edit poi remove <source-id> --category <cat>`.

Trace:
- TEST-068
- CON-011

---

### POI in Edit Review

**REQ-064: POI from Edit Playback Position**

The system SHALL resolve a POI placed during edit playback back to source coordinates WHEN the user drops a POI while watching a rendered edit, by mapping the edit timeline position to the source ID and source timestamp of the shot playing at that moment WITH the POI stored against the source (not the edit) so it persists across edit revisions.

Trace:
- TEST-069

**REQ-065: POI Visibility in Edit Show**

The system SHALL display POIs that fall within the range of each shot WHEN the user runs `ar-edit edit show <edit> --with-pois`, showing the POI category and note inline with the shot's transcript text WITH `--json` output including the full POI objects nested under each shot.

Trace:
- TEST-070
- CON-011

---

### POI Storage

**REQ-066: POI File Format**

The system SHALL store POIs in a per-source JSON file at `annotations/<source-id>.pois.json` with the following structure:

```json
{
  "source_id": "src-001",
  "pois": [
    {
      "id": "poi-001",
      "point": { "word": 45 },
      "category": "highlight",
      "note": "Perfect delivery of the key statistic",
      "created": "2026-03-13T10:30:00Z"
    },
    {
      "id": "poi-002",
      "point": { "time_ms": 62500 },
      "category": "issue",
      "note": "Microphone bump",
      "created": "2026-03-13T10:31:15Z"
    },
    {
      "id": "poi-003",
      "point": { "scene": 4 },
      "category": "transition",
      "note": null,
      "created": "2026-03-13T10:32:00Z"
    }
  ]
}
```

WITH the `point` field using an externally-tagged enum consistent with `ShotRange` serialisation conventions (ADR-002).

Trace:
- TEST-071

---

## Non-Functional Requirements

**NFR-007: POI Creation Latency**

POI creation SHALL complete in ≤ 50ms on local filesystem WITH no perceptible delay in the TUI or during playback annotation.

Trace:
- TEST-072

**NFR-008: POI Scale**

The system SHALL handle ≤ 10,000 POIs per source WITHOUT degradation in listing, querying, or transcript interleaving performance.

Trace:
- TEST-073

---

## Architecture Decisions

### ADR-005: POI as Separate File vs Extending Markers

**Status:** Proposed

**Context:** POIs could be stored as zero-duration markers in the existing `annotations/<source-id>.markers.json` file, or as a separate data type in a dedicated file.

**Options considered:**

| Option | Pros | Cons |
|--------|------|------|
| A. Zero-duration markers | Reuses existing code and storage; single query surface | Conflates range and point semantics; `ShotRange` with `from == to` is misleading; marker listing becomes noisy; category vocabulary conflicts with marker labels |
| B. Separate POI type and file | Clean semantic separation; independent evolution of categories; no pollution of marker queries; distinct CLI surface | Second annotation file per source; slight code duplication for file I/O |

**Decision:** Option B — separate POI type and file.

**Rationale:** Markers and POIs have different semantics (range vs point), different category vocabularies (`select`/`hero`/`avoid` vs `highlight`/`issue`/`transition`/`cue`/`note`), and different workflows. Forcing POIs into the marker model requires either a degenerate `from == to` range (which every consumer must special-case) or a new range variant (which complicates all existing range code). A separate type is simpler for both implementation and agent consumption. The I/O duplication is minimal — both are thin JSON wrappers over a vec of annotated structs.

### ADR-006: Point Type as Enum vs Millisecond-Only

**Status:** Proposed

**Context:** POIs could store only a millisecond timestamp (resolving words and scenes to ms at creation time) or preserve the original addressing mode as a tagged union.

**Options considered:**

| Option | Pros | Cons |
|--------|------|------|
| A. Millisecond-only | Simple; single type; no resolution required at read time | Loses semantic link to transcript word or scene; cannot survive re-transcription (word indices shift); agents lose the "which word" context |
| B. Tagged union (word / scene / time_ms) | Preserves original addressing; survives re-indexing of non-chosen types; agents can read "word 45" and look it up in transcript | Requires resolution to ms for playback; adds a point-type enum |

**Decision:** Option B — tagged union mirroring `ShotRange` but for single points.

**Rationale:** Consistency with the existing `ShotRange` design (ADR-002). The word-index form is especially valuable for agent workflows — an agent reading a transcript with interleaved POIs can see exactly which word is marked, not just a timestamp that requires cross-referencing. Resolution to milliseconds is a one-liner given the existing transcript and index data structures.

---

## Contract Specifications

### CON-011: POI CLI

#### `ar-edit poi add <source-id> --at-word <N> --category <cat> [--note <text>]`

Creates a POI at a word index.

```
Pre-conditions:  Source registered; transcript exists for word-type POIs; index exists for scene-type POIs
Post-conditions: POI appended to annotations/<source-id>.pois.json; sequential ID assigned
Exit codes:      0 = success, 1 = source not found, 2 = word/scene index out of bounds, 3 = invalid category
Output (--json): { "poi": { "id": "poi-001", "source_id": "src-001", "point": { "word": 45 }, "category": "highlight", "note": "...", "created": "..." } }
```

Implements: REQ-057

Verified by: TEST-062

#### `ar-edit poi add <source-id> --at-scene <N> --category <cat> [--note <text>]`

Creates a POI at a scene index.

```
Pre-conditions:  Source registered; scene index exists
Post-conditions: POI appended to annotations/<source-id>.pois.json; sequential ID assigned
Exit codes:      0 = success, 1 = source not found, 2 = scene index out of bounds, 3 = invalid category
Output (--json): { "poi": { "id": "poi-002", "source_id": "src-001", "point": { "scene": 3 }, "category": "transition", "note": null, "created": "..." } }
```

Implements: REQ-057

Verified by: TEST-062

#### `ar-edit poi add <source-id> --at-ms <N> --category <cat> [--note <text>]`

Creates a POI at a millisecond timestamp.

```
Pre-conditions:  Source registered; timestamp ≤ source duration
Post-conditions: POI appended to annotations/<source-id>.pois.json; sequential ID assigned
Exit codes:      0 = success, 1 = source not found, 2 = timestamp exceeds source duration, 3 = invalid category
Output (--json): { "poi": { "id": "poi-003", "source_id": "src-001", "point": { "time_ms": 34500 }, "category": "cue", "note": "Beat drop", "created": "..." } }
```

Implements: REQ-057

Verified by: TEST-062

#### `ar-edit poi list [<source-id>] [--category <cat>]`

Lists POIs, optionally filtered.

```
Pre-conditions:  Inside a project directory
Post-conditions: None (read-only)
Exit codes:      0 = success (including empty results), 1 = source not found
Output (--json): { "pois": [{ "id": "...", "source_id": "...", "point": {...}, "category": "...", "note": "...", "created": "...", "resolved_ms": 1234, "context": "...surrounding transcript text..." }] }
```

Implements: REQ-060

Verified by: TEST-065

#### `ar-edit poi remove <source-id> --id <poi-id>`

Removes a single POI.

```
Pre-conditions:  POI exists
Post-conditions: POI removed from annotations/<source-id>.pois.json; ID not reused
Exit codes:      0 = success, 1 = source or POI not found
Output (--json): { "removed": "poi-001" }
```

Implements: REQ-063

Verified by: TEST-068

#### `ar-edit poi remove <source-id> --category <cat>`

Removes all POIs of a given category.

```
Pre-conditions:  Source exists
Post-conditions: All matching POIs removed; IDs not reused
Exit codes:      0 = success (including zero matches), 1 = source not found, 3 = invalid category
Output (--json): { "removed": ["poi-002", "poi-005"], "count": 2 }
```

Implements: REQ-063

Verified by: TEST-068

---

## Test Specifications

**TEST-059: POI structure serialisation roundtrip**

Verify that a `SourcePois` struct with POIs of each point type (word, scene, time) serialises to JSON and deserialises back identically.

Implements: REQ-054

**TEST-060: POI point type variants**

Verify that each point type resolves to the correct millisecond timestamp given a transcript and scene index.

Implements: REQ-055

**TEST-061: POI category validation**

Verify that `ar-edit poi add` rejects unknown categories with exit code 3 and an error listing valid options.

Implements: REQ-056

**TEST-062: CLI POI creation**

For each point type (word, scene, time): create a POI via CLI, verify the JSON file is created/updated, the ID is sequential, and the `--json` output matches CON-011.

Implements: REQ-057

**TEST-063: TUI POI creation**

Verify that pressing `i` in the TUI transcript panel creates a POI at the cursor position with the selected category.

Implements: REQ-058

**TEST-064: POI during playback**

Verify that pressing `i` during playback creates a POI at the current playback timestamp, resolved to the nearest word index when a transcript exists.

Implements: REQ-059

**TEST-065: POI listing**

Verify that `ar-edit poi list` returns all POIs with resolved timestamps and context. Verify `--category` filtering returns only matching POIs. Verify `--json` output matches CON-011.

Implements: REQ-060

**TEST-066: POI in transcript view**

Verify that `ar-edit transcripts read <source-id> --with-pois` interleaves POI annotations at the correct word positions.

Implements: REQ-061

**TEST-067: Agent POI consumption**

Verify that `ar-edit poi list --json` and `ar-edit transcripts read --json --with-pois` include POI data at resolved positions suitable for agent consumption.

Implements: REQ-062

**TEST-068: POI removal**

Verify single-POI and bulk-by-category removal. Verify removed IDs are not reused by subsequent `poi add` calls.

Implements: REQ-063

**TEST-069: POI from edit playback**

Verify that a POI placed during edit playback resolves to the correct source and source-relative timestamp.

Implements: REQ-064

**TEST-070: POI in edit show**

Verify that `ar-edit edit show <edit> --with-pois` displays POIs falling within each shot's range.

Implements: REQ-065

**TEST-071: POI file format**

Verify that the on-disk JSON matches the schema defined in REQ-066, including externally-tagged point enum.

Implements: REQ-066

**TEST-072: POI creation latency**

Benchmark POI creation; verify ≤ 50ms on local filesystem.

Implements: NFR-007

**TEST-073: POI scale**

Create 10,000 POIs on a single source; verify listing and transcript interleaving complete without degradation (≤ 2x baseline latency).

Implements: NFR-008

---

## Purity Boundary Map

### Pure Core (no I/O, no shared state, deterministic)

- `PoiPoint` resolution: word index → ms, scene index → ms (given transcript/index data)
- POI category validation: string → Result<Category, InvalidCategory>
- POI interleaving: merge POI list into transcript word stream by position
- Edit-timeline-to-source resolution: edit position → (source_id, source_ms)

### Effectful Shell (orchestrates I/O, calls pure core)

- `poi::add_poi(project_dir, source_id, point, category, note)` → read/write JSON file
- `poi::list_pois(project_dir, source_id)` → read JSON file
- `poi::remove_poi(project_dir, source_id, poi_id)` → read/write JSON file
- CLI argument parsing and dispatch
- TUI event handling and rendering

### Boundary Contracts (data types crossing the boundary)

- `SourcePois` (in/out): the full per-source POI container
- `Poi` (in/out): individual POI struct
- `PoiPoint` (in): word/scene/time variant
- `PoiCategory` (in): validated category enum

### Dependency Rule

Dependencies point inward: shell → core. Core MUST NOT import from shell.

### Enforcement

Module visibility in `ar-edit-core` crate; `poi.rs` module follows the same pattern as `marker.rs`.

---

## Traceability Matrix

```
REQ-054 → TEST-059 → CON-011
REQ-055 → TEST-060
REQ-056 → TEST-061
REQ-057 → TEST-062 → CON-011
REQ-058 → TEST-063
REQ-059 → TEST-064
REQ-060 → TEST-065 → CON-011
REQ-061 → TEST-066
REQ-062 → TEST-067 → CON-011
REQ-063 → TEST-068 → CON-011
REQ-064 → TEST-069
REQ-065 → TEST-070 → CON-011
REQ-066 → TEST-071
NFR-007 → TEST-072
NFR-008 → TEST-073
```

---

## Ambiguity Log

| Original Term | Resolution |
|---------------|------------|
| "single point" | Exactly one of: word index, scene index, or millisecond timestamp — not a range |
| "controlled vocabulary" | Enum of exactly 5 values: highlight, issue, transition, cue, note — rejected at CLI parse time |
| "nearest word" | During playback POI creation, resolved to the word whose `[start_ms, end_ms]` interval contains the timestamp, or the closest word by `start_ms` if between words |
| "persists across edits" | POIs stored per-source in `annotations/`, not per-edit in `edits/` |
| "no perceptible delay" | ≤ 50ms for POI creation (NFR-007) |
| "without degradation" | ≤ 2x baseline latency at 10,000 POIs per source (NFR-008) |

---

## Open Questions

| # | Question | Status |
|---|----------|--------|
| 1 | Should POIs support user-defined custom categories beyond the five built-in ones? | Deferred — start with controlled vocabulary; extend via future spec if needed |
| 2 | Should POIs be includable in rendered video as chapter markers (MP4 chapter metadata)? | Deferred — natural extension but out of scope for this spec |
| 3 | Should there be an `ar-edit poi edit` command to update category or note on an existing POI? | Deferred — delete-and-recreate is sufficient for MVP; edit command adds complexity for minimal gain |
