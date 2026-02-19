---
title: "IMPL-001: Implementation Plan"
type: implementation-plan
version: 1.0.0
parent: SPEC-001
---

# IMPL-001: Implementation Plan

## Crate Structure

```
ar-edit/
├── Cargo.toml              # workspace root
├── crates/
│   ├── ar-edit-core/       # library: all domain logic
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── project.rs      # manifest, source registration, doctor
│   │   │   ├── transcript.rs   # whisper.cpp invocation, JSON parsing, import
│   │   │   ├── edit.rs         # edit document, ops, undo/redo, validation
│   │   │   ├── index.rs        # scene detection, thumbnails, descriptions
│   │   │   ├── search.rs       # cross-layer search (transcript + scene)
│   │   │   ├── render.rs       # ffmpeg render pipeline, overlay, subtitles
│   │   │   ├── playback.rs     # VLC/ffplay subprocess launch
│   │   │   ├── resolve.rs      # range resolution (word→time, scene→time)
│   │   │   └── models/
│   │   │       ├── mod.rs
│   │   │       ├── manifest.rs
│   │   │       ├── transcript.rs
│   │   │       ├── edit_document.rs
│   │   │       ├── source_index.rs
│   │   │       └── shot_range.rs
│   │   └── Cargo.toml
│   └── ar-edit/            # binary: CLI + TUI
│       ├── src/
│       │   ├── main.rs
│       │   ├── cli/
│       │   │   ├── mod.rs      # clap definitions
│       │   │   ├── project.rs  # init, add, doctor
│       │   │   ├── transcribe.rs
│       │   │   ├── transcripts.rs
│       │   │   ├── edit.rs
│       │   │   ├── play.rs
│       │   │   ├── render.rs
│       │   │   ├── index.rs
│       │   │   └── search.rs
│       │   └── tui/
│       │       ├── mod.rs      # ratatui app loop
│       │       ├── app.rs      # app state
│       │       ├── panels/
│       │       │   ├── timeline.rs
│       │       │   ├── transcript.rs
│       │       │   ├── sources.rs
│       │       │   └── status.rs
│       │       ├── input.rs    # keyboard handling
│       │       └── events.rs   # file watcher, subprocess events
│       └── Cargo.toml
├── tests/
│   ├── fixtures/
│   │   ├── test-video-a.mp4    # 3-5 second test videos
│   │   ├── test-video-b.mp4
│   │   ├── test-video-c.mp4
│   │   ├── src-001.transcript.json  # pre-generated
│   │   ├── src-002.transcript.json
│   │   └── src-001.index.json
│   ├── integration/
│   │   ├── test_project.rs
│   │   ├── test_transcribe.rs
│   │   ├── test_edit.rs
│   │   ├── test_render.rs
│   │   └── test_search.rs
│   └── e2e/
│       └── test_full_pipeline.sh
└── specs/                  # this directory
```

## Dependencies

### ar-edit-core

| Crate | Purpose |
|-------|---------|
| `serde` + `serde_json` | JSON serialization for all data models |
| `chrono` | Timestamps in manifests and ops |
| `thiserror` | Structured error types |
| `which` | Detect ffmpeg, whisper.cpp, VLC paths |
| `regex` | Transcript search, SRT/VTT parsing |

### ar-edit (binary)

| Crate | Purpose |
|-------|---------|
| `clap` (derive) | CLI argument parsing |
| `ratatui` + `crossterm` | Terminal UI |
| `notify` | Filesystem watching (TUI picks up external edits) |
| `indicatif` | Progress bars (CLI mode) |
| `tokio` | Async subprocess management (render progress, parallel transcribe) |
| `anyhow` | Error handling in binary |

## Implementation Phases

### Phase A: Foundation (data models + project management)

Delivers: `ar-edit init`, `ar-edit add`, `ar-edit doctor`

1. **Workspace setup** — Cargo workspace with `ar-edit-core` and `ar-edit` crates
2. **Data models** — All serde structs: Manifest, Source, Transcript, EditDocument, Shot, ShotRange, EditOp, SourceIndex, Scene, Thumbnail
3. **Project operations** — `init`, `add` (with ffprobe metadata extraction), `doctor` (dependency detection)
4. **CLI skeleton** — clap subcommand structure, `--json` flag threading, exit code handling
5. **Unit tests** — TEST-001, TEST-002, TEST-003, TEST-030

**Deliverable**: A CLI that can create projects and register video sources.

### Phase B: Transcription pipeline

Delivers: `ar-edit transcribe`, `ar-edit transcripts list/read/search`

1. **Audio extraction** — ffmpeg subprocess to extract WAV from video
2. **whisper.cpp invocation** — Subprocess, model selection, progress parsing
3. **Transcript ingestion** — Parse whisper.cpp JSON → internal format with global word indices
4. **SRT/VTT import** — Parse and convert to internal format
5. **Transcript operations** — list, read, search
6. **Unit tests** — TEST-004 through TEST-010

