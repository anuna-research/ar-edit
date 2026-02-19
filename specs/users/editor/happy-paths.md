# Happy Paths: Video Editor

| Field | Value |
|-------|-------|
| Document | Happy Paths: Video Editor |
| Version | 1.0.0 |

---

## Happy Path 1: Single Video Trim by Transcript

**Task**: Trim a single video by removing filler sections identified in the transcript.

**Preconditions**:
- One video file (`interview.mp4`) exists in the working directory
- `whisper.cpp` is installed and accessible
- `ffmpeg` is installed

**Steps**:

1. `ar-edit init myproject` → Creates project directory with config
2. `ar-edit add interview.mp4` → Registers source video, assigns source ID `src-001`
3. `ar-edit transcribe src-001` → Runs whisper.cpp, produces `src-001.transcript.json` with word-level timestamps
4. User reviews `src-001.transcript.json`, identifies keeper segments
5. `ar-edit edit create myedit` → Creates empty edit document `myedit.json`
6. `ar-edit edit add-segment myedit --source src-001 --from-word 0 --to-word 142` → Adds first keeper segment
7. `ar-edit edit add-segment myedit --source src-001 --from-word 200 --to-word 380` → Adds second keeper segment
8. `ar-edit preview myedit --segment 0` → Opens VLC at the first segment for review
9. `ar-edit render myedit -o trimmed.mp4` → Renders final output

**Postconditions**:
- `trimmed.mp4` exists, contains only the keeper segments
- No gap/silence between segments (clean concatenation)
- Audio and video remain in sync

**Failure Modes**:
- whisper.cpp transcription fails (bad audio codec) → Clear error with suggestion to re-encode audio
- Word index out of bounds → Error: "Word index 380 exceeds transcript length 375 for src-001"
- VLC not installed → Error with install instructions, falls back to ffplay

---

## Happy Path 2: Multi-Source Assembly

**Task**: Combine segments from 3 interview videos into a single edit.

**Preconditions**:
- Three video files exist: `alice.mp4`, `bob.mp4`, `carol.mp4`
- All prerequisites installed

**Steps**:

1. `ar-edit init interviews`
2. `ar-edit add alice.mp4 bob.mp4 carol.mp4` → Registers as `src-001`, `src-002`, `src-003`
3. `ar-edit transcribe --all` → Transcribes all sources in parallel
4. `ar-edit transcripts list` → Shows summary of each transcript with word counts and duration
5. `ar-edit transcripts search "climate change"` → Finds matching segments across all sources with word indices
6. User creates edit document (manually or by piping search results):
   ```
   ar-edit edit create assembly
   ar-edit edit add-segment assembly --source src-001 --from-word 45 --to-word 120
   ar-edit edit add-segment assembly --source src-003 --from-word 200 --to-word 280
   ar-edit edit add-segment assembly --source src-002 --from-word 10 --to-word 90
   ```
7. `ar-edit preview assembly` → Plays full assembly in VLC
8. `ar-edit render assembly -o final.mp4`

**Postconditions**:
- `final.mp4` contains segments from all three sources in specified order
- Transitions between sources are clean cuts (no artifacts)
- Source metadata preserved in output (chapter markers or sidecar file)

**Failure Modes**:
- Mismatched codecs between sources → Auto-normalize or clear error
- Segment overlap within same source → Warning (not error) — user may intend it
- Empty segment (from-word == to-word) → Error: "Segment has zero duration"

---

## Happy Path 3: Edit Document as Text File

**Task**: Edit the transcript directly as a text file, with the tool mapping text selections back to video timestamps.

**Preconditions**:
- Project with transcribed sources exists

**Steps**:

1. `ar-edit transcripts export --format editable -o draft.md` → Exports all transcripts as a markdown document with source/word annotations embedded as HTML comments
2. User opens `draft.md` in their text editor
3. User deletes unwanted paragraphs, reorders sections, copy-pastes segments between source sections
4. `ar-edit edit from-transcript draft.md -o assembly` → Parses the edited markdown, resolves annotations back to source/word-index pairs, creates edit document
5. `ar-edit validate assembly` → Checks all references are valid
6. `ar-edit render assembly -o final.mp4`

**Postconditions**:
- The rendered video matches the text content of `draft.md` exactly
- Word-level precision: cuts happen at word boundaries, not arbitrary timestamps

**Failure Modes**:
- User deletes annotation comments → Error: "Cannot resolve paragraph starting with '...' to any source. Annotations missing."
- User adds new text not in any source → Warning: "Text at line 42 does not match any source transcript. Skipping."
- Annotations corrupted → Error with line number and expected format
