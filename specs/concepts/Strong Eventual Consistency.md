# Strong Eventual Consistency (SEC)

A consistency model, weaker than linearizability but stronger than plain
eventual consistency, in which **any two replicas that have received the same
set of updates are in the same state** — with no need for the updates to arrive
in the same order, and with no rollback or conflict-resolution step. It is the
defining correctness property of a [[CRDT]].

Formally: replicas are *eventually consistent* (delivered-update sets converge)
**and** *strongly convergent* (equal delivered-update sets ⇒ equal state). The
merge function must be commutative, associative, and idempotent so that
duplication and reordering of updates are harmless.

[[SPEC-003-realtime-collaborative-editing#REQ-083]] requires SEC for the edit
document, markers, and POIs: peers that have observed the same changes hold
byte-identical materialised edit documents, and no merge ever requires human
conflict resolution. [[SPEC-003-realtime-collaborative-editing#NFR-012]] bounds
how quickly peers reach SEC after a partition heals.
