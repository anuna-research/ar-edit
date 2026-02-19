---
title: "SPEC-001: Test Specifications"
type: test-specifications
version: 1.0.0
parent: SPEC-001
---

# Test Specifications

## Testing Strategy

| Layer | Type | Tool | Focus |
|-------|------|------|-------|
| Core library | Unit tests | `cargo test` | Data model operations, range resolution, validation, undo/redo, search |
| CLI commands | Integration tests | `cargo test` + `assert_cmd` | Full command execution, JSON output parsing, exit codes |
| Subprocess | Integration tests | `cargo test` + fixture files | ffmpeg/whisper.cpp invocation, output parsing |
| TUI | Manual + snapshot | `ratatui` test utilities | Layout rendering, keyboard input handling |
| End-to-end | E2E tests | Shell scripts + fixture videos | Full pipeline: init → add → transcribe → edit → render |

### Fixture Strategy

- Small test videos (2-5 seconds each, 3 sources) checked into `tests/fixtures/`
- Pre-generated whisper.cpp transcripts for deterministic testing (no whisper.cpp dependency in CI)
- Pre-generated scene indices with descriptions

---

## Unit Tests (Core Library)

### TEST-001: Project initialization

```
Given:  An empty directory
When:   project::init("test-project") is called
Then:   Directory structure created; manifest.json valid; all subdirs exist
```
Verifies: REQ-001

### TEST-002: Source registration

```
Given:  A valid project; a video file
When:   project::add_source("test.mp4") is called
Then:   Source assigned "src-001"; metadata extracted (duration, codec, resolution); manifest updated
```
Verifies: REQ-002

### TEST-003: Source validation rejects non-video

```
Given:  A text file "notes.txt"
When:   project::add_source("notes.txt") is called
Then:   Error returned: "not a valid video container"
```
Verifies: REQ-003

### TEST-004: Transcription produces valid JSON

```
Given:  A registered source with audio
When:   transcribe::run(source, model="base") is called
Then:   Transcript JSON written; word_count > 0; all words have start_ms < end_ms; word indices are sequential
```
Verifies: REQ-004

### TEST-005: Transcript word indices are globally sequential

```
Given:  A transcript with multiple segments
Then:   words[0].index == 0; words[last].index == word_count - 1; no gaps; no duplicates
```
Verifies: REQ-005

### TEST-006: Whisper model selection

```
Given:  Model flag "--model small"
When:   transcribe::run() is called
Then:   whisper.cpp invoked with "-m <path-to-small-model>"
```
Verifies: REQ-006

### TEST-007: SRT/VTT import

```
Given:  A valid SRT file with timestamps
When:   transcribe::import("subtitles.srt") is called
Then:   Transcript JSON produced with segment-level timestamps; word_count matches word count in SRT
```
Verifies: REQ-007

### TEST-008: Transcript listing

```
Given:  A project with 3 transcribed sources
When:   transcripts::list() is called
Then:   Returns 3 entries with correct source_ids, durations, word_counts
```
Verifies: REQ-008

### TEST-009: Transcript reading

```
Given:  A transcribed source "src-001"
When:   transcripts::read("src-001") is called
Then:   Returns full transcript matching stored JSON
```
Verifies: REQ-009

### TEST-010: Transcript search

```
Given:  3 transcribed sources; "climate" appears in src-001 and src-003
When:   transcripts::search("climate") is called
Then:   Returns results from src-001 and src-003 with correct word indices and context
```
Verifies: REQ-010

### TEST-011: Edit document creation

```
Given:  A valid project
When:   edit::create("my-edit") is called
Then:   Edit JSON at edits/my-edit.edit.json; ops: []; head: -1; snapshot.shots: []
```
Verifies: REQ-011

### TEST-012: Segment addition (all range types)

```
Given:  An edit document; sources with transcript and index
When:   edit::add_segment(source="src-001", range=Words(0, 52)) is called
Then:   Op appended; shot-001 in snapshot; head incremented

When:   edit::add_segment(source="src-002", range=Scenes(0, 2)) is called
Then:   Op appended; shot-002 in snapshot; head incremented

When:   edit::add_segment(source="src-002", range=Time(15000, 22000)) is called
Then:   Op appended; shot-003 in snapshot; head incremented
```
Verifies: REQ-012

### TEST-013: Segment reordering

```
Given:  Edit with shots [shot-001, shot-002, shot-003]
When:   edit::move_segment("shot-003", position=0) is called
Then:   Snapshot order: [shot-003, shot-001, shot-002]; shot IDs unchanged
```
Verifies: REQ-013

### TEST-014: Segment removal

```
Given:  Edit with shots [shot-001, shot-002]
When:   edit::remove_segment("shot-001") is called
Then:   Snapshot: [shot-002]; remove op contains full shot-001 data for undo
```
Verifies: REQ-014

### TEST-015: Segment trimming

```
Given:  shot-001 with range Words(0, 52)
When:   edit::trim_segment("shot-001", Words(10, 40)) is called
Then:   Trim op with old_range and new_range; snapshot updated
```
Verifies: REQ-015

### TEST-016: Edit document display resolves ranges

```
Given:  Edit with word-range and scene-range shots
When:   edit::show() is called
Then:   Each shot has resolved start_ms, end_ms, duration_ms, text_preview or scene_preview
```
Verifies: REQ-016

### TEST-017: Validation catches all error types

