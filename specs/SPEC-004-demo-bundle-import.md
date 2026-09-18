---
id: SPEC-004
title: "SPEC-004: Demo Bundle Import"
type: specification
version: 1.0.0
status: draft
---

# SPEC-004: Demo Bundle Import

## Overview

A **demo bundle** is what `ar-crawl session --record` (or `replay --record`)
writes when an LLM agent drives a web app while being filmed: a raw, silent
browser screencast plus the structured data an editor needs to turn it into a
product demo. ar-crawl's job ends at capture — it composites nothing. ar-edit's
job starts at the bundle: this spec makes a recorded demo editable **as text**,
exactly like an interview, without transcription, because the narration was
written by the agent while it drove.

```
bundle/
├── video.webm       raw screencast at viewport × scale, no audio, no cursor drawn
├── manifest.json    per-step startMs/endMs, narration `title`, selector, success
├── cursor.json      cursor event log (move/ripple/hide/show), ms from video start,
│                    viewport CSS px
└── recording.json   Chrome DevTools Recorder JSON, replayable by ar-crawl
```

`ar-edit demo import <dir>` maps the bundle onto ar-edit's existing layers:

| Bundle data | ar-edit layer | Why |
|---|---|---|
| `video.webm` | `Source` (silent, REQ-003) | The footage |
| step `title`s + timings | `Transcript` | Narration is what is *said*; word ranges, search and subtitles work unchanged |
| visual step starts | `SourceIndex` scenes | What is *shown* changes at each action; better than pixel scene-detect on screen footage |
| titled steps | `Marker` (range) | Chapter navigation with `ar-edit markers` |
| every step | `Poi` (point) | Categorised landmarks; failed steps become `issue`s |
| `cursor.json` | `annotations/<id>.cursor.json` | Kept verbatim for the (future) cursor compositor |

### Relationship to Existing Concepts

Depends on REQ-003 as amended (silent sources), REQ-049 markers, SPEC-002
POIs, ADR-015 (annotations live in the per-source CRDT store), and the scene
index (REQ-0xx `index`). Compositing the cursor onto the video and voiceover
generation are **out of scope** (see Open Questions).

---

## Functional Requirements

### Bundle Recognition

**REQ-092: Bundle Validation**

The system SHALL recognise a directory as a demo bundle WHEN it contains a
`manifest.json` of version 1 that names an existing video file, WITH a
structured error naming the missing piece otherwise (`not a demo bundle`,
`bundle video not found`, `unsupported bundle version`) and no change to the
project before validation succeeds. `cursor.json` is optional (a bundle
recorded with `--no-cursor`).

Trace:
- TEST-122
- CON-019

### Source Registration

**REQ-093: Screencast as Silent Source**

The system SHALL register the bundle's video as a project source through the
ordinary `add` path WHEN importing, WITH the source flagged `transcribed` and
`indexed` on completion so `transcribe --all` and `index --all` do not
reprocess it. The video normally has no audio stream (REQ-003 permits this).

Trace:
- TEST-123
- REQ-003

### Narration

**REQ-094: Narration Transcript**

The system SHALL derive a transcript from the titled steps WHEN importing,
WITH one segment per titled step whose text is the title, and words spread
evenly across the segment so word ranges resolve. A step's narration SHALL
span from its `startMs` to the `startMs` of the **next titled step** (or the
end of the video) — not to its own `endMs` — because the hold after an action
is where the viewer sees its result. Untitled steps in between are absorbed;
zero-duration `marker` steps thereby get a real span. Blank titles are not
narration. The transcript's `model` SHALL be `ar-crawl-demo`.

Trace:
- TEST-124

### Scenes

**REQ-095: Step Scene Index**

The system SHALL build the source's scene index from the steps WHEN importing,
WITH scenes partitioning the timeline at the start of each **visual** step
(every type except `marker` and `cursor`), a leading untitled scene when the
first action starts after 0 ms, each scene described by its step's title, and
one thumbnail extracted at each scene start. No ffmpeg scene detection runs.

Trace:
- TEST-125

### Annotations

**REQ-096: Markers and Points of Interest**

The system SHALL create, through the per-source annotation store (ADR-015):
(a) one **marker** per narration span, ranged in time, labelled with the
title, noted with the underlying action (`click #submit`, `goto https://…`);
and (b) one **POI** at the start of every step except `cursor`, categorised
as: navigation types → `transition`; `marker` → `note`; click/keyboard/input
types → `cue`; hover/scroll/zoom → `highlight`; anything else → `note`; and
**any failed step → `issue`** regardless of type. Both SHALL carry the local
author (REQ-091).

