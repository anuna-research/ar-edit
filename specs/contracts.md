---
title: "SPEC-001: Contract Specifications"
type: contracts
version: 1.0.0
parent: SPEC-001
---

# Contract Specifications

## CON-001: Project Management CLI

### `ar-edit init <name>`

Creates a new project directory.

```
Pre-conditions:  <name> directory does not exist
Post-conditions: Directory created with manifest.json, empty sources/, transcripts/, index/, thumbnails/, edits/ subdirectories
Exit codes:      0 = success, 1 = directory exists
Output (--json): { "project": "<name>", "path": "<abs-path>" }
```

Implements: REQ-001, REQ-002

### `ar-edit add <file>...`

Registers source video files.

```
Pre-conditions:  Inside a project directory; files exist and are readable video
Post-conditions: Files symlinked/copied to sources/; metadata extracted via ffprobe; manifest updated
Exit codes:      0 = success, 1 = file not found or not a video, 2 = ffprobe failure
Output (--json): { "sources": [{ "id": "src-001", "duration_ms": 124500, "resolution": [1920, 1080], ... }] }
```

Implements: REQ-002, REQ-003

### `ar-edit doctor`

Checks runtime dependencies.

```
Pre-conditions:  None
Post-conditions: None (read-only)
Exit codes:      0 = all deps found, 1 = missing deps
Output (--json): { "ffmpeg": { "found": true, "version": "6.1" }, "whisper": { "found": true, "path": "/usr/local/bin/whisper-cli" }, "vlc": { "found": false, "fallback": "ffplay" } }
```

---

## CON-002: Transcription CLI

### `ar-edit transcribe <source-id> [--model <model>]`

Transcribes a single source.

```
Pre-conditions:  Source registered; whisper.cpp installed; audio extractable
Post-conditions: Transcript JSON written to transcripts/<source-id>.transcript.json; manifest updated (transcribed: true)
Exit codes:      0 = success, 1 = source not found, 2 = whisper.cpp failure
Output (--json): { "source_id": "src-001", "word_count": 487, "duration_ms": 124500, "model": "base", "transcript_path": "transcripts/src-001.transcript.json" }
```

### `ar-edit transcribe --all [--model <model>] [--parallel <N>]`

Transcribes all un-transcribed sources. `--parallel` controls concurrency (default: number of CPU cores / 2).

```
Output (--json): { "transcribed": [{ "source_id": "src-001", "word_count": 487 }, ...], "skipped": ["src-002"], "failed": [] }
```

### `ar-edit transcribe <source-id> --import <file>`

Imports an existing SRT, VTT, or whisper.cpp JSON transcript.

```
Pre-conditions:  File is valid SRT/VTT/whisper JSON
Post-conditions: Converted to internal transcript JSON format
Exit codes:      0 = success, 1 = parse error
```

Implements: REQ-004, REQ-005, REQ-006, REQ-007

Verified by: TEST-004, TEST-005, TEST-006, TEST-007

---

## CON-003: Transcript Operations CLI

### `ar-edit transcripts list`

```
Output (text):   Table: source_id | duration | word_count | path
Output (--json): { "transcripts": [{ "source_id": "src-001", "duration_ms": 124500, "word_count": 487, "path": "..." }] }
```

### `ar-edit transcripts read <source-id>`

```
Output (text):   Plain text transcript with segment breaks
Output (--json): Full transcript JSON (same as the stored file)
```

### `ar-edit transcripts search <query> [--source <id>]`

```
Output (--json): { "results": [{ "source_id": "src-001", "segment_index": 3, "from_word": 45, "to_word": 52, "start_ms": 12400, "end_ms": 14200, "text": "...climate policy has had a significant...", "context_before": "...", "context_after": "..." }] }
```

### `ar-edit transcripts export --format editable [-o <file>]`

```
Output: Annotated markdown file with source/word-index HTML comments
```

Implements: REQ-008, REQ-009, REQ-010, REQ-020

Verified by: TEST-008, TEST-009, TEST-010, TEST-020

---

## CON-004: Edit Document CLI

### `ar-edit edit create <name>`

```
Post-conditions: Empty edit document at edits/<name>.edit.json with ops: [], head: -1
Output (--json): { "name": "<name>", "path": "edits/<name>.edit.json" }
```

### `ar-edit edit add-segment <edit> --source <id> (--from-word/--to-word | --from-scene/--to-scene | --from-ms/--to-ms)`

```
Post-conditions: New op appended; snapshot updated; shot ID assigned
Output (--json): { "shot_id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } }, "duration_ms": 12400 }
```

### `ar-edit edit move-segment <edit> --shot <shot-id> --position <N>`

```
Post-conditions: Move op appended; snapshot reordered
Output (--json): { "shot_id": "shot-003", "old_position": 2, "new_position": 0 }
```

### `ar-edit edit remove-segment <edit> --shot <shot-id>`

```
Post-conditions: Remove op appended (with full shot data for undo); snapshot updated
Output (--json): { "removed": "shot-002" }
```

### `ar-edit edit trim-segment <edit> --shot <shot-id> (--from-word/--to-word | --from-scene/--to-scene | --from-ms/--to-ms)`

```
Post-conditions: Trim op appended with old and new range; snapshot updated
Output (--json): { "shot_id": "shot-001", "old_range": {...}, "new_range": {...}, "old_duration_ms": 12400, "new_duration_ms": 9800 }
```

### `ar-edit edit show <edit>`

```
Output (text):   Table: shot_id | source | range | duration | text_preview/scene_preview
Output (--json): Resolved view (see DATA-MODEL.md §3 Resolved View)
```

