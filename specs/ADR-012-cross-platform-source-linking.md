---
title: "ADR-012: Cross-Platform Source Linking"
type: architecture-decision-record
status: proposed
parent: SPEC-001
---

# ADR-012: Cross-Platform Source Linking

## Context

[[SPEC-001-transcript-video-editor#REQ-002]] registers source videos under the
project's `sources/` directory, and [[DATA-MODEL]] §Project structure describes
these as "symlinks or copies of source videos". A symlink keeps the project
small (source media — often gigabytes — is not duplicated) while preserving
[[SPEC-001-transcript-video-editor#NFR-005]] portability via relative paths.

Adding Windows to the platform matrix
([[SPEC-001-transcript-video-editor#NFR-015]]) breaks the assumption that
symlinks are cheap and always available:

- On macOS and Linux, an unprivileged process can create symlinks freely.
- On **Windows**, creating a symbolic link historically requires the
  `SeCreateSymbolicLinkPrivilege` — granted only to elevated processes or to
  ordinary users when [[Developer Mode]] is enabled. A tool that assumes
  symlink creation will fail for a typical, non-elevated Windows user.

The same media file must also resolve identically when a project is copied
between machines of different operating systems (NFR-005), so whatever is
written into `sources/` must not encode a platform-specific path form.

## Decision

Choose the source-linking mechanism per platform, in this order of preference,
and record the chosen mechanism in the manifest entry so behaviour is explicit
and auditable:

1. **macOS / Linux:** create a **symbolic link** into `sources/`.
2. **Windows with [[Developer Mode]] (or elevated):** create a symbolic link
   (parity with Unix).
3. **Windows without privilege, same volume:** create a [[hard link]] — no byte
   duplication, no privilege required, transparent to ffmpeg/whisper/VLC.
4. **Otherwise (cross-volume, or hard link unsupported):** **copy** the file.

The default is never "fail". An explicit `--copy` flag forces a copy on any
platform (useful when the user wants the project fully self-contained for
archival). The manifest records `link_mode: "symlink" | "hardlink" | "copy"`
per source so `ar-edit doctor` can report it and so portability expectations are
visible.

All four mechanisms leave a usable, relative reference under `sources/`, so the
project remains copyable per [[SPEC-001-transcript-video-editor#NFR-005]].

## Alternatives Considered

| Option | Pros | Cons |
|--------|------|------|
| Always copy (all platforms) | Simplest; fully self-contained; trivially portable | Duplicates multi-GB media; defeats the "thin project" property; slow `add` |
| Always symlink | Smallest projects | Fails for unprivileged Windows users; cross-volume symlinks brittle |
| **Per-platform fallback (chosen)** | Works for every user without elevation; keeps projects thin where possible; explicit `link_mode` | Slightly more branching in the `add` path; `link_mode` must be tested per platform |

## Consequences

- [[SPEC-001-transcript-video-editor#REQ-002]] and `CON-001` gain a
  platform-conditional clause; the `--json` output of `ar-edit add` SHALL
  include `link_mode`.
- A test asserts the Windows-without-privilege path falls back without error
  ([[test-specs#TEST-118]]).
- The collaboration source sync of
  [[SPEC-003-realtime-collaborative-editing#REQ-075]] is unaffected: replicated
  media is written as real bytes (or hard-linked from the content store) on the
  receiving peer, independent of how the *originating* peer linked it.
- No symlink path form is ever persisted in the manifest (paths stay relative),
  preserving NFR-005 cross-OS copyability.

## Trace

- [[SPEC-001-transcript-video-editor#REQ-002]]
- [[SPEC-001-transcript-video-editor#NFR-005]]
- [[SPEC-001-transcript-video-editor#NFR-015]]
- [[test-specs#TEST-118]]
