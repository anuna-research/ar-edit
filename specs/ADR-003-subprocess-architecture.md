---
title: "ADR-003: Subprocess Architecture for ffmpeg and whisper.cpp"
type: architecture-decision-record
status: accepted
parent: SPEC-001
---

# ADR-003: Subprocess Architecture for ffmpeg and whisper.cpp

## Context

The tool depends on ffmpeg (video processing, thumbnail extraction, rendering, overlay) and whisper.cpp (transcription). Both are mature C/C++ projects with CLI interfaces. We need to decide how to integrate them from Rust.

## Decision

Use **subprocess invocation** (`std::process::Command`) for both ffmpeg and whisper.cpp. Parse their stdout/stderr for progress and results. Do not link against their C libraries.

### ffmpeg invocation pattern

```
ffmpeg -i <source> -ss <start> -to <end> -c copy <output>     # segment extraction
ffmpeg -i <source> -vf "select='gt(scene,0.3)'" -vsync vfr    # scene detection
ffmpeg -i <source> -vf "drawtext=text='shot-003'..." <output>  # overlay
ffmpeg -f concat -i filelist.txt -c copy <output>              # concatenation
```

### whisper.cpp invocation pattern

```
whisper-cli -m <model> -f <audio> --output-json --print-progress  # transcription
```

Audio is pre-extracted from video via ffmpeg before passing to whisper.cpp.

## Alternatives Considered

### A. FFI bindings to libavcodec/libavformat

Link directly against ffmpeg's C libraries via Rust FFI.

- **Pro**: No subprocess overhead; finer-grained control; no need to parse CLI output
- **Con**: Massive API surface; unsafe Rust required; version coupling; build complexity (linking against system ffmpeg); less portable; ffmpeg's API is notoriously difficult to use correctly

### B. PyAV / MoviePy via embedded Python

Embed a Python runtime and use PyAV or MoviePy for video operations.

- **Pro**: Well-documented Python video editing APIs
- **Con**: Python runtime dependency defeats the purpose of a Rust binary; performance overhead; complexity of embedding

### C. GStreamer

Use GStreamer's pipeline model via gstreamer-rs bindings.

- **Pro**: Proper pipeline architecture; Rust bindings exist
- **Con**: Heavy dependency; overkill for our use case (we need cut + concat + overlay, not real-time pipeline processing); less ubiquitous than ffmpeg

## Consequences

- ffmpeg and whisper.cpp are runtime dependencies that must be installed separately
- The tool should detect their presence on startup and report clear errors if missing (`ar-edit doctor`)
- Progress parsing requires understanding ffmpeg's stderr output format (frame count, time, speed)
- whisper.cpp's `--print-progress` flag provides progress on stderr
- Subprocess approach is inherently more portable — works with any ffmpeg/whisper.cpp version
- Commands are logged for debugging and reproducibility
- The tool is effectively an **orchestrator** — it composes ffmpeg and whisper.cpp commands from the data model

## Trace

- REQ-004 (Transcription via whisper.cpp)
- REQ-025 (Final Render)
- REQ-032 (Thumbnail Extraction)
- REQ-023 (Overlay)
