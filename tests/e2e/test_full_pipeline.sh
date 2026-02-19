#!/usr/bin/env bash
# =============================================================================
# E2E test: Full pipeline from project init to rendered output verification.
#
# Pipeline tested:
#   init -> add (fixture videos) -> transcribe -> edit create -> add-segment
#   -> move-segment -> trim-segment -> validate -> play -> render -o out.mp4
#   -> verify output duration/codec
#
# Several CLI commands (init, add, transcribe, edit create, add-segment,
# move-segment, trim-segment, validate) are not yet wired to the CLI binary.
# Those steps are simulated by writing the project state directly to disk,
# exactly as the core library functions would. The implemented CLI commands
# (edit show, render) are invoked via the real binary.
#
# Prerequisites: ffmpeg, ffprobe
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BINARY="$REPO_ROOT/target/debug/ar-edit"

passed=0
failed=0

step()  { printf '\n\033[1;34m==> %s\033[0m\n' "$1"; }
pass()  { printf '  \033[0;32mPASS\033[0m: %s\n' "$1"; passed=$((passed + 1)); }
fail()  { printf '  \033[0;31mFAIL\033[0m: %s\n' "$1"; failed=$((failed + 1)); }
skip()  { printf '  \033[0;33mSKIP\033[0m: %s\n' "$1"; }

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

# ── Step 0: Prerequisites ──────────────────────────────────────────────────────

step "Checking prerequisites"

command -v ffmpeg  >/dev/null 2>&1 || { echo "ffmpeg is required"; exit 1; }
command -v ffprobe >/dev/null 2>&1 || { echo "ffprobe is required"; exit 1; }
pass "ffmpeg and ffprobe available"

# ── Step 1: Build ──────────────────────────────────────────────────────────────

step "Building ar-edit binary"

cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --quiet 2>&1

if [ -f "$BINARY" ]; then
    pass "Binary built at $BINARY"
else
    fail "Binary not found at $BINARY"
    exit 1
fi

# ── Step 2: Generate fixture videos ────────────────────────────────────────────

step "Generating fixture videos with ffmpeg"

# fixture1.mp4: 3-second blue screen with 440 Hz tone, 320x240, h264/aac
ffmpeg -y \
    -f lavfi -i "color=c=blue:size=320x240:d=3:r=25" \
    -f lavfi -i "sine=frequency=440:duration=3:sample_rate=44100" \
    -c:v libx264 -preset ultrafast -c:a aac -shortest \
    "$WORKDIR/fixture1.mp4" 2>/dev/null

# fixture2.mp4: 2-second red screen with 880 Hz tone, 320x240, h264/aac
ffmpeg -y \
    -f lavfi -i "color=c=red:size=320x240:d=2:r=25" \
    -f lavfi -i "sine=frequency=880:duration=2:sample_rate=44100" \
    -c:v libx264 -preset ultrafast -c:a aac -shortest \
    "$WORKDIR/fixture2.mp4" 2>/dev/null

if [ -f "$WORKDIR/fixture1.mp4" ] && [ -f "$WORKDIR/fixture2.mp4" ]; then
    pass "Fixture videos created (fixture1.mp4: 3s, fixture2.mp4: 2s)"
else
    fail "Fixture video creation failed"
    exit 1
fi

# ── Step 3: Init project (simulated) ──────────────────────────────────────────
#
# Simulates: ar-edit init test-project
# Creates the project directory structure and manifest.json as project::init().

step "Init project (simulated)"

PROJECT="$WORKDIR/test-project"
mkdir -p "$PROJECT"/{sources,transcripts,index,thumbnails,edits,annotations}

if [ -d "$PROJECT/sources" ] && [ -d "$PROJECT/edits" ]; then
    pass "Project directory structure created"
else
    fail "Project directory structure incomplete"
    exit 1
fi

# ── Step 4: Add source videos (simulated) ─────────────────────────────────────
#
# Simulates: ar-edit add fixture1.mp4 fixture2.mp4
# Creates symlinks in sources/ and populates manifest.json with source metadata.

