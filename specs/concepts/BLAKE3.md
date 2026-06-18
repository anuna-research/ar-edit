# BLAKE3

A fast cryptographic hash function. Beyond being a 256-bit digest, BLAKE3 is
built on a Merkle tree, which gives it two properties this project relies on:
**parallelism** (large files hash quickly across cores) and **verified
streaming** (a receiver can authenticate a file incrementally, chunk by chunk,
against the root hash without having the whole file first).

In [[SPEC-003-realtime-collaborative-editing]] BLAKE3 is the **content address**
for source media and derived artefacts: it is the key under which [[iroh-blobs]]
stores and requests blobs, the basis for deduplication, and the integrity check
that gates a received blob before it is written to `sources/`
([[SPEC-003-realtime-collaborative-editing#REQ-076]]). A source ID that maps to
two different BLAKE3 hashes across peers is a content conflict
([[SPEC-003-realtime-collaborative-editing#REQ-074]]).
