# Version Vector

A map from actor/replica identifier to the highest sequence number that replica
has produced and the local peer has observed. Comparing two version vectors
tells you, without exchanging full state, which changes each side is missing and
whether two states are causally ordered or concurrent.

Version vectors drive **delta sync**: on connect or reconnect, peers exchange
vectors and transfer only the operations the other side has not yet seen. This
is what makes [[SPEC-003-realtime-collaborative-editing#REQ-087]] (offline edit
then reconnect) and [[SPEC-003-realtime-collaborative-editing#NFR-012]]
(convergence bound, delta-only) achievable without resending the whole
document. [[Loro]] tracks a version vector internally and exposes it for the
`SYNC-REQ` message in [[SPEC-003-realtime-collaborative-editing#CON-015]].

Closely related to [[Lamport timestamps]] and actor-ID tie-breaking
([[SPEC-003-realtime-collaborative-editing#REQ-072]]) used to order
otherwise-concurrent changes deterministically.
