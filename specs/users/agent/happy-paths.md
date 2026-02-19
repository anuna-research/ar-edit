# Happy Paths: LLM Agent

| Field | Value |
|-------|-------|
| Document | Happy Paths: LLM Agent |
| Version | 1.0.0 |

---

## Happy Path 1: Agent-Driven Edit from Natural Language

**Task**: An LLM agent receives "Make a 2-minute highlight reel from these three interview clips, focusing on their answers about project outcomes."

**Preconditions**:
- Project directory with 3 source videos, already transcribed
- Agent has shell access to run `ar-edit` commands
- Agent can read/write files

**Steps**:

1. `ar-edit transcripts list --json` → Agent receives JSON array of sources with word counts, durations
2. `ar-edit transcripts read src-001 --json` → Agent reads full transcript with word-level timestamps
3. (Repeat for src-002, src-003)
4. Agent reasons over transcripts, identifies segments about "project outcomes"
5. Agent calculates total duration of candidate segments, trims to fit 2-minute target
6. Agent writes edit document directly as JSON to `highlight.json`
7. `ar-edit validate highlight.json --json` → Returns `{"valid": true}` or `{"valid": false, "errors": [...]}`
8. If invalid, agent reads errors, corrects the edit document, re-validates
9. `ar-edit render highlight.json -o highlight.mp4 --json` → Returns `{"success": true, "output": "highlight.mp4", "duration": 118.4}`

**Postconditions**:
- `highlight.mp4` exists, approximately 2 minutes long
- Contains only segments about project outcomes
- Agent received structured JSON at every step — no parsing of human-readable text required

**Failure Modes**:
- Transcript too large for agent context window → Agent uses `ar-edit transcripts search "outcomes" --json` to narrow down
- Segment boundary imprecise → Agent adjusts word indices and re-validates
- Render fails (ffmpeg error) → JSON error includes ffmpeg stderr for diagnosis

---

## Happy Path 2: Iterative Refinement with Human Feedback

**Task**: Human reviews the agent's first cut and says "The section from Bob is too long, cut it in half. Also move Alice's closing remarks to the end."

**Preconditions**:
- Agent has already created and rendered `v1.json`
- Human has reviewed the output

**Steps**:

1. `ar-edit edit show v1.json --json` → Agent reads current edit structure
2. Agent identifies the Bob segment (e.g. segment index 2), halves its word range
3. Agent identifies Alice's closing remarks segment, moves it to the end of the segment list
4. Agent writes updated edit document as `v2.json`
5. `ar-edit validate v2.json --json` → Validates
6. `ar-edit render v2.json -o v2.mp4 --json` → Renders
7. Agent reports changes to human: "Shortened Bob's segment from 45s to 22s. Moved Alice's closing to final position. Total duration: 95s."

**Postconditions**:
- `v2.mp4` reflects the requested changes
- `v1.json` is untouched (edits are non-destructive, versioned)
- Agent can explain what changed between versions

**Failure Modes**:
- Halving by word count produces an awkward cut mid-sentence → Agent uses sentence boundary detection from transcript
- Moving segments creates a jarring topic transition → Agent could insert a brief pause/fade (future feature)
