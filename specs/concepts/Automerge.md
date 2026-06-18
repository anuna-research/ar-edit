# Automerge

A mature, widely deployed [[CRDT]] library for JSON-like documents, with a Rust
core and strong formal grounding. Automerge offers maps, lists, text, and
counters, automatic merge, and efficient binary change/sync formats.

In [[SPEC-003-realtime-collaborative-editing#ADR-007]] Automerge is the primary
considered alternative to [[Loro]]. It was not selected because its list model
represents a *move* as delete-then-insert, which loses element identity under
concurrent editing — directly at odds with
[[SPEC-003-realtime-collaborative-editing#REQ-080]] (identity-preserving
concurrent shot moves). A move-as-tombstone workaround is possible but
re-introduces the hand-rolled invariant the [[Movable List CRDT]] primitive
avoids.
