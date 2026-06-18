# Movable List CRDT

A sequence [[CRDT]] that supports a first-class **move** operation — relocating
an existing element to a new position while preserving its identity — in
addition to insert and delete. Ordinary list CRDTs (RGA, Fugue, Y.Array) model a
move as *delete-then-insert*, which under concurrency loses the moved element's
identity and any concurrent edits made to it, and can duplicate the element when
two peers move it at once.

[[Loro]]'s `MovableList` implements move as its own operation with deterministic
convergence: concurrent moves of the same element resolve to a single position
on every peer, and a move concurrent with an edit to the element's contents
keeps both (the relocation and the content edit, on the same identity).

This is the decisive reason [[SPEC-003-realtime-collaborative-editing#ADR-007]]
selects Loro: shot reordering ([[SPEC-001-transcript-video-editor#REQ-013]],
`move-segment`) maps directly onto `MovableList`, satisfying
[[SPEC-003-realtime-collaborative-editing#REQ-080]] without a hand-rolled
identity-preservation invariant.
