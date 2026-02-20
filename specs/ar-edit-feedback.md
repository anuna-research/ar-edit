# ar-edit CLI Feedback Report

**Project:** spindle-intro-demo
**Date:** 2026-02-20
**ar-edit version:** installed via cargo (`/Users/anuna-02/.cargo/bin/ar-edit`)

---

## Summary

Used `ar-edit` to build a 3-shot edit from scratch: initialized sources, indexed scenes, added/trimmed/annotated segments, tested undo/redo, validated, and rendered to MP4. The core workflow is solid and the tool is pleasant to use. Found several bugs and areas for improvement documented below.

---

## Bugs

### 1. `add-segment` accepts invalid data without error (Severity: High)

All three of the following commands succeed silently — no error is raised at creation time:

```bash
# Non-existent source
ar-edit edit add-segment spindle-intro --source src-999 --from-ms 0 --to-ms 1000
# => Added shot-004 to 'spindle-intro'

# Time range exceeds source duration (src-001 is 265s)
ar-edit edit add-segment spindle-intro --source src-001 --from-ms 500000 --to-ms 600000
# => Added shot-005 to 'spindle-intro'

# Inverted time range (from > to)
ar-edit edit add-segment spindle-intro --source src-001 --from-ms 5000 --to-ms 3000
# => Added shot-006 to 'spindle-intro'
```

`ar-edit validate` does catch all of these after the fact, but invalid segments should be rejected eagerly at creation time rather than allowing the edit document to enter an invalid state. This is especially important because a user may not run `validate` before `render`.

**Recommendation:** Run validation checks inside `add-segment` (and `trim-segment`) before writing the operation. Return a non-zero exit code with a clear error message.

---

### 2. Error messages are printed twice (Severity: Medium)

Every error message is duplicated in stderr output. Examples:

```
Error: whisper model 'ggml-base.bin' not found in: ...
Error: whisper model 'ggml-base.bin' not found in: ...
```

```
Error: No such file or directory (os error 2)
  Hint: check that edit 'nonexistent' exists in the edits/ directory
Error: No such file or directory (os error 2)
  Hint: check that edit 'nonexistent' exists in the edits/ directory
```

```
Error: position out of bounds: 99 (max: 2)
Error: position out of bounds: 99 (max: 2)
```

This appears to be a systematic issue — likely the error is being printed both by the subcommand handler and by the top-level main error handler. Every error path tested exhibited this behavior.

**Recommendation:** Ensure only one layer prints the error. Likely fix: if the subcommand already prints/formats the error, the top-level handler should not re-print it (or vice versa).

---

### 3. `add` command output shows double-prefixed source IDs (Severity: Low)

When adding sources, the CLI output displays `src-src-001` instead of `src-001`:

```
Added source src-src-001: 2026-02-18 16-53-32.mkv
Added source src-src-002: 2026-02-18 17-00-32.mkv
Added source src-src-003: 2026-02-18 17-13-32.mkv
```

The manifest.json correctly stores `src-001`. This is purely a display bug in the `add` command's success message — likely a format string like `"src-{id}"` where `id` already includes the `src-` prefix.

---

### 4. Scene detection produces zero-duration scenes (Severity: Low)

After indexing `src-001`, the scene list includes scenes with 0.0s duration:

```
Scene 4: 128.1s - 128.1s (0.0s)  (no description)
Scene 13: 263.7s - 263.8s (0.0s)  (no description)
```

Zero-duration scenes are degenerate and likely represent noise in the scene detection algorithm. They could confuse downstream operations like `--from-scene` / `--to-scene` addressing.

**Recommendation:** Either merge zero-duration scenes with an adjacent scene, or filter them out with a minimum duration threshold (e.g., 0.5s).

---

### 5. Negative millisecond values cause confusing argument parsing error (Severity: Low)

```bash
ar-edit edit trim-segment spindle-intro --shot shot-001 --from-ms -100 --to-ms 5000
```

Produces:
```
error: unexpected argument '-1' found
  tip: to pass '-1' as a value, use '-- -1'
```

The error refers to `-1` instead of `-100`, and the suggested workaround (`-- -1`) doesn't apply well here since there are multiple flags. The clap parser interprets `-100` as a flag.

**Recommendation:** Use `clap`'s `allow_negative_numbers = true` or `allow_hyphen_values = true` on the relevant arguments, or validate and reject negative values with a clear error message.

---

## Areas for Improvement

### 1. `doctor` should suggest installation commands

`ar-edit doctor` reports missing dependencies but doesn't suggest how to install them:

```
Dependencies:
  ffmpeg 8.0.1
  ffprobe 8.0.1
  whisper-cli MISSING
  vlc missing (fallback: ffplay)
```

**Recommendation:** Add platform-specific install hints, e.g.:
```
  whisper-cli MISSING  (install: brew install whisper-cpp)
  vlc missing          (install: brew install --cask vlc)
```

### 2. `transcribe --all` error message could be more actionable

When the whisper model is missing, the error says:
```
Error: whisper model 'ggml-base.bin' not found in: ~/.cache/whisper, ...
```

It would help to include a download command or URL, e.g.:
```
Hint: download with: whisper-cli --download-model base
```

### 3. Total duration display in `edit show`

The `edit show` output lists individual shot durations but does not display the total edit duration. Adding a summary line would be helpful:

```
Total: 3 shots, 26:08.000
```

### 4. `validate` could be run automatically before `render`

Since `render` depends on a valid edit, it would be a good safety check to run validation automatically before starting the render pipeline, rather than relying on the user to run `validate` manually.

### 5. `play` re-renders from scratch even when a render already exists

Running `ar-edit play spindle-intro` builds a full preview file from scratch in a temp directory before launching ffplay, even if a rendered MP4 already exists. For a ~26-minute edit this adds unnecessary wait time.

**Recommendation:** If a recent render output exists (or cache the preview), reuse it. Alternatively, accept a `--file` flag to play an existing render directly, or render only if the edit has changed since the last preview.

### 6. `play` does not pass `--resolution` through to the preview render

The play command always renders at full resolution (1920x1080) for the preview. For a quick preview, rendering at a lower resolution (e.g., 720p) would be significantly faster.

**Recommendation:** Add a `--resolution` flag to `play`, or default the preview render to a lower resolution.

### 7. `edit history` could show operation details

The history output shows operation type and shot ID but not what changed:

```
3  trim_shot  shot-001  2026-02-20 01:17:12
```

Showing the old and new values would make the history more useful:
```
3  trim_shot  shot-001  [0ms..265011ms] -> [2000ms..260000ms]  2026-02-20 01:17:12
```

---

## What Works Well

- **Core editing workflow** is intuitive and fast — create, add, trim, note, show, validate, render all chain together cleanly
- **Undo/redo** works correctly and the history display with the `→` pointer is helpful
- **JSON output mode** (`--json`) is well-structured and consistent across commands
- **Source management** properly normalizes filenames and extracts media metadata on add
- **Scene indexing** is fast and produces useful scene boundaries
- **Markers** system is clean and the labeling concept (select, avoid, review) is well thought out
- **Validation** catches a comprehensive set of issues (missing sources, out-of-range times, inverted ranges)
- **Rendering** works correctly and produces a proper MP4 output
- **Error messages** (aside from being duplicated) are generally clear and include helpful hints

---

## Environment Notes

- macOS Darwin 25.2.0
- ffmpeg 8.0.1 / ffprobe 8.0.1
- whisper-cpp: not installed (transcription not tested)
- VLC: not installed (playback not tested, ffplay available as fallback)
