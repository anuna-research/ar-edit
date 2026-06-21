# PAKE (Password-Authenticated Key Exchange)

A family of cryptographic protocols by which two (or more) parties who share
only a **low-entropy** secret — a password or short phrase — establish a strong
shared cryptographic key over an untrusted channel, while guaranteeing that an
attacker cannot learn the password or key faster than by online guessing.

The defining property is **single online guess per attempt**: an active attacker
who interposes on a run gets exactly one chance to test a candidate password,
and a failure leaks nothing usable for an offline dictionary attack against
recorded traffic. This is what lets a memorable, human-transcribable secret be
secure despite its low entropy.

[[SPAKE2]] is the specific PAKE used in
[[SPEC-003-realtime-collaborative-editing]];
[[SPEC-003-realtime-collaborative-editing#NFR-014]] formalises the single-guess
requirement plus channel lockout after repeated failures.
