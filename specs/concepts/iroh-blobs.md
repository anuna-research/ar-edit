# iroh-blobs

A content-addressed blob transfer protocol and store built on [[iroh]]. Blobs are
addressed by their [[BLAKE3]] hash; large blobs use BLAKE3's verified-streaming
property so a receiver can validate data incrementally as it arrives. Transfers
are **deduplicated** (a blob already present by hash is never re-fetched),
**resumable**, and **integrity-checked** by construction.

In [[SPEC-003-realtime-collaborative-editing#ADR-010]] iroh-blobs replicates
source video and the derived artefacts (transcripts, indices, thumbnails)
between peers ([[SPEC-003-realtime-collaborative-editing#REQ-075]],
[[SPEC-003-realtime-collaborative-editing#REQ-077]]). Content addressing makes
the "both peers already hold this file" case free, and the BLAKE3 verification
underpins [[SPEC-003-realtime-collaborative-editing#REQ-076]] — a received blob
is admitted only after its recomputed hash matches the request.