### `ar-edit edit history <edit>`

```
Output (text):   Numbered op list with head marker (→)
Output (--json): { "head": 4, "ops": [...] }
```

### `ar-edit edit from-transcript <file> [-o <edit-name>]`

```
Pre-conditions:  Annotated markdown file with valid ar-edit annotations
Post-conditions: New edit document created from resolved annotations
Exit codes:      0 = success, 1 = parse errors (with line numbers), 3 = validation errors
```

### `ar-edit undo <edit>`

```
Post-conditions: head decremented; snapshot recomputed
Exit codes:      0 = success, 1 = nothing to undo
Output (--json): { "head": 3, "undone_op": { "id": 4, "op": "trim_shot", ... } }
```

### `ar-edit redo <edit>`

```
Post-conditions: head incremented; snapshot recomputed
Exit codes:      0 = success, 1 = nothing to redo
Output (--json): { "head": 4, "redone_op": { "id": 4, "op": "trim_shot", ... } }
```

### `ar-edit validate <edit>`

```
Pre-conditions:  Edit document exists
Post-conditions: None (read-only)
Exit codes:      0 = valid, 3 = validation errors
Output (--json): { "valid": true } or { "valid": false, "errors": [{ "shot_id": "shot-003", "error": "word index 380 exceeds transcript word_count 375 for src-001" }] }
```

### `ar-edit schema edit`

```
Output: JSON Schema for the edit document format
```

Implements: REQ-011–019, REQ-046–048

Verified by: TEST-011–019, TEST-051–053

---

## CON-005: Validation Contract

Validation checks performed by `ar-edit validate`:

| Check | Error message pattern |
|-------|----------------------|
| Source exists | `source '{id}' not found in project` |
| Transcript exists (for word range) | `source '{id}' has no transcript; cannot use word range` |
| Index exists (for scene range) | `source '{id}' has no scene index; cannot use scene range` |
| Word index in bounds | `word index {n} exceeds word_count {m} for {id}` |
| Scene index in bounds | `scene index {n} exceeds scene_count {m} for {id}` |
| Time in bounds | `time {n}ms exceeds duration {m}ms for {id}` |
| Range order | `from ({n}) must be <= to ({m}) in {shot_id}` |
| Non-zero duration | `{shot_id} has zero duration` |

Implements: REQ-017

Verified by: TEST-017

---

## CON-006: Playback CLI

### `ar-edit play <edit> [--overlay [minimal]] [--shot <shot-id>]`

```
Pre-conditions:  Edit document valid; VLC or ffplay available
Post-conditions: Video player launched as subprocess; on exit, position summary output
Exit codes:      0 = playback completed/exited, 2 = player not found
Output (--json, on exit): { "last_position_ms": 34500, "shot_id": "shot-003", "source_id": "src-002", "word_index": 85, "scene_index": 2 }
```

### `ar-edit play <source-id> --at <timecode>` or `--at-word <N>` or `--at-scene <N>`

```
Pre-conditions:  Source exists; player available
Post-conditions: Player launched at specified position
```

Implements: REQ-021, REQ-022, REQ-023, REQ-024, REQ-037

Verified by: TEST-021–024, TEST-047

---

## CON-007: Render CLI

### `ar-edit render <edit> -o <output> [--subtitles] [--burn-overlay] [--codec <c>] [--resolution <WxH>]`

```
Pre-conditions:  Edit document valid; ffmpeg available
Post-conditions: Output video file rendered
Exit codes:      0 = success, 2 = ffmpeg failure, 3 = validation error
Output (--json): { "success": true, "output": "final.mp4", "duration_ms": 95200, "file_size_bytes": 12345678 }
Progress (--json, streaming): { "progress": 0.45, "current_shot": "shot-003", "eta_seconds": 12 }
```

Implements: REQ-025, REQ-026, REQ-027, REQ-036

Verified by: TEST-025–027, TEST-042

---

## CON-008: Index and Search CLI

### `ar-edit index <source-id> [--describe] [--interval <sec>]`

```
Post-conditions: Thumbnails extracted; scene index created; manifest updated (indexed: true)
                 If --describe: description placeholder JSON written
Output (--json): { "source_id": "src-001", "scene_count": 12, "thumbnail_count": 15, "index_path": "index/src-001.index.json" }
```

### `ar-edit index --all [--describe] [--parallel <N>]`

```
Output (--json): { "indexed": [...], "skipped": [...], "failed": [...] }
```

### `ar-edit index show <source-id>`

```
Output (--json): Full index JSON
```

### `ar-edit index set-description <source-id> --scene <N> --text "<description>"`

```
Post-conditions: Scene description updated in index JSON (append-only — old value logged)
Output (--json): { "source_id": "src-001", "scene_index": 2, "description": "..." }
```

### `ar-edit search <query> [--source <id>] [--type transcript|scene|metadata]`

```
Output (--json): { "results": [{ "source_id": "src-001", "match_type": "transcript", "from_word": 45, "to_word": 52, "start_ms": 12400, "text": "..." }, { "source_id": "src-002", "match_type": "scene", "scene_index": 3, "start_ms": 45000, "description": "Aerial shot of coastline..." }] }
```

Implements: REQ-031–035

Verified by: TEST-037–041

---

## CON-009: Exit Code Contract

| Code | Meaning | Usage |
|------|---------|-------|
| 0 | Success | Command completed successfully |
| 1 | User error | Bad arguments, missing source, file not found |
| 2 | System error | ffmpeg/whisper.cpp failure, disk full, permissions |
| 3 | Validation error | Edit document failed validation |

Implements: REQ-030

Verified by: TEST-030