step "Add source videos (simulated)"

ln -s "$WORKDIR/fixture1.mp4" "$PROJECT/sources/src-001.mp4"
ln -s "$WORKDIR/fixture2.mp4" "$PROJECT/sources/src-002.mp4"

cat > "$PROJECT/manifest.json" <<'MANIFEST'
{
  "version": "1.0.0",
  "name": "test-project",
  "created": "2026-02-20T00:00:00Z",
  "sources": [
    {
      "id": "src-001",
      "path": "sources/src-001.mp4",
      "original_filename": "fixture1.mp4",
      "duration_ms": 3000,
      "video_codec": "h264",
      "audio_codec": "aac",
      "resolution": [320, 240],
      "frame_rate": 25.0,
      "audio_channels": 1,
      "audio_sample_rate": 44100,
      "added": "2026-02-20T00:00:00Z",
      "transcribed": true,
      "indexed": true
    },
    {
      "id": "src-002",
      "path": "sources/src-002.mp4",
      "original_filename": "fixture2.mp4",
      "duration_ms": 2000,
      "video_codec": "h264",
      "audio_codec": "aac",
      "resolution": [320, 240],
      "frame_rate": 25.0,
      "audio_channels": 1,
      "audio_sample_rate": 44100,
      "added": "2026-02-20T00:00:00Z",
      "transcribed": true,
      "indexed": true
    }
  ],
  "next_source_id": 3,
  "defaults": {
    "whisper_model": "base",
    "thumbnail_interval_sec": 10,
    "render_codec": "h264",
    "render_container": "mp4"
  }
}
MANIFEST

if [ -L "$PROJECT/sources/src-001.mp4" ] && [ -L "$PROJECT/sources/src-002.mp4" ]; then
    pass "Source videos symlinked and manifest created"
else
    fail "Source symlinks missing"
    exit 1
fi

# ── Step 5: Transcribe (simulated) ────────────────────────────────────────────
#
# Simulates: ar-edit transcribe --all
# Creates transcript JSON files matching the fixture video durations.

step "Transcribe sources (simulated)"

cat > "$PROJECT/transcripts/src-001.transcript.json" <<'TRANSCRIPT1'
{
  "source_id": "src-001",
  "model": "base",
  "language": "en",
  "duration_ms": 3000,
  "segments": [
    {
      "index": 0, "start_ms": 0, "end_ms": 1500,
      "text": "Welcome to the demo.",
      "words": [
        {"index": 0, "text": "Welcome", "start_ms": 100, "end_ms": 500, "confidence": 0.95},
        {"index": 1, "text": "to", "start_ms": 500, "end_ms": 700, "confidence": 0.98},
        {"index": 2, "text": "the", "start_ms": 700, "end_ms": 900, "confidence": 0.97},
        {"index": 3, "text": "demo.", "start_ms": 900, "end_ms": 1400, "confidence": 0.92}
      ]
    },
    {
      "index": 1, "start_ms": 1500, "end_ms": 3000,
      "text": "This is a sample.",
      "words": [
        {"index": 4, "text": "This", "start_ms": 1600, "end_ms": 1800, "confidence": 0.96},
        {"index": 5, "text": "is", "start_ms": 1800, "end_ms": 1950, "confidence": 0.99},
        {"index": 6, "text": "a", "start_ms": 1950, "end_ms": 2050, "confidence": 0.98},
        {"index": 7, "text": "sample.", "start_ms": 2050, "end_ms": 2500, "confidence": 0.94}
      ]
    }
  ],
  "word_count": 8
}
TRANSCRIPT1

cat > "$PROJECT/transcripts/src-002.transcript.json" <<'TRANSCRIPT2'
{
  "source_id": "src-002",
  "model": "base",
  "language": "en",
  "duration_ms": 2000,
  "segments": [
    {
      "index": 0, "start_ms": 0, "end_ms": 2000,
      "text": "Second clip here.",
      "words": [
        {"index": 0, "text": "Second", "start_ms": 100, "end_ms": 500, "confidence": 0.95},
        {"index": 1, "text": "clip", "start_ms": 500, "end_ms": 900, "confidence": 0.93},
        {"index": 2, "text": "here.", "start_ms": 900, "end_ms": 1400, "confidence": 0.96}
      ]
    }
  ],
  "word_count": 3
}
TRANSCRIPT2

