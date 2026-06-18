# ar-edit

A CLI video editor that enables transcript-based editing. Edit video by manipulating text transcripts rather than scrubbing timelines. Transcribe locally with whisper.cpp, preview with VLC, render with ffmpeg.

## How it works

1. **Transcribe** — whisper.cpp produces word-level timestamps for each source video
2. **Index** — ffmpeg detects scene changes and extracts thumbnails for visual context
3. **Edit** — build an edit document by selecting word ranges, scene ranges, or time ranges from sources
4. **Preview** — watch the assembled edit in VLC with optional timecode/shot ID overlay
5. **Render** — ffmpeg concatenates segments into a final video with optional subtitles

The edit document is a thin reference layer — it stores only source IDs and range pointers. Transcripts and scene indices are the source of truth for timing and content.

## Dual-mode interface

- **TUI mode** (`ar-edit tui`) — interactive ratatui terminal UI for humans
- **CLI mode** (`ar-edit <command> --json`) — structured output for LLM agents

Both modes operate on the same project files. The TUI watches for external changes so human and agent edits stay in sync.

## Usage

```bash
# Project setup
ar-edit init my-project
cd my-project
ar-edit add interview.mp4 broll.mp4
ar-edit doctor                          # check ffmpeg, whisper.cpp, VLC

# Transcription
ar-edit transcribe --all
ar-edit transcripts search "climate"

# Visual indexing
ar-edit index --all
ar-edit index set-description src-001 --scene 0 --text "Wide shot of office"

# Editing
ar-edit edit create rough-cut
ar-edit edit add-segment rough-cut --source src-001 --from-word 0 --to-word 52
ar-edit edit add-segment rough-cut --source src-002 --from-scene 0 --to-scene 2
ar-edit edit show rough-cut
ar-edit validate rough-cut

# Review
ar-edit play rough-cut --overlay
ar-edit undo rough-cut
ar-edit redo rough-cut

# Render
ar-edit render rough-cut -o final.mp4 --subtitles

# Markers (source review)
ar-edit mark src-001 --from-word 45 --to-word 120 --label select --note "Best take"
ar-edit markers --json

# Points of interest (single-moment annotations)
ar-edit poi add src-001 --at-word 45 --category highlight --note "Key statistic"
ar-edit poi add src-001 --at-ms 62500 --category issue --note "Mic bump"
ar-edit poi list src-001 --category highlight
ar-edit poi remove src-001 --id poi-001

# Text-based editing
ar-edit transcripts export --format editable -o draft.md
# ... edit the markdown ...
ar-edit edit from-transcript draft.md

# Realtime collaboration (SPEC-003, experimental)
ar-edit share                 # prints a pairing phrase, e.g. 7-saturn-pioneer
ar-edit pair 7-saturn-pioneer # join from another machine
ar-edit session status        # session + sync status
```

### Realtime collaboration (experimental)

`ar-edit share` / `ar-edit pair <phrase>` let two or more instances co-edit a
project over a direct peer-to-peer connection. The edit document is a
[CRDT](specs/concepts/CRDT.md) ([Loro](specs/concepts/Loro.md)), so concurrent
edits merge automatically; source media syncs content-addressed by
[BLAKE3](specs/concepts/BLAKE3.md). Pairing uses
[SPAKE2](specs/concepts/SPAKE2.md) over a rendezvous server, then hands off to
[iroh](specs/concepts/iroh.md).

Status: the CRDT engine, iroh transport, blob-integrity sync, delta reconnect,
presence, and rendezvous relay are implemented and tested in the
`ar-edit-collab` crate. The **live session transport is behind the
`collab-transport` build feature** (needs rustc ≥ 1.91), and the **SPAKE2
pairing is pending a mandatory security review** (ADR-009) before production
use — so `pair` currently validates the phrase and reports status rather than
opening a live session in default builds. See
[SPEC-003](specs/SPEC-003-realtime-collaborative-editing.md) and
[IMPL-003 status](plans/IMPL-003-STATUS.md).

## Shot ranges

Each shot references a source via one of three range types:

| Type | Flag | Requires | Example |
|------|------|----------|---------|
| Word range | `--from-word / --to-word` | Transcript | `--from-word 0 --to-word 52` |
| Scene range | `--from-scene / --to-scene` | Scene index | `--from-scene 0 --to-scene 2` |
| Time range | `--from-ms / --to-ms` | Nothing | `--from-ms 15000 --to-ms 22000` |

## Project structure

```
my-project/
├── manifest.json
├── sources/
│   ├── src-001.mp4
│   └── src-002.mp4
├── transcripts/
│   ├── src-001.transcript.json
│   └── src-002.transcript.json
├── index/
│   ├── src-001.index.json
│   └── src-002.index.json
├── thumbnails/
│   └── src-001_00m10s.jpg
├── edits/
│   └── rough-cut.edit.json
└── annotations/
    ├── src-001.markers.json
    └── src-001.pois.json
```

## Dependencies

| Tool | Purpose | Required |
|------|---------|----------|
| [ffmpeg](https://ffmpeg.org/) | Video processing, rendering, thumbnails | Yes |
| [whisper.cpp](https://github.com/ggerganov/whisper.cpp) | Local transcription | For transcription |
| [VLC](https://www.videolan.org/) | Preview playback | No (ffplay fallback) |

Run `ar-edit doctor` to check availability.

## Building

```bash
make build          # debug build
make release        # release build
make install        # install to ~/.cargo/bin
make test           # run all tests
make check          # fmt + clippy
make fmt-fix        # auto-format code
```

Or directly with cargo:

```bash
cargo build --release
```

## Specs

See [`specs/`](specs/) for the full specification:

- [SPEC-001](specs/SPEC-001-transcript-video-editor.md) — functional requirements
- [SPEC-002](specs/SPEC-002-points-of-interest.md) — points of interest (single-moment annotations)
- [SPEC-003](specs/SPEC-003-realtime-collaborative-editing.md) — realtime collaborative editing via p2p CRDTs
- [DATA-MODEL](specs/DATA-MODEL.md) — data structures and relationships
- [IMPL-001](specs/IMPL-001-implementation-plan.md) — implementation phases
- [IMPL-003 status](plans/IMPL-003-STATUS.md) — collaboration implementation status
- [ADR-001](specs/ADR-001-event-sourced-edits.md) — event-sourced edit document
- [ADR-002](specs/ADR-002-tagged-union-shot-ranges.md) — tagged union shot ranges
- [ADR-003](specs/ADR-003-subprocess-architecture.md) — subprocess architecture
- [ADR-004](specs/ADR-004-dual-mode-tui-cli.md) — dual-mode TUI + CLI
- [ADR-012](specs/ADR-012-cross-platform-source-linking.md) — cross-platform source linking

## License

AGPL-3.0 — see [LICENCE](LICENCE).
