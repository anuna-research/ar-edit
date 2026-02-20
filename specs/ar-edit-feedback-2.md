# ar-edit CLI — Comprehensive Test Results

**Project:** spindle-intro-demo
**Date:** 2026-02-20
**Method:** Systematic testing of every command, subcommand, and flag across 13 phases
**Tests executed:** ~318 test cases across all 19 top-level commands + subcommands
**Project state:** All baseline checksums verified before and after — no project files were modified

---

## Summary Statistics

| Category | Count |
|----------|-------|
| Test cases executed | ~318 |
| New bugs found | 10 |
| New improvement areas | 7 |
| Original bugs verified fixed | 3 of 5 |
| Original improvements already done | 3 of 7 |

---

## Verification of Original Bugs

### Bug #1 (add-segment accepts invalid data): FIXED

All three originally-reported cases now produce proper errors:

```bash
ar-edit edit add-segment test-plan-edit --source src-999 --from-ms 0 --to-ms 1000
# => Error: source 'src-999' not found in manifest

ar-edit edit add-segment test-plan-edit --source src-001 --from-ms 500000 --to-ms 600000
# => Error: from_ms (500000) exceeds source duration (265011ms)

ar-edit edit add-segment test-plan-edit --source src-001 --from-ms 10000 --to-ms 5000
# => Error: from (10000ms) must be <= to (5000ms)
```

Zero-duration ranges are also now rejected: `Error: range has zero duration`

`trim-segment` has the same fixes: inverted ranges and out-of-range times are properly rejected.

### Bug #2 (double error messages): FIXED

Systematic `sort | uniq -c` check across 11 error-producing commands found **zero duplicate error lines**. Every tested error now appears exactly once. Commands tested:
- `edit show nonexistent`, `edit history nonexistent`, `validate nonexistent`
- `index show nonexistent`, `markers nonexistent`, `render nonexistent`
- `edit remove-segment --shot shot-999`, `edit move-segment --position 99`
- `undo nonexistent`, `redo` past end, `transcribe nonexistent-source`

### Bug #3 (double-prefixed source IDs in add): NOT RE-VERIFIED

Could not re-test `add` without modifying the project's source list. Recommend manual verification.

### Bug #4 (zero-duration scenes): STILL PRESENT

`index show src-001` still shows zero-duration scenes in the scene list.

### Bug #5 (negative ms confusing error): FIXED

Negative values now produce a clear error:

```bash
ar-edit edit trim-segment test-plan-edit --shot shot-001 --from-ms -100 --to-ms 5000
# => Error: must be non-negative

ar-edit mark src-001 --from-ms -100 --to-ms 5000 --label select
# => Error (exit code 1, properly rejected)
```

---

## Verification of Original Improvement Suggestions

| # | Suggestion | Status |
|---|-----------|--------|
| 1 | `doctor` install hints | **Already implemented** — now shows `Install: brew install whisper-cpp` etc. |
| 2 | `transcribe` model download hint | Not verified (whisper not installed) |
| 3 | Total duration in `edit show` | **Already implemented** — now shows `Total: 3 shots, 00:26:04.500` |
| 4 | Auto-validate before render | Not verified |
| 5 | `play` re-renders from scratch | **Still present** — confirmed play renders full preview every time |
| 6 | `play --resolution` | **Fixed** — `play --resolution 640x360` correctly renders at specified resolution |
| 7 | `edit history` details | **Still absent** — history still shows only op type + shot ID |

---

## New Bugs Found

### 6. `mark` accepts invalid data without validation (Severity: High)