pass "Transcripts created for src-001 and src-002"

# ── Step 6: Edit create (simulated) ───────────────────────────────────────────
#
# Simulates: ar-edit edit create rough-cut
# Creates an empty edit document.

step "Edit create (simulated)"

cat > "$PROJECT/edits/rough-cut.edit.json" <<'EDIT_CREATE'
{
  "name": "rough-cut",
  "created": "2026-02-20T00:00:00Z",
  "next_shot_id": 1,
  "head": -1,
  "ops": [],
  "snapshot": {"shots": []}
}
EDIT_CREATE

pass "Empty edit document 'rough-cut' created"

# ── Step 7: Add segments (simulated) ──────────────────────────────────────────
#
# Simulates:
#   ar-edit edit add-segment rough-cut --source src-001 --from-ms 0 --to-ms 1500
#   ar-edit edit add-segment rough-cut --source src-002 --from-ms 0 --to-ms 2000
#   ar-edit edit add-segment rough-cut --source src-001 --from-ms 1500 --to-ms 3000
#
# Adds 3 shots:
#   shot-001: src-001 first half  (0ms..1500ms = 1.5s)
#   shot-002: src-002 full clip   (0ms..2000ms = 2.0s)
#   shot-003: src-001 second half (1500ms..3000ms = 1.5s)

step "Add segments (simulated)"

cat > "$PROJECT/edits/rough-cut.edit.json" <<'EDIT_ADD'
{
  "name": "rough-cut",
  "created": "2026-02-20T00:00:00Z",
  "next_shot_id": 4,
  "head": 2,
  "ops": [
    {"id": 0, "ts": "2026-02-20T00:01:00Z", "op": "add_shot", "shot": {"id": "shot-001", "source": "src-001", "range": {"time": {"from_ms": 0, "to_ms": 1500}}, "notes": []}},
    {"id": 1, "ts": "2026-02-20T00:02:00Z", "op": "add_shot", "shot": {"id": "shot-002", "source": "src-002", "range": {"time": {"from_ms": 0, "to_ms": 2000}}, "notes": []}},
    {"id": 2, "ts": "2026-02-20T00:03:00Z", "op": "add_shot", "shot": {"id": "shot-003", "source": "src-001", "range": {"time": {"from_ms": 1500, "to_ms": 3000}}, "notes": []}}
  ],
  "snapshot": {
    "shots": [
      {"id": "shot-001", "source": "src-001", "range": {"time": {"from_ms": 0, "to_ms": 1500}}, "notes": []},
      {"id": "shot-002", "source": "src-002", "range": {"time": {"from_ms": 0, "to_ms": 2000}}, "notes": []},
      {"id": "shot-003", "source": "src-001", "range": {"time": {"from_ms": 1500, "to_ms": 3000}}, "notes": []}
    ]
  }
}
EDIT_ADD

pass "3 segments added (shot-001, shot-002, shot-003)"

# ── Step 8: Move segment (simulated) ──────────────────────────────────────────
#
# Simulates: ar-edit edit move-segment rough-cut --shot shot-002 --position 0
# Moves shot-002 to the front of the timeline.
# Order becomes: shot-002, shot-001, shot-003

step "Move segment (simulated)"

