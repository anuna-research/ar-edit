# User: LLM Agent

| Field | Value |
|-------|-------|
| Archetype | LLM Agent |
| Document | User Profile: LLM Agent |
| Version | 1.0.0 |

## Role

An AI agent (e.g. Claude, GPT) that drives `ar-edit` programmatically to produce video edits from natural language instructions. The agent reads transcripts, reasons about content, and issues CLI commands to assemble edits.

## Goals

1. **Primary**: Receive a natural language editing instruction (e.g. "combine the best takes from interview A and B, cutting filler words") and translate it into a valid edit document
2. **Secondary**: Validate edits against transcript boundaries before rendering
3. **Tertiary**: Iterate on edits based on human feedback without starting from scratch

## Constraints

- **Interface**: Can only interact via CLI commands and file I/O — no GUI, no interactive prompts
- **Determinism**: Every command must be idempotent or clearly stateful; the agent needs to reason about current state
- **Error handling**: Must receive structured, parseable error output (JSON or exit codes) to make retry decisions
- **Context window**: Transcripts may be large; the agent needs summary/search capabilities, not just raw dumps
- **No side effects on preview**: Preview commands must not mutate project state

## Interaction Pattern

1. Receives a project directory containing video files
2. Issues `ar-edit transcribe <file>` commands to generate transcripts
3. Reads transcript JSON files to understand content
4. Constructs an edit document (JSON/TOML) specifying segments from each source
5. Issues `ar-edit validate <edit-doc>` to check the edit document
6. Issues `ar-edit preview <edit-doc> [--segment N]` to preview specific segments
7. Issues `ar-edit render <edit-doc> -o output.mp4` to produce final output
8. Can modify the edit document and re-validate/re-render

## Success Criteria

- Every CLI command returns structured output (JSON) with clear success/error semantics
- The agent can construct a valid edit document from transcripts without any human intervention
- Error messages include enough context for the agent to self-correct (e.g. "segment 3 end time 45.2s exceeds source duration 44.8s")
- The full pipeline (transcribe -> edit -> render) can be driven entirely through sequential CLI invocations
