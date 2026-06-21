---
title: "SPEC-001: Data Model"
type: specification-addendum
version: 1.0.0
parent: SPEC-001
---

# Data Model

## Overview

The system has 5 core data structures stored as JSON files in the project directory. The key design principle is that **the edit document is a thin reference layer** — it contains only source IDs and range references. All timing, text, and metadata are resolved at runtime by joining the edit document against the transcripts or scene index.

### Two data layers, independently populated

Every source can have **both** layers — they are not mutually exclusive:

| Layer | Atom | Produced by | Provides |
|-------|------|-------------|----------|
| **Transcript** | Word | `ar-edit transcribe` (whisper.cpp) | What is being *said* — word text, timing, confidence |
| **Scene Index** | Scene | `ar-edit index` (ffmpeg scene detect + agent descriptions) | What is being *shown* — setting, subjects, action |

An interview video has transcript AND visual context. A drone shot has scenes AND maybe ambient audio worth transcribing. Search spans both layers.

A shot in the edit document specifies its range using **one of** these:
1. **Word range** — `from_word` / `to_word` (requires transcript)
2. **Scene range** — `from_scene` / `to_scene` (requires scene index)
3. **Time range** — `from_ms` / `to_ms` (direct, always works)

This means an agent can find a segment by searching the transcript ("when Alice mentions climate policy"), the scene descriptions ("exterior shot of the coastline"), or both ("the part where Alice is standing outside discussing the reef").

```
project/
├── manifest.json              # Project manifest
├── sources/                   # Symlinks or copies of source videos
│   ├── src-001.mp4
│   └── src-002.mp4
├── transcripts/
│   ├── src-001.transcript.json
│   └── src-002.transcript.json
├── index/
│   ├── src-001.index.json
│   └── src-002.index.json
├── thumbnails/
│   ├── src-001_00m00s.jpg
│   ├── src-001_00m10s.jpg
│   └── ...
└── edits/
    ├── rough-cut.edit.json
    └── final.edit.json
```

---

## 1. Project Manifest

The root of the project. Created by `ar-edit init`.

```json
{
  "version": "1.0.0",
  "name": "my-project",
  "created": "2026-02-19T12:00:00Z",
  "sources": [
    {
      "id": "src-001",
      "path": "sources/src-001.mp4",
      "original_filename": "interview-alice.mp4",
      "duration_ms": 124500,
      "video_codec": "h264",
      "audio_codec": "aac",
      "resolution": [1920, 1080],
      "frame_rate": 29.97,
      "audio_channels": 2,
      "audio_sample_rate": 48000,
      "added": "2026-02-19T12:01:00Z",
      "transcribed": true,
      "indexed": true
    }
  ],
  "next_source_id": 2,
  "defaults": {
    "whisper_model": "base",
    "thumbnail_interval_sec": 10,
    "render_codec": "h264",
    "render_container": "mp4"
  }
}
```

**Key points:**
- `next_source_id` ensures IDs are never reused
- `path` is always relative to project root
- Source metadata is extracted from ffprobe on `ar-edit add`

---

## 2. Transcript

Produced by `ar-edit transcribe`. This is the central data structure — everything else references into it via word indices.

```json
{
  "source_id": "src-001",
  "model": "base",
  "language": "en",
  "duration_ms": 124500,
  "segments": [
    {
      "index": 0,
      "start_ms": 0,
      "end_ms": 5230,
      "text": "Welcome to the interview today we're going to talk about",
      "words": [
        {
          "index": 0,
          "text": "Welcome",
          "start_ms": 0,
          "end_ms": 420,
          "confidence": 0.95
        },
        {
          "index": 1,
          "text": "to",
          "start_ms": 420,
          "end_ms": 540,
          "confidence": 0.97
        },
        {
          "index": 2,
          "text": "the",
          "start_ms": 540,
          "end_ms": 650,
          "confidence": 0.98
        }
      ]
    },
    {
      "index": 1,
      "start_ms": 5230,
      "end_ms": 11800,
      "text": "the impact of climate policy on regional communities",
      "words": [
        {
          "index": 9,
          "text": "the",
          "start_ms": 5230,
          "end_ms": 5400,
          "confidence": 0.96
        }
      ]
    }
  ],
  "word_count": 487
}
```

**Key points:**
- **Word indices are global and sequential across the entire transcript** (not per-segment). Word 0 is the first word of the video. Word 486 is the last.
- `start_ms` / `end_ms` give millisecond-precision boundaries for each word
- `segments` are whisper.cpp's natural utterance groupings — useful for display but the edit system operates on **word indices**
- `confidence` is whisper.cpp's per-token probability — useful for the agent to judge transcript reliability