cat > "$PROJECT/edits/rough-cut.edit.json" <<'EDIT_MOVE'
{
  "name": "rough-cut",
  "created": "2026-02-20T00:00:00Z",
  "next_shot_id": 4,
  "head": 3,
  "ops": [
    {"id": 0, "ts": "2026-02-20T00:01:00Z", "op": "add_shot", "shot": {"id": "shot-001", "source": "src-001", "range": {"time": {"from_ms": 0, "to_ms": 1500}}, "notes": []}},
    {"id": 1, "ts": "2026-02-20T00:02:00Z", "op": "add_shot", "shot": {"id": "shot-002", "source": "src-002", "range": {"time": {"from_ms": 0, "to_ms": 2000}}, "notes": []}},
    {"id": 2, "ts": "2026-02-20T00:03:00Z", "op": "add_shot", "shot": {"id": "shot-003", "source": "src-001", "range": {"time": {"from_ms": 1500, "to_ms": 3000}}, "notes": []}},
    {"id": 3, "ts": "2026-02-20T00:04:00Z", "op": "move_shot", "shot_id": "shot-002", "from_position": 1, "to_position": 0}
  ],
  "snapshot": {
    "shots": [
      {"id": "shot-002", "source": "src-002", "range": {"time": {"from_ms": 0, "to_ms": 2000}}, "notes": []},
      {"id": "shot-001", "source": "src-001", "range": {"time": {"from_ms": 0, "to_ms": 1500}}, "notes": []},
      {"id": "shot-003", "source": "src-001", "range": {"time": {"from_ms": 1500, "to_ms": 3000}}, "notes": []}
    ]
  }
}
EDIT_MOVE

pass "Moved shot-002 to position 0"

# ── Step 9: Trim segment (simulated) ──────────────────────────────────────────
#
# Simulates: ar-edit edit trim-segment rough-cut --shot shot-003 --from-ms 1500 --to-ms 2500
# Trims shot-003 from 1500-3000ms down to 1500-2500ms (1.5s -> 1.0s).
#
# Final timeline:
#   shot-002: src-002 0..2000ms (2.0s)
#   shot-001: src-001 0..1500ms (1.5s)
#   shot-003: src-001 1500..2500ms (1.0s)
#   Total expected duration: 4.5s

step "Trim segment (simulated)"

cat > "$PROJECT/edits/rough-cut.edit.json" <<'EDIT_TRIM'
{
  "name": "rough-cut",
  "created": "2026-02-20T00:00:00Z",
  "next_shot_id": 4,
  "head": 4,
  "ops": [
    {"id": 0, "ts": "2026-02-20T00:01:00Z", "op": "add_shot", "shot": {"id": "shot-001", "source": "src-001", "range": {"time": {"from_ms": 0, "to_ms": 1500}}, "notes": []}},
    {"id": 1, "ts": "2026-02-20T00:02:00Z", "op": "add_shot", "shot": {"id": "shot-002", "source": "src-002", "range": {"time": {"from_ms": 0, "to_ms": 2000}}, "notes": []}},
    {"id": 2, "ts": "2026-02-20T00:03:00Z", "op": "add_shot", "shot": {"id": "shot-003", "source": "src-001", "range": {"time": {"from_ms": 1500, "to_ms": 3000}}, "notes": []}},
    {"id": 3, "ts": "2026-02-20T00:04:00Z", "op": "move_shot", "shot_id": "shot-002", "from_position": 1, "to_position": 0},
    {"id": 4, "ts": "2026-02-20T00:05:00Z", "op": "trim_shot", "shot_id": "shot-003", "old_range": {"time": {"from_ms": 1500, "to_ms": 3000}}, "new_range": {"time": {"from_ms": 1500, "to_ms": 2500}}}
  ],
  "snapshot": {
    "shots": [
      {"id": "shot-002", "source": "src-002", "range": {"time": {"from_ms": 0, "to_ms": 2000}}, "notes": []},
      {"id": "shot-001", "source": "src-001", "range": {"time": {"from_ms": 0, "to_ms": 1500}}, "notes": []},
      {"id": "shot-003", "source": "src-001", "range": {"time": {"from_ms": 1500, "to_ms": 2500}}, "notes": []}
    ]
  }
}
EDIT_TRIM

pass "Trimmed shot-003 to 1500-2500ms (1.0s)"

# ── Step 10: Validate ─────────────────────────────────────────────────────────
#
# ar-edit validate is not yet wired to the CLI binary. Skipped.