**Deliverable**: Full transcription pipeline. Can transcribe videos and search transcripts.

### Phase C: Edit document + undo/redo

Delivers: `ar-edit edit create/add-segment/move-segment/remove-segment/trim-segment/show/history`, `ar-edit validate`, `ar-edit undo`, `ar-edit redo`

1. **Edit document operations** — Create, add, move, remove, trim (all append ops to log)
2. **Snapshot recomputation** — Replay ops[0..=head] to produce snapshot
3. **Undo/redo** — Head pointer manipulation with fork behavior
4. **Range resolution** — Resolve word/scene/time ranges to millisecond timestamps via transcripts/indices
5. **Validation** — All checks from CON-005
6. **Edit display** — Resolved view with text/scene previews
7. **Unit tests** — TEST-011 through TEST-019, TEST-051 through TEST-053

**Deliverable**: Full edit document lifecycle. Can create, manipulate, validate, undo/redo edits.

### Phase D: Index + search

Delivers: `ar-edit index`, `ar-edit index show`, `ar-edit index set-description`, `ar-edit search`

1. **Scene detection** — ffmpeg `select='gt(scene,0.3)'` subprocess
2. **Thumbnail extraction** — ffmpeg frame extraction at scene changes + intervals
3. **Index creation** — SourceIndex JSON with scenes and thumbnails
4. **Description scaffolding** — Null descriptions ready for agent population
5. **Cross-layer search** — Unified search across transcripts and scene descriptions
6. **Unit tests** — TEST-037 through TEST-041

**Deliverable**: Sources can be visually indexed. Agent can populate descriptions. Search spans both layers.

### Phase E: Playback + preview

Delivers: `ar-edit play`, preview with overlay

1. **Player detection** — VLC → ffplay fallback chain
2. **Segment playback** — Launch player at start/end timestamps
3. **Full edit playback** — Render lightweight preview, launch player
4. **Overlay rendering** — ffmpeg drawtext filter for timecode/shot-id/source
5. **Pause-and-feedback output** — Structured JSON on player exit
6. **Integration tests** — TEST-021 through TEST-024

**Deliverable**: Can watch edits in VLC with overlay. Pause-and-feedback workflow works.

### Phase F: Render pipeline

Delivers: `ar-edit render`, subtitle embedding

1. **Segment extraction** — ffmpeg `-ss -to` for each shot
2. **Concatenation** — ffmpeg concat demuxer
3. **Codec handling** — Copy when possible, re-encode when needed
4. **Progress reporting** — Parse ffmpeg stderr for frame/time progress
5. **Subtitle generation** — SRT from transcript word timestamps, embed via ffmpeg
6. **Integration tests** — TEST-025 through TEST-027, TEST-042

**Deliverable**: Can render final output with subtitles.

### Phase G: Annotated transcript round-trip

Delivers: `ar-edit transcripts export --format editable`, `ar-edit edit from-transcript`

1. **Export** — Generate annotated markdown with segment-level annotations
2. **Import** — Parse edited markdown, resolve annotations to source/word pairs
3. **Fuzzy matching** — Handle split/merged blocks by text matching
4. **Round-trip test** — TEST-018, TEST-020

**Deliverable**: Edit-as-text workflow works end to end.

### Phase H: TUI

Delivers: `ar-edit tui`

1. **App skeleton** — ratatui event loop, crossterm terminal setup/restore
2. **Timeline panel** — Shot list with selection, scrolling
3. **Transcript panel** — Full transcript with shot range highlighting
4. **Source list panel** — Source status overview
5. **Status bar** — Playback position, render progress
6. **Keyboard input** — a/d/J/K/t/p/Enter/ctrl-z/ctrl-y///q
7. **File watcher** — Detect external changes to edit document
8. **Search mode** — Inline search with result selection
9. **Manual testing** — TEST-043 through TEST-050

**Deliverable**: Full interactive TUI.

### Phase I: E2E testing + polish

1. **E2E test script** — Full pipeline: init → add → transcribe → edit → render
2. **Error message polish** — Consistent formatting, actionable suggestions
3. **Shell completions** — clap generate for bash/zsh/fish
4. **Schema command** — `ar-edit schema edit` outputs JSON Schema

**Deliverable**: Production-ready CLI.

## Dependency Graph

```
Phase A ──► Phase B ──► Phase C ──► Phase E ──► Phase F ──► Phase I
                 │                    ▲
                 └──► Phase D ────────┘
                                      │
              Phase C ──► Phase G     │
                                      │
              Phase C ──► Phase H ────┘
```

- A is prerequisite for everything
- B and C can partially overlap (data models from A enable both)
- D depends on B (needs transcripts for search integration)
- E depends on C + D (needs edit documents and indices)
- F depends on E (render builds on the same ffmpeg pipeline as preview)
- G depends on C + B
- H depends on C (needs edit operations) and benefits from E (playback integration)
- I depends on everything
