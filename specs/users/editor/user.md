# User: Video Editor

| Field | Value |
|-------|-------|
| Archetype | Video Editor |
| Document | User Profile: Video Editor |
| Version | 1.0.0 |

## Role

A content creator, journalist, or video producer who works with multiple video sources and needs to assemble edits quickly using transcripts rather than scrubbing timelines manually.

## Goals

1. **Primary**: Assemble a final cut from multiple source videos by editing their transcripts as text
2. **Secondary**: Preview edits quickly before committing to a full render
3. **Tertiary**: Iterate rapidly — try different arrangements of transcript segments without re-rendering each time

## Constraints

- **Technical proficiency**: Comfortable with CLI tools and terminal workflows; not necessarily a professional video editor
- **Environment**: macOS, Linux, or Windows 10+ workstation with ffmpeg, VLC, and whisper.cpp installed locally (platform support is specified in [[SPEC-001-transcript-video-editor#NFR-015]])
- **Accessibility**: Relies on text-based interfaces; may use screen readers or keyboard-only navigation
- **Hardware**: Has a machine capable of running whisper.cpp models (at minimum the `base` model); may not have a GPU
- **Time pressure**: Wants to produce edits in minutes, not hours; transcript-based editing is chosen precisely because it is faster than timeline scrubbing

## Daily Workflow

1. Imports 1-5 video files into the tool
2. Runs transcription on each video (or uses pre-existing transcripts)
3. Reviews transcripts, identifying the segments they want to keep
4. Assembles a new "edit document" by selecting and reordering transcript segments from multiple sources
5. Previews individual clips or the full assembly in VLC
6. Renders the final output video
7. Iterates: adjusts the edit document and re-renders as needed

## Pain Points (Current State)

- Scrubbing video timelines to find the right moment is slow
- Cutting between multiple sources in a traditional NLE requires managing complex timelines
- No existing open-source tool provides the "edit as text" paradigm from tools like Descript
- Coordinating timestamps manually between transcripts and ffmpeg commands is error-prone

## Success Criteria

- Can go from raw video files to a rendered multi-source edit in under 10 manual steps
- Never has to manually calculate or type a timestamp — all timing is derived from transcript word boundaries
- Can preview any segment before rendering the full output