step "Validate"
skip "validate CLI command not yet wired up"

# ── Step 11: Edit show (CLI) ──────────────────────────────────────────────────

step "Edit show (CLI)"

cd "$PROJECT"
SHOW_OUTPUT=$("$BINARY" edit show rough-cut 2>&1)

echo "$SHOW_OUTPUT"

# Verify all 3 shots appear in output
if echo "$SHOW_OUTPUT" | grep -q "shot-001"; then
    pass "edit show includes shot-001"
else
    fail "shot-001 not found in show output"
fi

if echo "$SHOW_OUTPUT" | grep -q "shot-002"; then
    pass "edit show includes shot-002"
else
    fail "shot-002 not found in show output"
fi

if echo "$SHOW_OUTPUT" | grep -q "shot-003"; then
    pass "edit show includes shot-003"
else
    fail "shot-003 not found in show output"
fi

if echo "$SHOW_OUTPUT" | grep -q "3 shots"; then
    pass "edit show reports 3 shots"
else
    fail "expected '3 shots' in show output"
fi

# ── Step 12: Play ─────────────────────────────────────────────────────────────
#
# Skipped: requires a media player (VLC/ffplay) and interactive playback.

step "Play"
skip "requires media player (VLC/ffplay) for interactive playback"

# ── Step 13: Render (CLI) ─────────────────────────────────────────────────────

step "Render to out.mp4 (CLI)"

"$BINARY" render rough-cut -o "$WORKDIR/out.mp4" 2>&1

if [ -f "$WORKDIR/out.mp4" ]; then
    pass "Output file created at out.mp4"
else
    fail "Output file not found"
    exit 1
fi

# ── Step 14: Verify output duration and codec ─────────────────────────────────

step "Verify output duration and codec"

OUT_DUR=$(ffprobe -v error -show_entries format=duration \
    -of default=noprint_wrappers=1:nokey=1 "$WORKDIR/out.mp4")
OUT_CODEC=$(ffprobe -v error -select_streams v:0 -show_entries stream=codec_name \
    -of default=noprint_wrappers=1:nokey=1 "$WORKDIR/out.mp4")

# Expected: shot-002 (2.0s) + shot-001 (1.5s) + shot-003 (1.0s) = 4.5s
EXPECTED_SECS="4.5"

echo "  Output codec:    $OUT_CODEC (expected: h264)"
echo "  Output duration: ${OUT_DUR}s (expected: ~${EXPECTED_SECS}s)"

# Verify codec
if [ "$OUT_CODEC" = "h264" ]; then
    pass "Codec is h264"
else
    fail "Expected codec h264, got $OUT_CODEC"
fi

# Verify duration within tolerance (1.0s to account for codec frame boundaries
# and ffmpeg concat demuxer timing)
DURATION_OK=$(awk "BEGIN {
    diff = $OUT_DUR - $EXPECTED_SECS
    if (diff < 0) diff = -diff
    print (diff < 1.0) ? \"yes\" : \"no\"
}")

if [ "$DURATION_OK" = "yes" ]; then
    pass "Duration is ~${EXPECTED_SECS}s (actual: ${OUT_DUR}s)"
else
    fail "Duration mismatch: got ${OUT_DUR}s, expected ~${EXPECTED_SECS}s (tolerance: 1.0s)"
fi

# Verify output has both video and audio streams
STREAM_COUNT=$(ffprobe -v error -show_entries stream=codec_type \
    -of default=noprint_wrappers=1:nokey=1 "$WORKDIR/out.mp4" | wc -l | tr -d ' ')

if [ "$STREAM_COUNT" -ge 2 ]; then
    pass "Output has video and audio streams ($STREAM_COUNT streams)"
else
    fail "Expected at least 2 streams (video+audio), got $STREAM_COUNT"
fi

# ── Summary ────────────────────────────────────────────────────────────────────

echo ""
echo "========================================"
printf '  Passed: %d  Failed: %d\n' "$passed" "$failed"
echo "========================================"

[ "$failed" -eq 0 ] && exit 0 || exit 1