The `mark` command has the same class of validation bugs that `add-segment` originally had (Bug #1), but `mark` has **not** been fixed. All of the following succeed with exit code 0:

```bash
# Nonexistent source — silently creates annotations/nonexistent.markers.json
ar-edit mark nonexistent --from-ms 0 --to-ms 1000 --label select --note "Bad source"

# Inverted time range (from > to)
ar-edit mark src-001 --from-ms 30000 --to-ms 10000 --label select --note "Inverted"

# Out-of-range times (src-001 duration is 265011ms)
ar-edit mark src-001 --from-ms 500000 --to-ms 600000 --label select --note "Out of range"

# Scene numbers out of range
ar-edit mark src-001 --from-scene 999 --to-scene 1000 --label select --note "Bad scene"

# Zero-duration range
ar-edit mark src-001 --from-ms 5000 --to-ms 5000 --label select --note "Zero dur"

# Empty label string
ar-edit mark src-001 --from-ms 10000 --to-ms 20000 --label '' --note "Empty label"

# Arbitrary invalid label value (not select/avoid/review)
ar-edit mark src-001 --from-ms 100000 --to-ms 110000 --label invalid_label --note "Bad label"
```

**Recommendation:** Apply the same validation that was added to `add-segment` — check source existence, time range bounds, range ordering, and consider restricting labels to valid enum values.

---

### 7. `markers` listing crashes when word-range markers exist without transcript (Severity: High)

Creating a word-range marker succeeds even when the source has no transcript:

```bash
ar-edit mark src-001 --from-word 0 --to-word 10 --label select --note "Word range"
# => Created mark-006 (exit 0)
```

But then listing markers crashes:

```bash
ar-edit markers src-001
# => Error: transcript required to resolve words range
```

This makes the **entire markers file unreadable** until the offending marker is manually deleted from the JSON file. The `mark` command should either:
1. Reject word-range markers when no transcript exists (preferred), or
2. The `markers` listing should render word-range markers without resolving them

---

### 8. `play --overlay` is broken (Severity: Medium)

```bash
ar-edit play spindle-intro --overlay
# => Error: ffmpeg failed: segment encoding failed: Error opening output files: Invalid argument

ar-edit play spindle-intro --overlay minimal
# => Same error
```

The overlay feature triggers ffmpeg encoding that fails. Both `--overlay` and `--overlay minimal` produce the same error.

---

### 9. `from-transcript` silently accepts invalid input (Severity: Medium)

All of the following create an edit with 0 shots and exit with code 0 (success):

```bash
# Plain text (not JSON)
echo "not json" > /tmp/bad.txt
ar-edit edit from-transcript /tmp/bad.txt
# => Created edit 'bad' (0 shots)

# Empty file
touch /tmp/empty.json
ar-edit edit from-transcript /tmp/empty.json
# => Created edit 'empty' (0 shots)

# Valid JSON with wrong structure
echo '{"foo": "bar"}' > /tmp/wrong.json
ar-edit edit from-transcript /tmp/wrong.json
# => Created edit 'wrong' (0 shots)
```

**Recommendation:** At minimum, warn the user when no segments are extracted from the input file. Ideally, validate that the file matches the expected annotated transcript format and error on unrecognized formats.

---

### 10. `render` passes invalid codec to ffmpeg without pre-validation (Severity: Low)

```bash
ar-edit render spindle-intro --output /tmp/test.mp4 --codec invalid_codec
# => Rendering 'spindle-intro' (3 shots) to /tmp/test.mp4 [codec: invalid_codec]...
# => Error: ffmpeg failed: segment extraction failed (exit code: Some(8))
```

The codec value is passed directly to ffmpeg without validation. The error message also leaks a Rust `Debug` representation (`Some(8)`) rather than a user-friendly message.

**Recommendation:** Validate codec against a known-valid list (h264, h265, etc.) before starting the render. Format the exit code as just `8` not `Some(8)`.

---

### 11. `index --interval 0` and `--parallel 0` accepted without validation (Severity: Low)

Both `--interval 0` and `--parallel 0` are accepted as valid values and cause the indexing process to run indefinitely or extremely slowly (hit 30-second timeout in testing).

**Recommendation:** Reject zero and negative values for both `--interval` and `--parallel` with a clear error message.

---

### 12. `edit create` and `note` accept empty strings (Severity: Low)

```bash
ar-edit edit create ''
# => Creates an edit with empty name (exit 0)

ar-edit edit note test-plan-edit --shot shot-001 --text ""
# => Adds a note with empty text (exit 0)
```

Similarly, `ar-edit init ''` creates a project named "untitled" but with a blank directory path in the output message.

**Recommendation:** Reject empty strings for edit names and note text.

---

### 13. `play` silently ignores inapplicable flags (Severity: Low)

When playing an edit (not a source), the `--at`, `--at-scene`, and `--at-word` flags are all silently ignored with no warning:

```bash
ar-edit play spindle-intro --at invalid    # no error, renders normally
ar-edit play spindle-intro --at-scene 999  # no error, renders normally
ar-edit play spindle-intro --at-word 0     # no error, renders normally
```

**Recommendation:** Either validate these flags and warn when they don't apply, or restrict them to source playback context only.

---

### 14. No `--version` flag (Severity: Low)

```bash
ar-edit --version
# => error: unexpected argument '--version' found
```

`--version` is a standard CLI convention. Currently there is no way to check the installed version.

---

### 15. `render` error message for nonexistent output directory is misleading (Severity: Low)

```bash
ar-edit render spindle-intro --output /nonexistent/dir/output.mp4
# => Rendering 'spindle-intro' (3 shots) to /nonexistent/dir/output.mp4...
# => Error: Read-only file system (os error 30)
```

The render starts (printing the "Rendering..." line) before discovering the output path is invalid. The error "Read-only file system" is the OS error for macOS `/` but doesn't clearly communicate that the directory doesn't exist.

**Recommendation:** Validate the output directory exists before starting the render pipeline.

---

## New Improvement Areas

### 8. Add marker deletion capability

There is no command to delete individual markers. The only way to remove a marker is to manually edit the JSON file.

```bash
ar-edit mark --help     # no --delete flag
ar-edit markers --help  # no delete/remove subcommand
```

**Recommendation:** Add `ar-edit markers delete <source> --id <marker-id>` or similar.

### 9. `undo`/`redo` are top-level commands, not `edit` subcommands

`undo` and `redo` are invoked as `ar-edit undo <edit>` and `ar-edit redo <edit>`, not `ar-edit edit undo <edit>`. This is counter-intuitive since all other edit-mutating operations are subcommands of `ar-edit edit` (`add-segment`, `trim-segment`, `move-segment`, `note`, `remove-segment`, `create`).

```bash
ar-edit edit undo spindle-intro   # => error: unrecognized subcommand 'undo'
ar-edit undo spindle-intro        # => works
```

**Recommendation:** Either move `undo`/`redo` under `ar-edit edit` for consistency, or add aliases so both forms work.

### 10. `trim-segment` requires both `--from-ms` and `--to-ms`

Partial trims (adjusting only one end of a segment) are not supported:

```bash
ar-edit edit trim-segment test-plan-edit --shot shot-002 --from-ms 6000
# => error: required arguments were not provided: --to-ms

ar-edit edit trim-segment test-plan-edit --shot shot-002 --to-ms 18000
# => error: required arguments were not provided: --from-ms
```

**Recommendation:** Allow partial trims where only `--from-ms` or `--to-ms` is specified, keeping the other end unchanged.

### 11. `edit list` command doesn't exist

There is no command to list all edit documents in the project:

```bash
ar-edit edit list
# => error: unrecognized subcommand 'list'
#    tip: a similar subcommand exists: 'history'
```

Currently the only way to see available edits is `ls edits/`.

**Recommendation:** Add `ar-edit edit list` to enumerate all edit documents in the `edits/` directory.

### 12. `search ''` (empty query) matches everything

An empty search query returns all metadata rather than producing an error:

```bash
ar-edit search ''
# => Returns marker metadata for mark-001
```

This may be intentional but is surprising behavior.

### 13. `render --output` should validate directory existence before rendering

The render command starts the full rendering pipeline before discovering that the output path doesn't exist. For a long render, this wastes significant time.

### 14. `transcripts export` usage is unclear

```bash
ar-edit transcripts export src-001
# => error: unexpected argument 'src-001' found
```

The `export` subcommand does not accept a positional source argument. Its interface isn't clear from `--help`.

---

## Observations (Non-Actionable Notes)

- **`--json` flag is flexible:** Works in all positions — at end of command, before subcommand args, and as a global flag (`ar-edit --json edit show`). All produce valid JSON.
- **All JSON outputs validate:** Every `--json` flag tested produces parseable JSON, including error cases (which return `{"error": "..."}` objects).
- **All read-only commands are idempotent:** Repeated runs of `edit show`, `index show`, `markers`, and `doctor` produce identical output.
- **Commands outside project directory fail gracefully:** Most commands error with clear messages when run outside a project. `doctor` works anywhere (expected).
- **`init` can nest inside an already-initialized project:** Running `init` inside an existing project creates a subdirectory rather than erroring.
- **`init` with very long names (>255 chars):** Properly fails with OS filename limit error.
- **`init` with special characters:** Accepted — `"test project with spaces!"` creates a valid project.
- **Model validation on `transcribe`:** `--model nonexistent` properly rejects with `invalid whisper model 'nonexistent': expected one of tiny, base, small, medium, large`.
- **`--import` format detection:** Properly checks file extension and rejects unsupported formats like `.txt`, and empty extensions (`/dev/null`).
- **`play --shot`:** Works correctly — plays a specific shot directly via ffplay without needing to render the full edit.
- **`play --resolution`:** Works correctly — the resolution flag is now properly passed to the preview renderer.
- **`tui`:** Cannot start without a TTY device (shows "Device not configured"), which is expected for a terminal UI.
- **Undo/redo work correctly:** Full cycle tested — undo to empty, redo to restore, history fork (undo + new op truncates redo history), `--json` output for both.
- **`edit show` now includes total duration:** Shows `Total: 3 shots, 00:26:04.500` at the bottom.
- **`doctor` now includes install hints:** Shows `Install: brew install whisper-cpp` for missing dependencies.

---

## Test Matrix Summary

| Phase | Area | Tests | Pass | Fail/Issue | Notes |
|-------|------|-------|------|------------|-------|
| 0 | Baseline checksums | 6 | 6 | 0 | All checksums verified before and after |
| 1 | Help/doctor/schema/completions | 52 | 50 | 2 | No `--version` flag; `edit undo` is top-level |
| 2 | Read-only project inspection | 49 | 47 | 2 | `edit list` doesn't exist; `search ''` matches all |
| 3 | Transcription error paths | 18 | 18 | 0 | All error paths produce clear messages |
| 4 | Mutating edit operations | 80 | 76 | 4 | Empty name/text accepted; partial trims not supported |
| 5 | Mutating marker operations | 24 | 14 | 10 | Major: no input validation; markers crash on word-range |
| 6 | Index operations | 22 | 18 | 4 | `--interval 0`/`--parallel 0` accepted; negative args |
| 7 | Init and add | 11 | 10 | 1 | Empty string init creates "untitled" |
| 8 | Edit from-transcript | 7 | 4 | 3 | Silently accepts invalid input |
| 9 | Play | 16 | 10 | 6 | Overlay broken; flags silently ignored |
| 10 | Render | 10 | 7 | 3 | Invalid codec not pre-validated; leaks Rust debug |
| 11 | Cross-cutting concerns | 20 | 20 | 0 | All JSON valid; no double errors; idempotent |
| 12 | TUI | 3 | 2 | 1 | Expected: no TTY in test environment |
| **Total** | | **~318** | **~282** | **~36** | |
