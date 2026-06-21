# Lamport timestamps

A logical clock scheme (Leslie Lamport, 1978) that assigns a monotonically
increasing counter to events so that causally related events are ordered: if
event A happens-before B, then `L(A) < L(B)`. Ties between concurrent events are
broken by a unique actor/process identifier, yielding a deterministic **total
order** over events that respects causality.

In [[SPEC-003-realtime-collaborative-editing]] this is the basis for
deterministic tie-breaking of otherwise-concurrent [[CRDT]] changes
([[SPEC-003-realtime-collaborative-editing#REQ-072]]): the peer's actor ID
breaks ties so all peers pick the same winner, without implying a false
happens-before relationship. Closely related to the [[Version Vector]], which
generalises a single Lamport counter to a per-actor vector for delta sync.