### Mapping: whisper.cpp output → internal format

whisper.cpp's `--output-json` produces tokens with timestamps. The ingestion step:
1. Filters to word tokens only (strips special tokens like `[BLANK]`, `[SOT]`, etc.)
2. Assigns sequential global word indices
3. Converts timestamp strings ("00:00:05,230") to integer milliseconds
4. Preserves segment boundaries

---

## 3. Edit Document (Event-Sourced)

The edit document is an **append-only operation log** with a cached snapshot. Nothing is ever deleted — undo/redo moves a head pointer. This makes every edit non-destructive and fully auditable.

> **Collaborative mode (SPEC-003):** for realtime multiplayer editing the edit
> document is reformulated as a CRDT (a Loro document) so concurrent edits from
> multiple peers merge automatically. The single-writer op-log described here is
> the single-player form; the JSON structure below remains the canonical
> on-disk/agent **view** (materialised from the CRDT), so all read-side commands
> are unchanged. See [[SPEC-003-realtime-collaborative-editing]] and
> [[SPEC-003-realtime-collaborative-editing#ADR-011]] (which supersedes
> [[ADR-001-event-sourced-edits]]'s single-writer mutation model).

### Non-destructive editing principles

| Layer | Mutated by edits? | Undo strategy |
|-------|-------------------|---------------|
| Source video files | Never | N/A — immutable |
| Transcripts | Never | N/A — immutable |
| Scene index | Never (descriptions added, never overwritten) | N/A — append-only |
| Edit document | Yes — this is where undo/redo lives | Operation log + head pointer |

### Structure

```json
{
  "name": "rough-cut",
  "created": "2026-02-19T13:00:00Z",
  "next_shot_id": 6,
  "head": 4,
  "ops": [
    {
      "id": 0,
      "ts": "2026-02-19T13:00:01Z",
      "op": "add_shot",
      "shot": { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } }
    },
    {
      "id": 1,
      "ts": "2026-02-19T13:00:15Z",
      "op": "add_shot",
      "shot": { "id": "shot-002", "source": "src-003", "range": { "words": { "from": 200, "to": 280 } } }
    },
    {
      "id": 2,
      "ts": "2026-02-19T13:01:02Z",
      "op": "add_shot",
      "shot": { "id": "shot-003", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } }
    },
    {
      "id": 3,
      "ts": "2026-02-19T13:02:30Z",
      "op": "move_shot",
      "shot_id": "shot-003",
      "to_position": 1
    },
    {
      "id": 4,
      "ts": "2026-02-19T13:03:45Z",
      "op": "trim_shot",
      "shot_id": "shot-002",
      "old_range": { "words": { "from": 200, "to": 280 } },
      "new_range": { "words": { "from": 210, "to": 265 } }
    }
  ],
  "snapshot": {
    "shots": [
      { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
      { "id": "shot-003", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } },
      { "id": "shot-002", "source": "src-003", "range": { "words": { "from": 210, "to": 265 } } }
    ]
  }
}
```

### How undo/redo works

```
ops:  [0]  [1]  [2]  [3]  [4]  [5]  [6]
                                ^
                               head

ctrl-z:   head moves left  → recompute snapshot from ops[0..head]
ctrl-y:   head moves right → reapply ops[head+1]
new edit: truncate ops after head, append new op, advance head
```

- **`head`** points to the last applied operation
- **Undo (ctrl-z)**: decrement `head`, recompute `snapshot` by replaying `ops[0..=head]`
- **Redo (ctrl-y)**: increment `head` (if ops exist beyond it), reapply
- **New mutation after undo**: ops after `head` are discarded (standard undo fork behavior), new op appended
- **Snapshot** is a cache of the current state — always recomputable from `ops[0..=head]`

### Operation types

| Op | Fields | Inverse (for display) |
|----|--------|-----------------------|
| `add_shot` | `shot` (full shot data) | Remove the shot |
| `remove_shot` | `shot_id`, `shot` (full data preserved for redo) | Re-add the shot |
| `move_shot` | `shot_id`, `from_position`, `to_position` | Move back |
| `trim_shot` | `shot_id`, `old_range`, `new_range` | Restore old range |
| `replace_range_type` | `shot_id`, `old_range`, `new_range` | Restore old range |

Every destructive operation stores enough data to reverse itself. `remove_shot` preserves the full shot data in the op so it can be restored on undo.

### Agent interaction

The agent reads **only the `snapshot`** — it doesn't need to understand the operation log. When the agent writes an edit document, it can either:

1. Write a full document with ops (if it wants to preserve history)
2. Write a snapshot-only document (ops: [], head: -1) — treated as a fresh starting point

The CLI commands (`ar-edit edit add-segment`, etc.) always append to the operation log. The agent can also issue `ar-edit undo` and `ar-edit redo` as CLI commands.

**Key points:**
- **`range` is a tagged union** — exactly one of `words`, `scenes`, or `time`
  - `words` — indices into the source's transcript (requires transcript)
  - `scenes` — indices into the source's scene index (requires index with descriptions)
  - `time` — direct millisecond timestamps (always works, no prerequisites)
- Shot IDs are stable — removing shot-002 does NOT renumber shot-003
- `next_shot_id` ensures IDs are never reused
- The same source can appear in multiple shots with different range types
- You can mix speech and B-roll freely in the same edit
- **No resolved data stored** — timing/text is always resolved at runtime

### Resolved View (computed, not stored)

When you run `ar-edit edit show`, the system joins the edit document against transcripts and/or scene indices to produce:

```json
{
  "name": "rough-cut",
  "total_duration_ms": 52200,
  "shots": [
    {
      "id": "shot-001",
      "source": "src-001",
      "range_type": "words",
      "start_ms": 0,
      "end_ms": 12400,
      "duration_ms": 12400,
      "text_preview": "Welcome to the interview today we're going to talk about the impact of...",
      "scene_preview": null
    },
    {
      "id": "shot-002",
      "source": "src-003",
      "range_type": "words",
      "start_ms": 67300,
      "end_ms": 82100,
      "duration_ms": 14800,
      "text_preview": "What we found in the regional assessment was that communities...",
      "scene_preview": null
    },
    {
      "id": "shot-003",
      "source": "src-002",
      "range_type": "scenes",
      "start_ms": 0,
      "end_ms": 18000,
      "duration_ms": 18000,
      "text_preview": null,
      "scene_preview": "Aerial drone shot of coastline at sunset, waves breaking on reef"
    },
    {
      "id": "shot-005",
      "source": "src-002",
      "range_type": "time",
      "start_ms": 15000,
      "end_ms": 22000,
      "duration_ms": 7000,
      "text_preview": null,
      "scene_preview": "Close-up of coral formation underwater"
    }
  ]
}
```

**The critical insight: the edit document is just pointers. Transcripts and scene indices are the source of truth for timing and content. The resolved view merges both layers for display.**

---

## 4. Source Index

Produced by `ar-edit index`. Combines metadata, transcript summary, and thumbnail references into a searchable structure.

```json
{
  "source_id": "src-001",
  "indexed_at": "2026-02-19T12:05:00Z",
  "metadata": {
    "duration_ms": 124500,
    "resolution": [1920, 1080],
    "codec": "h264",
    "file_size_bytes": 52428800
  },
  "thumbnails": [
    {
      "path": "thumbnails/src-001_00m00s.jpg",
      "timestamp_ms": 0,
      "description": null
    },
    {
      "path": "thumbnails/src-001_00m10s.jpg",
      "timestamp_ms": 10000,
      "description": null
    },
    {
      "path": "thumbnails/src-001_00m18s.jpg",
      "timestamp_ms": 18000,
      "description": "Scene change detected"
    }
  ],
  "scene_count": 4,
  "scenes": [
    {
      "index": 0,
      "start_ms": 0,
      "end_ms": 18000,
      "thumbnail": "thumbnails/src-001_00m00s.jpg",
      "description": "Interior office, wide shot — two people seated at desk, window with city skyline"
    },
    {
      "index": 1,
      "start_ms": 18000,
      "end_ms": 45000,
      "thumbnail": "thumbnails/src-001_00m18s.jpg",
      "description": "Close-up of speaker A (Alice), bookshelf background"
    },
    {
      "index": 2,
      "start_ms": 45000,
      "end_ms": 78000,
      "thumbnail": "thumbnails/src-001_00m45s.jpg",
      "description": "Close-up of speaker B (interviewer), nodding"
    },
    {
      "index": 3,
      "start_ms": 78000,
      "end_ms": 124500,
      "thumbnail": "thumbnails/src-001_01m18s.jpg",
      "description": null
    }
  ]
}
```

**Key points:**
- `description` fields are `null` by default — designed to be populated by an LLM agent via `ar-edit index update-description`
- `scenes` are detected via ffmpeg scene change filter (`select='gt(scene,0.3)'`)
- Thumbnails are extracted at both fixed intervals AND scene changes
- The index is a read-side structure — it never affects edits or renders

### Agent-populated descriptions

An LLM agent would:
1. Read the index JSON
2. Read each thumbnail image
3. Write descriptions back: `ar-edit index set-description src-001 --timestamp 18000 --text "Interior office, two people at a desk, window with city view"`

---

## 5. Markers and Annotations

Two kinds of annotations exist: **source markers** (on raw footage) and **shot notes** (on shots in an edit). Both are designed for the human-agent feedback loop — the user marks things up, the agent reads them and acts.

### Source Markers

Stored per-source in `annotations/src-NNN.markers.json`. Created during source review — the user watches raw footage and flags the good parts.

```json
{
  "source_id": "src-001",
  "markers": [
    {
      "id": "mark-001",
      "range": { "words": { "from": 45, "to": 120 } },
      "label": "select",
      "note": "Best take of the climate answer, natural delivery",
      "created": "2026-02-19T14:00:00Z"
    },
    {
      "id": "mark-002",
      "range": { "time": { "from_ms": 62000, "to_ms": 68000 } },
      "label": "avoid",
      "note": "Audio spike, unusable",
      "created": "2026-02-19T14:01:00Z"
    },
    {
      "id": "mark-003",
      "range": { "words": { "from": 300, "to": 340 } },
      "label": "select",
      "note": "Great closing statement",
      "created": "2026-02-19T14:02:00Z"
    }
  ]
}
```

**Labels** are freeform strings. Conventional labels:

| Label | Meaning |
|-------|---------|
| `select` | Good material — use this in the edit |
| `hero` | The best take — prioritise this |
| `avoid` | Bad audio, flubbed line, technical issue — skip this |
| `maybe` | Worth reviewing but not a definite include |

Markers use the same `range` tagged union as shots (words, scenes, or time). The agent reads markers to understand the user's preferences before assembling an edit.

### Shot Notes

Stored inline in the edit document, on individual shots. Created during edit review — the user watches the assembled edit and gives feedback.

```json
{
  "id": "shot-002",
  "source": "src-003",
  "range": { "words": { "from": 200, "to": 280 } },
  "notes": [
    { "text": "Too long, trim the first half", "created": "2026-02-19T15:00:00Z" },
    { "text": "Great energy in the second sentence", "created": "2026-02-19T15:01:00Z" }
  ]
}
```

**Notes** are append-only — never deleted, only added. The agent reads them as instructions for the next revision.

### The review workflow

```
1. User watches raw footage    →  drops source markers (select, avoid, hero)
2. Agent reads markers          →  assembles edit from selects/heroes
3. User watches assembled edit  →  adds shot notes ("too long", "swap these")
4. Agent reads shot notes       →  revises the edit
5. Repeat 3–4 until done
```

### Project structure update

```
project/
├── ...
├── annotations/
│   ├── src-001.markers.json
│   ├── src-002.markers.json
│   └── src-003.markers.json
└── ...
```

---

## 6. Annotated Transcript (for text-based editing)

Exported by `ar-edit transcripts export --format editable`. This is a Markdown file with HTML comment annotations that survive text editing.

```markdown
# Source: src-001 — interview-alice.mp4

<!-- ar-edit:src-001:w0 -->Welcome<!-- /ar-edit --> <!-- ar-edit:src-001:w1 -->to<!-- /ar-edit -->
<!-- ar-edit:src-001:w2 -->the<!-- /ar-edit --> <!-- ar-edit:src-001:w3 -->interview<!-- /ar-edit -->
<!-- ar-edit:src-001:w4 -->today<!-- /ar-edit --> <!-- ar-edit:src-001:w5 -->we're<!-- /ar-edit -->
...

---

# Source: src-002 — interview-bob.mp4

<!-- ar-edit:src-002:w0 -->So<!-- /ar-edit --> <!-- ar-edit:src-002:w1 -->the<!-- /ar-edit -->
<!-- ar-edit:src-002:w2 -->first<!-- /ar-edit --> <!-- ar-edit:src-002:w3 -->thing<!-- /ar-edit -->
...
```

When the user deletes text or reorders sections, the HTML comments travel with the words. `ar-edit edit from-transcript` parses the remaining annotations to reconstruct source/word-index pairs.

**This format is verbose by design.** An alternative compact format wraps at the segment level:

```markdown
# Source: src-001 — interview-alice.mp4

<!-- ar-edit:src-001:w0-w8 -->
Welcome to the interview today we're going to talk about
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w9-w18 -->
the impact of climate policy on regional communities
<!-- /ar-edit:src-001 -->
```

The segment-level format is the default. The user edits by deleting, reordering, or splitting these blocks. If they split a block mid-sentence, `ar-edit edit from-transcript` falls back to fuzzy text matching against the transcript to resolve word boundaries.

---

## Relationships

```
manifest.json
  └── sources[] ──────────────────────┐
        │                             │
        ▼                             ▼
  src-NNN.transcript.json       src-NNN.index.json
   (words with timing)           (scenes with timing + descriptions)
        │                             │
        │                             │
        └────────────┬────────────────┘
                     │
                     ▼
               *.edit.json
                └── shots[]
                      │
                      ├── source: src-NNN
                      └── range (one of):
                            ├── words:  { from, to } ──► transcript.words[N].start_ms
                            ├── scenes: { from, to } ──► index.scenes[N].start_ms
                            └── time:   { from_ms, to_ms } (direct)
```

The edit document is a **join table**. It has no data of its own — only references. This means:
- Transcripts can be regenerated (better model) without touching edits
- Scene descriptions can be updated without touching edits
- Edits are tiny (a 100-shot edit is ~2KB)
- Validation is a simple bounds check against the relevant layer
- The agent can construct edit documents by reasoning over either transcripts, scene descriptions, or both
- A single edit can mix speech and visual sources freely

---

## Rust Type Summary

```rust
// manifest.json
struct Manifest {
    version: String,
    name: String,
    created: DateTime<Utc>,
    sources: Vec<Source>,
    next_source_id: u32,
    defaults: Defaults,
}

struct Source {
    id: String,           // "src-001"
    path: PathBuf,        // relative to project root
    original_filename: String,
    duration_ms: u64,
    video_codec: String,
    audio_codec: String,
    resolution: (u32, u32),
    frame_rate: f64,
    audio_channels: u8,
    audio_sample_rate: u32,
    added: DateTime<Utc>,
    transcribed: bool,
    indexed: bool,
}

// src-NNN.transcript.json
struct Transcript {
    source_id: String,
    model: String,
    language: String,
    duration_ms: u64,
    segments: Vec<TranscriptSegment>,
    word_count: u32,
}

struct TranscriptSegment {
    index: u32,
    start_ms: u64,
    end_ms: u64,
    text: String,
    words: Vec<Word>,
}

struct Word {
    index: u32,           // global sequential index
    text: String,
    start_ms: u64,
    end_ms: u64,
    confidence: f32,
}

// *.edit.json
struct EditDocument {
    name: String,
    created: DateTime<Utc>,
    next_shot_id: u32,
    head: i32,            // index into ops; -1 = empty
    ops: Vec<EditOp>,
    snapshot: EditSnapshot,  // cached state at head
}

struct EditSnapshot {
    shots: Vec<Shot>,
}

struct Shot {
    id: String,           // "shot-001"
    source: String,       // "src-001"
    range: ShotRange,
    notes: Vec<ShotNote>,
}

struct ShotNote {
    text: String,
    created: DateTime<Utc>,
}

/// Tagged union — exactly one variant per shot
enum ShotRange {
    /// References word indices in transcript (inclusive)
    Words { from: u32, to: u32 },
    /// References scene indices in source index (inclusive)
    Scenes { from: u32, to: u32 },
    /// Direct millisecond timestamps (no prerequisite)
    Time { from_ms: u64, to_ms: u64 },
}

struct EditOp {
    id: u32,
    ts: DateTime<Utc>,
    op: EditOpKind,
}

enum EditOpKind {
    AddShot { shot: Shot },
    RemoveShot { shot_id: String, shot: Shot },  // preserves data for undo
    MoveShot { shot_id: String, from_position: u32, to_position: u32 },
    TrimShot { shot_id: String, old_range: ShotRange, new_range: ShotRange },
}

// src-NNN.index.json
struct SourceIndex {
    source_id: String,
    indexed_at: DateTime<Utc>,
    metadata: SourceMetadata,
    thumbnails: Vec<Thumbnail>,
    scene_count: u32,
    scenes: Vec<Scene>,
}

struct Thumbnail {
    path: PathBuf,
    timestamp_ms: u64,
    description: Option<String>,
}

struct Scene {
    index: u32,           // sequential, like word indices
    start_ms: u64,
    end_ms: u64,
    thumbnail: PathBuf,
    description: Option<String>,
}

// annotations/src-NNN.markers.json
struct SourceMarkers {
    source_id: String,
    markers: Vec<Marker>,
}

struct Marker {
    id: String,           // "mark-001"
    range: ShotRange,     // reuses the same tagged union
    label: String,        // "select", "hero", "avoid", "maybe", or freeform
    note: Option<String>,
    created: DateTime<Utc>,
}
```
