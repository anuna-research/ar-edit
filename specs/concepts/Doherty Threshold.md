# Doherty Threshold

A usability principle (one of the Laws of UX): productivity soars when a system
and its users interact at a pace (**≤ 400 ms**) that neither has to wait on the
other. Below this threshold an interaction feels instantaneous and the user
stays in flow; above it, attention drifts and perceived quality drops.

[[PROTO-001]] treats UX laws as testable constraints rather than preferences. In
[[SPEC-003-realtime-collaborative-editing]] the Doherty Threshold is instantiated
as [[SPEC-003-realtime-collaborative-editing#NFR-009]]: a committed local edit
must appear on every connected peer's screen within 400 ms at the 95th
percentile, measured end-to-end from commit through CRDT delta encode, transport,
remote apply, and remote render.