Trace:
- TEST-126
- SPEC-002

### Cursor Log

**REQ-097: Cursor Log Preservation**

The system SHALL copy the bundle's cursor log to
`annotations/<source-id>.cursor.json` WHEN present, byte-for-byte in meaning
(same events, `tMs`, coordinates in viewport CSS px, `scale`), so a later
compositor can draw the cursor at output resolution without re-recording.

Trace:
- TEST-127

---

## Contract Specifications

### CON-019: Demo Import CLI

```
ar-edit demo import <dir>

Pre-conditions:  Inside a project; <dir> is a demo bundle (REQ-092)
Post-conditions: New source registered (silent); transcripts/<id>.transcript.json,
                 index/<id>.index.json + thumbnails, annotations/<id>.annot.json
                 (markers + POIs), annotations/<id>.cursor.json written;
                 manifest flags transcribed = indexed = true
Flags:           --json (summary), --dry-run (report counts, write nothing)
Exit codes:      0 = success; non-zero with a structured message when <dir> is
                 not a bundle or ffmpeg/ffprobe fail
Output (--json): { "source_id", "video", "duration_ms", "silent", "segments",
                   "words", "scenes", "markers", "pois", "cursor_events" }
```

Implements: REQ-092 … REQ-097

---

## Test Specifications

### TEST-122: Non-bundle refused before touching the project

```
Given:  A directory with no manifest.json
When:   ar-edit demo import <dir>
Then:   Non-zero exit, stderr contains "not a demo bundle", manifest unchanged
```
Verifies: REQ-092

### TEST-123: Screencast registered as a silent, pre-flagged source

```
Given:  A bundle whose video has no audio stream
When:   ar-edit demo import <dir>
Then:   src-001 exists, audio_channels = 0, transcribed = indexed = true
```
Verifies: REQ-093

### TEST-124: Narration spans to the next titled step

```
Given:  Steps goto[0–1000]"Open", marker[1000]"Sorted", click[1500–1800]"Create",
        hover[2000–2100] (untitled), duration 3000
When:   imported
Then:   3 segments; segment 2 spans 1000–1500; segment 3 spans 1500–3000;
        word ranges over the transcript resolve into shots
```
Verifies: REQ-094

### TEST-125: Scenes at visual steps with thumbnails

```
Given:  the steps above
When:   imported
Then:   scenes (0,1500) (1500,2000) (2000,3000); marker/cursor start none;
        scene 0 described "Open"; a thumbnail file exists per scene
```
Verifies: REQ-095

### TEST-126: Markers and categorised POIs

```
Given:  the steps above, hover failed
When:   imported
Then:   3 markers labelled with the titles, click marker noted "click #b";
        4 POIs: transition, note, cue, issue; authored by the local author
```
Verifies: REQ-096

### TEST-127: Cursor log preserved

```
Given:  a bundle with a 3-event cursor.json
When:   imported
Then:   annotations/src-001.cursor.json has the same 3 events and scale
```
Verifies: REQ-097

---

## Purity Boundary Map

`ar-edit-core::demo` is pure over the bundle: it reads the bundle and derives
transcript, scene boundaries, marker specs and POI specs as values.
`index::build_index_from_scenes` performs the only media I/O (thumbnails).
The CLI (`cmd_demo_import`) owns project writes and the annotation store.

---

## Traceability Matrix

```
REQ-092 → TEST-122 → CON-019
REQ-093 → TEST-123
REQ-094 → TEST-124
REQ-095 → TEST-125
REQ-096 → TEST-126
REQ-097 → TEST-127
```

---

## Ambiguity Log

| Original Term | Resolution |
|---------------|------------|
| "narration for a step" | From the step's start to the next *titled* step's start, not to the step's own `endMs` |
| "visual step" | Any step type except `marker` and `cursor` |
| "failed step" | `success: false` in the manifest → POI category `issue` whatever the step type |
| "preserved" (cursor log) | Same events and values; re-serialised by ar-edit, so whitespace may differ |

---

## Open Questions

| # | Question | Status |
|---|----------|--------|
| 1 | Cursor compositing: a `build_cursor_filter` next to `overlay.rs` turning `cursor.json` into an ffmpeg overlay chain (position lerp / ripple / fade expressions), applied at render. | Deferred — next spec; the log is preserved for it |
| 2 | Voiceover: the narration transcript is already a script; TTS clips aligned to segment starts as a second source. | Deferred |
| 3 | Re-import after re-filming (`ar-crawl replay --record`): should the importer update the existing source in place rather than register a new one? | Deferred — pacing is baked into the recording, so timings are stable across re-films; update-in-place is the natural follow-up |
