---
title: "ADR-002: Tagged Union Shot Ranges"
type: architecture-decision-record
status: accepted
parent: SPEC-001
---

# ADR-002: Tagged Union Shot Ranges

## Context

Sources may have speech (transcript with word-level timestamps), visual content (scene index with descriptions), or both. The edit document needs to reference time ranges in a way that works across all source types and allows mixing them in a single edit.

## Decision

A shot's `range` field is a **tagged union** with exactly one of three variants:

```json
{ "words":  { "from": 0, "to": 52 } }
{ "scenes": { "from": 0, "to": 2 } }
{ "time":   { "from_ms": 15000, "to_ms": 22000 } }
```

- `words` — resolved via transcript word indices → timestamps
- `scenes` — resolved via scene index indices → timestamps
- `time` — direct millisecond timestamps, no prerequisite data needed

## Alternatives Considered

### A. Always store timestamps

All shots use `from_ms` / `to_ms`. Word or scene references are resolved at creation time.

- **Pro**: Uniform format; no runtime resolution needed
- **Con**: Loses the semantic link to the transcript — if the user re-transcribes with a better model, edits break. Cannot adjust by "3 words earlier" without reverse-mapping timestamps to words.

### B. Dual fields (word + time always present)

Store both word indices and resolved timestamps on every shot.

- **Pro**: Fast reads; no resolution step
- **Con**: Data duplication; stale timestamps if transcript is regenerated; confusing for agent (which is authoritative?); not all sources have words

### C. Single "segment index" abstraction

Unify words and scenes into a single "segment" concept with a universal index.

- **Pro**: Simpler API — always `from_segment` / `to_segment`
- **Con**: Segments mean different things for different sources; loses type information; confusing error messages ("segment 5 out of bounds" — is that a word or a scene?)

## Consequences

- The system must resolve ranges at runtime, requiring access to transcript/index files
- Validation must check that the range type matches available data (word range requires transcript, scene range requires index)
- The agent can choose the most natural range type for each source
- `time` range provides an escape hatch when neither transcript nor index exists
- Serde's `#[serde(tag)]` or adjacently-tagged enum handles the JSON serialization naturally in Rust

## Trace

- REQ-012 (Segment Addition)
- REQ-015 (Segment Trimming)
- REQ-017 (Edit Document Validation)