```
Test cases:
  - Source "src-999" not in project          → error
  - Word index 500 in transcript of 487      → error
  - Scene index 10 in index of 4             → error
  - Time 200000ms in source of 124500ms      → error
  - from > to                                → error
  - from == to (zero duration)               → error
  - Word range on non-transcribed source     → error
  - Scene range on non-indexed source        → error
  - Valid edit document                       → no errors
```
Verifies: REQ-017

### TEST-018: Edit from annotated transcript

```
Given:  Annotated markdown with 3 segments from 2 sources
When:   edit::from_transcript("draft.md") is called
Then:   Edit document with 3 shots; correct source/word-index pairs
```
Verifies: REQ-018

### TEST-019: Direct JSON edit document authoring

```
Given:  A hand-written JSON edit document
When:   Loaded and validated
Then:   Passes schema validation; usable for preview and render
```
Verifies: REQ-019

### TEST-020: Editable transcript export round-trip

```
Given:  3 transcribed sources
When:   Export to markdown, then re-import unchanged
Then:   Resulting edit document contains all segments in original order
```
Verifies: REQ-020

---

## Integration Tests (CLI)

### TEST-021–024: Playback commands

```
TEST-021: ar-edit play <edit> --shot shot-001 → player process launched; exit code 0
TEST-022: ar-edit play <edit> → preview rendered; player launched
TEST-023: ar-edit play <edit> --overlay → overlay text present in rendered preview (verify via ffprobe)
TEST-024: ar-edit play src-001 --at-word 50 → player launched at correct timestamp
```
Verifies: REQ-021–024

### TEST-025–027: Render commands

```
TEST-025: ar-edit render edit -o out.mp4 → output file exists; duration matches expected
TEST-026: ar-edit render edit -o out.mp4 --codec h265 → output codec is h265
TEST-027: ar-edit render edit -o out.mp4 --json → streaming progress JSON on stdout
```
Verifies: REQ-025–027

### TEST-028: Structured JSON output

```
Given:  Any command with --json
Then:   stdout is valid JSON; contains "success" or "error" field; stderr is empty on success
```
Verifies: REQ-028

### TEST-029: Idempotent read operations

```
Given:  list, show, read, search, validate commands
When:   Run twice in sequence
Then:   Output identical; project files unchanged (md5sum before == after)
```
Verifies: REQ-029

### TEST-030: Exit codes

```
Test each exit code:
  0 — successful command
  1 — ar-edit add nonexistent.mp4
  2 — render with ffmpeg path set to /nonexistent
  3 — ar-edit validate invalid-edit.json
```
Verifies: REQ-030

---

## Index and Search Tests

### TEST-037: Source indexing

```
Given:  A registered source
When:   ar-edit index src-001
Then:   Index JSON created; thumbnails extracted; scene_count > 0
```
Verifies: REQ-031

### TEST-038: Thumbnail extraction

```
Given:  A 10-second video with default 10s interval
Then:   At least 1 thumbnail; JPEG format; 640px wide; filename matches pattern
```
Verifies: REQ-032

### TEST-039: Scene description scaffolding

```
Given:  Indexed source
Then:   Each scene has description: null; path to thumbnail; valid timestamp range
```
Verifies: REQ-033

### TEST-040: Cross-index search

```
Given:  Source with transcript containing "climate"; scene with description "coastline"
When:   search("climate") → returns transcript match
When:   search("coastline") → returns scene match
When:   search("nonexistent") → returns empty results
```
Verifies: REQ-034

### TEST-041: Index summary

```
Given:  Indexed source
When:   ar-edit index show src-001
Then:   Returns scene_count, thumbnail_count, duration_ms
```
Verifies: REQ-035

### TEST-042: Subtitle embedding

```
Given:  Edit with word-range shots; --subtitles flag
When:   Rendered
Then:   Output contains subtitle stream (ffprobe confirms); subtitle timing matches word boundaries
```
Verifies: REQ-036

---

## TUI Tests

### TEST-043–050: TUI layout and interaction

```
TEST-043: TUI launches without panic on valid project
TEST-044: Edit timeline panel renders all shots with correct fields
TEST-045: Transcript panel highlights selected shot's word range
TEST-046: Source list panel shows all sources with status
TEST-047: Play key launches player subprocess
TEST-048: a/d/J/K/t keys produce correct edit operations
TEST-049: Render progress bar updates during render
TEST-050: Search (/) returns results and allows adding as shot
```
Verifies: REQ-038–045

---

## Undo/Redo Tests

### TEST-051: Undo reverts last operation

```
Given:  Edit with 3 ops, head=2
When:   undo() called
Then:   head=1; snapshot matches ops[0..=1] replayed
```
Verifies: REQ-046

### TEST-052: Redo re-applies after undo

```
Given:  Edit with head=1 after undo, ops[2] exists
When:   redo() called
Then:   head=2; snapshot matches ops[0..=2] replayed
```
Verifies: REQ-047

### TEST-053: New edit after undo forks history

```
Given:  Edit with 3 ops, head=1 after undo
When:   add_segment() called
Then:   ops[2] replaced with new op; old ops[2] discarded; head=2
```
Verifies: REQ-047

### TEST-053b: Operation history display

```
Given:  Edit with 5 ops, head=3
When:   edit::history() called
Then:   All 5 ops listed; ops[3] marked as head; ops[4] shown as "undone"
```
Verifies: REQ-048
