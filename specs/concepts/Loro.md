# Loro

A high-performance [[CRDT]] library (Rust core, with bindings) for building
local-first and collaborative applications. Loro provides rich CRDT container
types — `Map`, `List`, `MovableList` (see [[Movable List CRDT]]), `Text`, and
`Tree` — composable into a single document, with compact delta encoding,
[[Version Vector]]-based incremental sync, time-travel/checkout, and a per-peer
`UndoManager`.

In [[SPEC-003-realtime-collaborative-editing#ADR-007]] Loro is the chosen CRDT
engine. The edit document maps onto: a `MovableList` of shot containers, a `Map`
of fields per shot, a `List` of append-only notes, and `Map`-keyed sets for
markers and POIs. Loro's `UndoManager` provides the per-actor undo required by
[[SPEC-003-realtime-collaborative-editing#REQ-086]], and its delta export/import
drives the sync envelope in
[[SPEC-003-realtime-collaborative-editing#CON-015]].

Considered alternative: [[Automerge]] — mature and well-grounded, but its list
move is delete+insert, which does not preserve element identity under
concurrency.
