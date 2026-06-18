# SPAKE2

A **Password-Authenticated Key Exchange** ([[PAKE]]) protocol. Two parties who
share only a low-entropy secret (a short password or phrase) each send a single
blinded message derived from that secret; from the exchange both derive a strong,
high-entropy shared session key — but only if their secrets match.

Security properties that make it suitable for human-transcribed pairing phrases:

- **Offline-attack resistance**: an eavesdropper who records the entire exchange
  cannot mount an offline dictionary attack to recover the password or the key.
- **Single online guess**: an active man-in-the-middle gets at most *one*
  password guess per protocol run; a wrong guess simply fails key confirmation
  and reveals nothing. This is the property
  [[SPEC-003-realtime-collaborative-editing#NFR-014]] depends on.

In [[SPEC-003-realtime-collaborative-editing]] SPAKE2 is keyed by the
`<num>-<word>-<word>` pairing phrase and run over a [[Rendezvous Server]]
channel ([[SPEC-003-realtime-collaborative-editing#REQ-070]],
[[SPEC-003-realtime-collaborative-editing#ADR-009]]). Because it touches
cryptography, its integration is a **no-go area** under [[PROTO-001]] AI Trust
Boundaries: audited implementation, cross-model review, and human expert sign-off
are mandatory. This is the design used by *magic-wormhole*.
