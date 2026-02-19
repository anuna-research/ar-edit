---
title: "ADR-004: Dual-Mode Interface (TUI + CLI)"
type: architecture-decision-record
status: accepted
parent: SPEC-001
---

# ADR-004: Dual-Mode Interface (TUI + CLI)

## Context

The tool has two user types: a human editor who needs an interactive interface, and an LLM agent that needs structured, non-interactive I/O. These have fundamentally different interaction patterns.

## Decision

The tool operates in **two modes** from a single binary:

1. **TUI mode** (`ar-edit tui` or `ar-edit` in a project dir) — Interactive ratatui terminal UI for humans. Keyboard-driven. Panels for edit timeline, transcripts, sources, playback status.

2. **CLI mode** (all subcommands with optional `--json`) — Non-interactive commands for the agent. Each command does one thing, returns structured output, exits.

Both modes operate on the same data files. The TUI watches for file changes so that agent-made edits are picked up in real-time (and vice versa).

### Architecture

```
┌──────────────────────────────────────────────┐
│                  ar-edit binary               │
│                                               │
│  ┌─────────────┐        ┌──────────────────┐  │
│  │  CLI layer   │        │   TUI layer      │  │
│  │  (clap)      │        │   (ratatui)      │  │
│  │  --json out  │        │   keyboard input │  │
│  └──────┬───────┘        └────────┬─────────┘  │
│         │                         │             │
│         └────────┬────────────────┘             │
│                  │                              │
│         ┌────────▼────────┐                     │
│         │   Core library   │                    │
│         │   (domain logic) │                    │
│         │                  │                    │
│         │  - Project ops   │                    │
│         │  - Transcript    │                    │
│         │  - Edit document │                    │
│         │  - Index         │                    │
│         │  - Render        │                    │
│         │  - Search        │                    │
│         └────────┬─────────┘                    │
│                  │                              │
│         ┌────────▼────────┐                     │
│         │   Subprocess     │                    │
│         │   (ffmpeg,       │                    │
│         │    whisper.cpp,  │                    │
│         │    vlc)          │                    │
│         └─────────────────┘                     │
└──────────────────────────────────────────────┘
```

The **core library** contains all domain logic and is shared. The CLI and TUI are thin layers that call into core and handle I/O differently.

## Alternatives Considered

### A. TUI only, agent sends keystrokes

The agent simulates keyboard input into the TUI via a pseudo-terminal.

- **Pro**: Single interface to maintain
- **Con**: Fragile; parsing TUI output is unreliable; agent needs to handle terminal escape sequences; no structured output

### B. CLI only, no TUI

Human uses CLI commands exclusively. No interactive mode.

- **Pro**: Simplest to build; agent and human use the same interface
- **Con**: Poor editing UX for humans; no persistent view of the edit state; no real-time playback status; the whole point of ratatui is a better human experience

### C. Separate binaries (ar-edit + ar-edit-tui)

Split into two crates/binaries.

- **Pro**: Smaller binary if user only needs CLI
- **Con**: Distribution complexity; shared logic must be in a library crate anyway; feature flags achieve the same result without separate binaries

## Consequences

- The Rust project will have a workspace with at least: `ar-edit-core` (library), `ar-edit` (binary with CLI + TUI)
- TUI is gated behind a feature flag (`--features tui`) so a minimal CLI-only build is possible
- The TUI must watch the edit document file for external changes (agent edits, or manual file edits)
- File watching can use `notify` crate (cross-platform filesystem events)
- The core library must be fully synchronous or use tokio for async subprocess management
- Both modes share identical validation, rendering, and search logic

## Trace

- REQ-028 (Structured JSON Output)
- REQ-038 (TUI Launch)
- REQ-039–045 (TUI Layout and Interaction)
