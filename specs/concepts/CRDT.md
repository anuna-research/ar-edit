# CRDT (Conflict-free Replicated Data Type)

A data structure that can be replicated across multiple peers, edited
independently and concurrently on each, and merged automatically without
coordination or conflict resolution. Merge is **commutative, associative, and
idempotent**, so peers that have seen the same set of changes converge to the
same state regardless of the order in which those changes arrived — the
[[Strong Eventual Consistency]] guarantee.

CRDTs come in two broad flavours: **state-based** (CvRDTs — peers exchange and
merge whole states or deltas via a join function) and **operation-based**
(CmRDTs — peers broadcast operations that commute). Practical libraries such as
[[Loro]] and [[Automerge]] are delta/op hybrids that ship compact incremental
updates and track causality with a [[Version Vector]].

In [[SPEC-003-realtime-collaborative-editing]] the edit document is reformulated
as a collection of CRDT containers: a [[Movable List CRDT]] for shot order,
last-writer-wins registers for shot fields, and observed-remove sets for markers
and points of interest. This replaces the single-writer event log of
[[ADR-001-event-sourced-edits]] (see
[[SPEC-003-realtime-collaborative-editing#ADR-011]]).
