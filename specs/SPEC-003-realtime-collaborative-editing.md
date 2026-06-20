---
id: SPEC-003
title: "SPEC-003: Realtime Collaborative Editing via P2P CRDTs"
type: specification
version: 1.1.0
status: implementing
parent: SPEC-001
---

<!-- Revision 1.1.0 (2026-06-19): pairing rendezvous reworked from a dedicated
     server to serverless phrase-keyed pkarr / BitTorrent Mainline DHT discovery
     with SPAKE2 run over the resulting direct iroh connection. Resolves OQ-3;
     revises ADR-009; adds ADR-013 and CON-017; repurposes CON-014; demotes the
     s7 TCP relay to an optional DHT-blocked fallback. Pattern after
     ../did-crdt ADR-006 (pkarr-derived keypair for keyed DHT discovery). -->
---

# SPEC-003: Realtime Collaborative Editing via P2P CRDTs

## Overview

This specification extends [[SPEC-001-transcript-video-editor]] from a
single-machine tool into a **realtime, multiplayer** editor in which two or
more `ar-edit` instances collaborate on the same project over a direct
peer-to-peer connection — no central server holds project state.

Three capabilities compose to deliver this:

1. **Pairing** — A human pairs two instances by reading a short
   `<num>-<word>-<word>` phrase aloud (or pasting it) and running
   `ar-edit pair <phrase>`. The phrase is a low-entropy secret consumed by a
   [[SPAKE2]] password-authenticated key exchange so that, after pairing, both
   peers share a strong symmetric key that no eavesdropper or man-in-the-middle
   can derive from observing the handshake.
2. **Transport** — Once paired, peers communicate directly over [[iroh]]
   (a [[QUIC]]-based p2p library) for both control messages and bulk data.
   A lightweight [[Rendezvous Server]] is used only to broker the initial
   handshake; all project data flows peer-to-peer.
3. **Convergent state** — The mutable project state (the edit document, plus
   source markers and points of interest) is reformulated as a collection of
   [[CRDT]]s so that concurrent edits from multiple peers merge automatically
   without conflicts and every peer converges on identical state
   ([[Strong Eventual Consistency]]).

The **first observable step** of a paired session is **source-material
synchronisation**: before any collaborative editing begins, the peers reconcile
their source inventories and replicate any missing source media via
[[iroh-blobs]] (content-addressed by [[BLAKE3]] hash, with deduplication), so
that every peer can resolve, preview, and render every shot in the shared edit.

### Scope decisions (from stakeholder dialogue, 2026-06-18)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Source sync | Full content-addressed media replication via [[iroh-blobs]] | "Both clients sync the source materials" requires that either peer can independently render any shot; content addressing gives free dedup and integrity |
| CRDT formulation | Off-the-shelf document CRDT ([[Loro]] — see [[SPEC-003-realtime-collaborative-editing#ADR-007]]) | Composition-First (Constitutional Principle 15); a hand-rolled CRDT at this novelty is unjustified risk |
| Pairing rendezvous | **Serverless: phrase-keyed [[pkarr]] / [[Mainline DHT]] discovery → SPAKE2 over direct iroh** (see [[SPEC-003-realtime-collaborative-editing#ADR-013]]) | No server to host (resolves OQ-3); reuses the public DHT iroh already uses for discovery; SPAKE2's single-guess property keeps the low-entropy phrase safe. (Revised from a dedicated rendezvous server in v1.1.0; the TCP relay survives as an optional DHT-blocked fallback.) |
| Coordination scope | **N-peer** from the first version | Stakeholder requirement; CRDT actor model and sync topology specified for ≥ 3 concurrent editors |

### Phase 0 note — Experiment vs Specify

Per [[PROTO-001]]'s decision table, this feature rates **High** on technical
novelty (CRDT reformulation of an event-sourced document), **High** on
performance risk (realtime propagation latency over p2p, multi-GB media
transfer), and **Medium–High** on data uncertainty (merge semantics for moves
and trims under concurrency). Two or more High factors normally indicate a
**governed experiment first**. Because the stakeholder has asked to specify
directly, this document proceeded — and the mandatory de-risking spike
([[SPEC-003-realtime-collaborative-editing#OQ-1]]) has since been **run and
passed** (`spike/oq-1/`, 2026-06-18): (a) Loro `MovableList` gives
identity-preserving concurrent moves and Strong Eventual Consistency, (b) the
SPAKE2 handshake is ~0.2 ms (pairing is network-bound, not crypto-bound), and
(c) BLAKE3 hashes a 2 GB source at 1.85–2.58 GB/s (content-addressing is not the
sync bottleneck). The High-novelty CRDT-convergence risk is therefore
empirically retired; the actual iroh transport latency/throughput run is
deferred to [[SPEC-003-realtime-collaborative-editing#OQ-7]] (host
disk-constrained). Findings: `spike/oq-1/FINDINGS.md`.

---

## User Profiles

This spec adds a collaborative *facet* to the two existing profiles rather than
a new archetype:

- [[users/editor/user|Video Editor]] — now potentially one of several humans
  editing the same project live (e.g. an editor and a director reviewing
  together). The keyboard-first, accessibility-conscious constraints from the
  profile carry over: pairing and presence MUST be operable without a mouse and
  legible to a screen reader.
- [[users/agent/user|LLM Agent]] — may join a session as a peer to assemble or
  revise an edit while a human watches changes appear live. The agent's
  constraint that *all interaction is via CLI + structured output* extends to
  collaboration: pairing, sync status, presence, and convergence MUST be
  fully drivable and observable through `--json`.

A new happy path — *two editors co-review a rough cut* — is recorded in
[[users/editor/happy-paths]] and a corresponding *agent joins a live session*
path in [[users/agent/happy-paths]]; both are derived from, not the source of,
the requirements below (Constitutional Principle 13).

---

## Relationship to Existing Concepts & Impact on SPEC-001

The mutable/immutable split established in [[DATA-MODEL]] is the foundation that
makes this tractable:

| Layer | Mutability ([[DATA-MODEL]]) | Collaboration treatment |
|-------|------------------------------|--------------------------|
| Source video files | Immutable | Replicated once via [[iroh-blobs]]; never mutated, only transferred |
| Transcripts | Immutable | Replicated as content-addressed blobs; identical hash ⇒ no transfer |
| Scene index | Append-only (descriptions added) | Modelled as a [[CRDT]] map (descriptions are LWW per scene) |
| Edit document | Mutable (the *only* fully-mutable structure) | **Reformulated as a [[CRDT]]** — the core of this spec |
| Source markers ([[SPEC-001-transcript-video-editor#REQ-049]]) | Mutable | Modelled as an add/remove [[CRDT]] set |
| Points of interest ([[SPEC-002-points-of-interest#REQ-054]]) | Mutable | Modelled as an add/remove [[CRDT]] set |

### Impact on [[ADR-001-event-sourced-edits]]

[[ADR-001-event-sourced-edits]] chose an append-only operation log with a head
pointer and cached snapshot. That model assumes a **single writer** and a
**total order** of operations — both assumptions break under concurrent
multi-peer editing. This spec's [[SPEC-003-realtime-collaborative-editing#ADR-011]]
**supersedes ADR-001's mutation model** (not its non-destructive intent) by
making a [[CRDT]] the canonical representation of the edit document.
Single-player editing becomes the degenerate case of a one-actor CRDT, so the
non-destructive guarantee is preserved while gaining merge.

The following [[SPEC-001-transcript-video-editor]] requirements are
**semantically affected** and re-specified for collaborative mode here; their
single-player behaviour is unchanged:

- [[SPEC-001-transcript-video-editor#REQ-046]] (Undo) and
  [[SPEC-001-transcript-video-editor#REQ-047]] (Redo) — redefined as
  **per-actor** undo in [[SPEC-003-realtime-collaborative-editing#REQ-086]].
- [[SPEC-001-transcript-video-editor#REQ-048]] (Operation history) — the linear
  op log is replaced by a partially-ordered change history; history display
  becomes causal, not linear.
- [[SPEC-001-transcript-video-editor#REQ-013]] (Segment reordering) — now a
  move on a [[Movable List CRDT]] (see
  [[SPEC-003-realtime-collaborative-editing#REQ-080]]).

These changes will require a version bump and an `implementing → superseded`
transition on the affected portions of [[SPEC-001-transcript-video-editor]]
once SPEC-003 reaches `implemented`; that bookkeeping is tracked as
[[SPEC-003-realtime-collaborative-editing#OQ-4]].

---

## Functional Requirements

### Pairing and Session Establishment

**REQ-067: Pairing Phrase Generation**

The system SHALL generate a human-transcribable pairing phrase of the form
`<num>-<word>-<word>` WHEN the user opens a collaborative session via
`ar-edit share` WITH the leading `<num>` (range 0–999) and each `<word>` (drawn
from the [[BIP39]] English wordlist — 2048 words, each uniquely identified by
its first four letters) together forming the **shared low-entropy secret**: the
whole phrase is both the [[SPAKE2]] password *and* the seed for the
phrase-derived discovery keypair (see
[[SPEC-003-realtime-collaborative-editing#ADR-013]]). The phrase SHALL be
generated with a cryptographically secure RNG, providing at least 2048² ≈ 2^22
combinations across the two words (plus ~2^10 from `<num>`), and be displayed
prominently for the host to communicate out-of-band. (`<num>` is no longer a
server channel index; it is part of the secret.)

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-074]]
- [[SPEC-003-realtime-collaborative-editing#TEST-075]]
- [[SPEC-003-realtime-collaborative-editing#CON-012]]
- [[SPEC-003-realtime-collaborative-editing#CON-013]]

**REQ-068: Session Hosting**

The system SHALL open the current project for collaboration WHEN the user runs
`ar-edit share` WITH the result being: (a) a pairing phrase generated per
[[SPEC-003-realtime-collaborative-editing#REQ-067]], (b) **publication of a
signed [[pkarr]] discovery record advertising the host's [[iroh]] NodeAddr
(NodeId + direct addresses + relay), keyed by the phrase-derived discovery key,
to the [[Mainline DHT]] (or a pkarr relay)** per
[[SPEC-003-realtime-collaborative-editing#CON-017]] and refreshed before
expiry, and (c) the host process (TUI, blocking `share`, or the session daemon —
[[SPEC-003-realtime-collaborative-editing#ADR-014]]) entering a waiting state that accepts joining peers
until the session is closed. The phrase SHALL expire after a bounded,
configurable interval (default 10 minutes) after which the discovery record is
withdrawn and the phrase single-use-invalidated. Publication SHALL be
suppressible via a `DISABLE_DHT_PUBLISH`-style opt-out (privacy).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-076]]
- [[SPEC-003-realtime-collaborative-editing#CON-012]]
- [[SPEC-003-realtime-collaborative-editing#CON-017]]

**REQ-069: Session Joining**

The system SHALL join an existing collaborative session WHEN the user runs
`ar-edit pair <phrase>` WITH the system parsing the phrase per
[[SPEC-003-realtime-collaborative-editing#CON-013]], **deriving the discovery
keypair from the phrase, looking up the host's [[pkarr]] discovery record on the
[[Mainline DHT]]** ([[SPEC-003-realtime-collaborative-editing#CON-017]]),
**dialling the host directly over [[iroh]] using the record's address hints,
and completing the [[SPAKE2]] exchange (keyed by the full phrase) over that
direct connection** — on success establishing the session with the host and
every other peer. A phrase that fails to parse SHALL be rejected before any
network action (Constitutional Principle 14).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-077]]
- [[SPEC-003-realtime-collaborative-editing#TEST-078]]
- [[SPEC-003-realtime-collaborative-editing#CON-012]]
- [[SPEC-003-realtime-collaborative-editing#CON-013]]
- [[SPEC-003-realtime-collaborative-editing#CON-017]]

**REQ-070: Authenticated Key Agreement**

The system SHALL derive a shared session key using a [[SPAKE2]] exchange
**conducted over the direct [[iroh]] connection** to the peer, keyed by the
full pairing phrase, BEFORE any project data is exchanged WITH the property that
(a) an eavesdropper cannot derive the session key, (b) an active attacker gets
at most **one** online password guess per pairing attempt (the single-guess
[[PAKE]] property), and (c) a confirmed key match is required before the
connection carries any project bytes. The number of failed pairing attempts
SHALL be bounded — the discovery key is single-use and the session burns after
the configured limit (default: burn on first failed confirmation).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-079]]
- [[SPEC-003-realtime-collaborative-editing#TEST-080]]
- [[SPEC-003-realtime-collaborative-editing#NFR-014]]
- [[SPEC-003-realtime-collaborative-editing#CON-014]]

**REQ-071: Serverless Peer Discovery and Direct Connection**

The system SHALL discover peers **without a dedicated server** by
publishing/looking up [[pkarr]] discovery records on the [[Mainline DHT]] (or a
pkarr relay) under the phrase-derived key
([[SPEC-003-realtime-collaborative-editing#CON-017]]), AFTER WHICH the
[[SPAKE2]] handshake ([[SPEC-003-realtime-collaborative-editing#CON-014]]) and
all project data (CRDT sync, blob transfer, presence) SHALL flow over the
direct peer-to-peer [[iroh]] connection. No third party SHALL receive the
pairing phrase, the [[SPAKE2]] session key, or any project data; the discovery
record carries only the host's [[iroh]] NodeAddr as **unauthenticated dialling
hints** (the iroh handshake authenticates the NodeId, so a forged hint costs
only a failed connection attempt). An optional [[Rendezvous Server]] relay
([[SPEC-003-realtime-collaborative-editing#ADR-013]]) MAY be used as a fallback
where the DHT is unreachable; it likewise never learns the phrase, key, or data.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-081]]
- [[SPEC-003-realtime-collaborative-editing#CON-014]]
- [[SPEC-003-realtime-collaborative-editing#CON-017]]

**REQ-072: Peer Actor Identity**

The system SHALL assign each peer a stable, unique **actor ID** for the lifetime
of its participation in a session WITH the actor ID derived from the peer's
[[iroh]] node public key, used as the [[CRDT]] actor/site identifier for all
changes that peer originates, and never reused by a different peer within a
session. Actor IDs SHALL totally order otherwise-concurrent changes
deterministically (tie-breaking for [[CRDT]] convergence) without implying a
happens-before relationship.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-082]]
- [[SPEC-003-realtime-collaborative-editing#TEST-083]]

**REQ-073: Session Membership Lifecycle**

The system SHALL track session membership as peers join and leave WHEN peers
connect, disconnect gracefully, or time out WITH each membership transition
surfaced to all connected peers (see
[[SPEC-003-realtime-collaborative-editing#REQ-085]]), a disconnected peer's
already-applied changes retained (CRDTs do not require the originator to stay
connected), and the session remaining alive as long as ≥ 1 peer is present.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-084]]
- [[SPEC-003-realtime-collaborative-editing#CON-012]]

**REQ-089: Session-Owning Process (Daemon)**

The system SHALL hold a live collaborative session in a **single long-running
process** that owns the in-memory [[CRDT]] document, the peer connections, and
the [[pkarr]] record refresh — because discrete one-shot CLI invocations cannot
maintain live state between calls. That process is either (a) the interactive
TUI or a blocking `ar-edit share`, or (b) a background **session daemon**
(`ar-edit daemon`) for headless/agent use. The daemon SHALL expose a local IPC
endpoint (a Unix-domain socket under the project directory; named pipe on
Windows per [[SPEC-001-transcript-video-editor#NFR-015]]) through which clients
attach to apply edits and read state, and SHALL re-publish the discovery record
before its TTL while it runs. WHEN the owning process exits, the session ends
and the discovery record is withdrawn ([[SPEC-003-realtime-collaborative-editing#REQ-068]]).
Starting a daemon SHALL NOT clobber an already-running one: if a live daemon is
listening on the socket, the new instance SHALL refuse rather than unlink the
socket (which would orphan the running session); a socket is removed only after
it is proven stale.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-119]]
- [[SPEC-003-realtime-collaborative-editing#CON-018]]
- [[SPEC-003-realtime-collaborative-editing#ADR-014]]

**REQ-090: CLI / Agent Attach to a Live Session**

The system SHALL route discrete edit and session commands (`ar-edit edit …`,
`ar-edit session …`) to a running session daemon over its IPC endpoint when one
is active for the project, so an agent or CLI user participates in the *live*
session and their mutations propagate to peers; WHEN no daemon is running, those
commands SHALL fall back to one-shot operations on the on-disk edit document
(unchanged single-player behaviour). IPC input is untrusted and fully recognised
before any action ([[SPEC-003-realtime-collaborative-editing#CON-018]],
Constitutional Principle 14).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-120]]
- [[SPEC-003-realtime-collaborative-editing#CON-018]]

---

### Source Material Synchronisation

**REQ-074: Source Inventory Reconciliation**

The system SHALL reconcile the source inventories of all peers as the first
action of a newly joined session, BEFORE collaborative editing is enabled, by
exchanging each peer's source manifest entries (source ID, [[BLAKE3]] content
hash, duration, codec metadata) and computing, per peer, the set of source
blobs it is missing WITH the reconciliation producing a deterministic merged
manifest in which a source ID maps to exactly one content hash. Two peers
presenting the **same source ID** with **different content hashes** SHALL raise
a structured conflict (see
[[SPEC-003-realtime-collaborative-editing#REQ-076]]) rather than silently
overwriting.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-085]]
- [[SPEC-003-realtime-collaborative-editing#TEST-086]]
- [[SPEC-003-realtime-collaborative-editing#CON-016]]

**REQ-075: Content-Addressed Media Replication**

The system SHALL replicate missing source media between peers over
[[iroh-blobs]] WITH each source file addressed by its [[BLAKE3]] hash, only the
byte ranges a peer lacks transferred (content-addressed deduplication), and the
transfer resumable across reconnects. A peer SHALL be able to fetch any source
referenced by the shared edit from any peer that holds it. Blob receipt SHALL be
**streamed in chunks and hashed incrementally** (BLAKE3 verified streaming) so
arbitrarily large media (multi-GB sources, e.g. the 2 GB
[[SPEC-003-realtime-collaborative-editing#NFR-011]] target) transfers without a
fixed in-memory read cap; the recomputed hash is checked before the blob is
admitted ([[SPEC-003-realtime-collaborative-editing#REQ-076]]).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-087]]
- [[SPEC-003-realtime-collaborative-editing#NFR-011]]
- [[SPEC-003-realtime-collaborative-editing#CON-016]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**REQ-076: Media Integrity Verification**

The system SHALL verify every received source blob by recomputing its
[[BLAKE3]] hash and comparing it to the advertised hash BEFORE the blob is
written into the project's `sources/` directory or referenced by any edit WITH
a mismatch causing the blob to be rejected, quarantined, and reported as a
structured error, and never partially applied (fail-closed, Constitutional
Principle 14).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-088]]
- [[SPEC-003-realtime-collaborative-editing#TEST-089]]
- [[SPEC-003-realtime-collaborative-editing#CON-016]]

**REQ-077: Derived-Artefact Replication**

The system SHALL replicate the immutable and append-only derived artefacts —
transcripts ([[SPEC-001-transcript-video-editor#REQ-005]]), scene indices, and
keyframe thumbnails — between peers as content-addressed blobs WITH a peer that
already holds an identical-hash artefact performing no transfer, so that every
peer can resolve word/scene ranges and render previews locally. Scene
*descriptions*, which are mutable, are synchronised as [[CRDT]] state per
[[SPEC-003-realtime-collaborative-editing#REQ-081]], not as immutable blobs.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-090]]
- [[SPEC-003-realtime-collaborative-editing#CON-016]]

**REQ-078: Synchronisation Ordering and Progress**

The system SHALL gate collaborative editing on synchronisation readiness such
that a peer's edit-mutation commands are accepted only AFTER it holds (or has
verifiably begun lazy retrieval of) the source inventory and CRDT state needed
to interpret them, AND SHALL report synchronisation progress (bytes
transferred, blobs remaining, percentage, per-source status) as structured
output in `--json` mode and as a progress indicator in the TUI.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-091]]
- [[SPEC-003-realtime-collaborative-editing#NFR-011]]
- [[SPEC-003-realtime-collaborative-editing#CON-012]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

---

### Collaborative Edit Document (CRDT)

**REQ-079: Edit Document as CRDT**

The system SHALL represent the edit document as a [[CRDT]] document (see
[[SPEC-003-realtime-collaborative-editing#ADR-007]]) that is the **canonical**
form of the edit in both single-player and collaborative modes WITH the CRDT
encoding the ordered list of shots, each shot's fields, and the shot notes, AND
WITH the existing on-disk JSON edit document
([[DATA-MODEL]] §3) derivable as a materialised view of the CRDT so that all
read-side commands ([[SPEC-001-transcript-video-editor#REQ-016]] `edit show`,
render, validate) operate unchanged.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-092]]
- [[SPEC-003-realtime-collaborative-editing#TEST-093]]
- [[SPEC-003-realtime-collaborative-editing#ADR-007]]
- [[SPEC-003-realtime-collaborative-editing#ADR-011]]

**REQ-080: Shot Ordering as a Movable Sequence**

The system SHALL represent the order of shots using a [[Movable List CRDT]] such
that (a) a shot inserted concurrently by two peers appears once per peer that
inserted it (no loss, no duplication of a single logical insert), (b) a shot
**moved** by `move-segment` is relocated rather than deleted-and-reinserted
(preserving the shot's identity, notes, and any concurrent edits to it), and
(c) concurrent moves of the same shot converge to a single deterministic
position across all peers. To make (a) hold without coordination, **new
collaborative inserts SHALL use an actor-scoped, generated shot id** (not a
per-replica sequential counter), so two peers inserting concurrently never
produce a colliding id whose fields would overwrite each other; a
locally-duplicated id is likewise disambiguated rather than overwritten.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-094]]
- [[SPEC-003-realtime-collaborative-editing#TEST-095]]
- [[SPEC-003-realtime-collaborative-editing#TEST-096]]

**REQ-081: Shot Field Convergence**

The system SHALL converge concurrent edits to a shot's fields WITH the shot's
`range` (the [[ADR-002-tagged-union-shot-ranges]] tagged union) modelled as a
last-writer-wins register tie-broken by actor ID per
[[SPEC-003-realtime-collaborative-editing#REQ-072]], the shot's `source` field
likewise, and concurrent trims of the same shot resolving to exactly one
range (no interleaving of `from`/`to` from different writers that could produce
an invalid range). The same convergence applies to mutable scene descriptions.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-097]]
- [[SPEC-003-realtime-collaborative-editing#TEST-098]]

**REQ-082: Markers and POIs as CRDT Sets**

The system SHALL represent source markers
([[SPEC-001-transcript-video-editor#REQ-049]]) and points of interest
([[SPEC-002-points-of-interest#REQ-054]]) as add/remove [[CRDT]] sets keyed by
their stable IDs WITH concurrent additions by different peers all retained,
a removal winning over a concurrent addition of the **same** ID it observed
(observed-remove semantics), and shot notes
([[SPEC-001-transcript-video-editor#REQ-051]]) — which are append-only — modelled
as a grow-only ordered set so no note is ever lost on merge.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-099]]
- [[SPEC-003-realtime-collaborative-editing#TEST-100]]

**REQ-083: Strong Eventual Consistency**

The system SHALL guarantee that any two peers that have observed the same set of
changes hold byte-identical materialised edit documents, regardless of the order
in which those changes arrived ([[Strong Eventual Consistency]]) WITH the
materialised view being a pure, deterministic function of the CRDT state and
no merge requiring human conflict resolution for the edit document, markers, or
POIs.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-101]]
- [[SPEC-003-realtime-collaborative-editing#TEST-102]]
- [[SPEC-003-realtime-collaborative-editing#NFR-012]]

**REQ-084: Real-time Change Propagation**

The system SHALL propagate each peer's committed change to every connected peer
as an incremental [[CRDT]] update over the direct [[iroh]] connection WITH the
change applied to remote peers' state and reflected in their TUI without a
full-document resend, and WITH the propagation observable as a live update in
the collaborators' edit timeline panel
([[SPEC-001-transcript-video-editor#REQ-039]]).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-103]]
- [[SPEC-003-realtime-collaborative-editing#NFR-009]]
- [[SPEC-003-realtime-collaborative-editing#CON-015]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**REQ-085: Peer Presence and Awareness**

The system SHALL display, to every peer, the set of currently connected peers
WITH each peer shown by a session-local display name and actor ID, and the TUI
indicating which shot each peer currently has selected (cursor presence) WITH
presence updates being ephemeral (not persisted to the edit document) and
disappearing when a peer disconnects.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-104]]
- [[SPEC-003-realtime-collaborative-editing#CON-012]]
- [[SPEC-003-realtime-collaborative-editing#CON-015]]

**REQ-086: Per-Actor Undo and Redo**

The system SHALL, in collaborative mode, scope undo and redo to the **local
actor's own changes** such that `ar-edit undo` (or `ctrl-z`) reverts the local
peer's most recent change without reverting changes made concurrently by other
peers, AND the inverse change propagates to all peers as an ordinary [[CRDT]]
update. This redefines, for collaborative mode, the single-writer head-pointer
semantics of [[SPEC-001-transcript-video-editor#REQ-046]] and
[[SPEC-001-transcript-video-editor#REQ-047]]; single-player behaviour
(one actor) is observationally identical to the existing undo/redo.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-105]]
- [[SPEC-003-realtime-collaborative-editing#TEST-106]]

**REQ-087: Offline Editing and Reconnect Merge**

The system SHALL allow a peer that has lost its connection to continue editing
locally WITH its changes accumulating in local [[CRDT]] state, and on
reconnection SHALL exchange only the changes each side is missing (delta sync
driven by [[Version Vector]]s) and converge per
[[SPEC-003-realtime-collaborative-editing#REQ-083]] WITHOUT data loss and
WITHOUT requiring a full state resend.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-107]]
- [[SPEC-003-realtime-collaborative-editing#TEST-108]]
- [[SPEC-003-realtime-collaborative-editing#NFR-012]]
- [[SPEC-003-realtime-collaborative-editing#CON-015]]

**REQ-088: Format Migration and Single-Player Continuity**

The system SHALL transparently migrate an existing event-sourced edit document
([[DATA-MODEL]] §3, `ops`/`head`/`snapshot`) into the [[CRDT]] representation on
first open WITH the materialised shot list after migration being identical to
the pre-migration `snapshot`, AND SHALL continue to operate on single-player
projects (no session) as a one-actor [[CRDT]], preserving the non-destructive
guarantee of [[ADR-001-event-sourced-edits]]. Migration SHALL run under a
**fixed migration peer id** so two peers migrating the same legacy edit
independently produce identical [[CRDT]] operations (idempotent on merge — no
duplicated shots), and SHALL then **re-key the document to the caller's
node-derived actor** ([[SPEC-003-realtime-collaborative-editing#REQ-072]]) so
subsequent edits carry a unique peer id rather than colliding on the migration
actor.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-109]]
- [[SPEC-003-realtime-collaborative-editing#TEST-110]]
- [[SPEC-003-realtime-collaborative-editing#ADR-011]]

---

## Non-Functional Requirements

**NFR-009: Change Propagation Latency**

A committed local edit SHALL be visible in every connected peer's TUI within
**400 ms** at the 95th percentile UNDER a session of ≤ 8 peers on connections
with ≤ 100 ms round-trip time, satisfying the [[Doherty Threshold]]. The
measurement spans commit → CRDT delta encode → iroh send → remote apply →
remote render.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-111]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**NFR-010: Pairing Latency**

A pairing attempt (`ar-edit pair <phrase>` → established direct iroh connection)
SHALL complete within **5 seconds** at the 95th percentile UNDER conditions
where both peers can reach the [[Rendezvous Server]] and a direct or relayed
iroh path exists, WITH a structured timeout error if no path is found within a
bounded interval (default 30 s).

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-112]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**NFR-011: Media Sync Throughput**

Source-media replication SHALL sustain throughput ≥ **80%** of the available
end-to-end bandwidth between two peers on a direct iroh connection UNDER
transfer of a representative 2 GB source set, WITH already-present
(identical-hash) blobs incurring zero transfer and progress reported per
[[SPEC-003-realtime-collaborative-editing#REQ-078]].

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-113]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**NFR-012: Convergence Bound**

After a network partition heals, all peers SHALL reach
[[Strong Eventual Consistency]] (byte-identical materialised edit documents)
within **2 seconds** at the 95th percentile of reconnection UNDER ≤ 1,000
accumulated offline changes per peer, exchanging only delta state.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-114]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**NFR-013: Collaboration Scale**

The system SHALL sustain a session of up to **8 concurrent peers** editing an
edit document of up to 500 shots across 50 sources
([[SPEC-001-transcript-video-editor#NFR-006]]) WITHOUT propagation latency
([[SPEC-003-realtime-collaborative-editing#NFR-009]]) degrading beyond its
stated bound and WITHOUT unbounded growth of CRDT metadata between
garbage-collection points.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-115]]
- [[SPEC-003-realtime-collaborative-editing#OBS-003]]

**NFR-014: Pairing Brute-Force Resistance**

The pairing mechanism SHALL limit an active attacker to **one** online guess of
the pairing phrase per pairing attempt (the single-guess [[PAKE]] property of
[[SPAKE2]]) and SHALL invalidate a channel after a bounded number (default 3) of
failed key confirmations, making offline dictionary attack against observed
rendezvous traffic computationally infeasible.

Trace:
- [[SPEC-003-realtime-collaborative-editing#TEST-116]]
- [[SPEC-003-realtime-collaborative-editing#TEST-117]]

---

## Architecture Decisions

### ADR-007: CRDT Representation and Library

**Status:** Accepted — merge-semantics claim **spike-confirmed** (OQ-1, 2026-06-18:
identity-preserving concurrent moves + SEC verified on Loro 1.13.1); transport
integration pending

**Context:** The edit document must be reformulated so concurrent edits from
N peers merge to identical state. The dominant operations are *insert shot*,
*remove shot*, *move shot* (reorder), *trim shot* (field update), and *append
note*. Reorder-preserving-identity is the hardest: a naïve list CRDT models a
move as delete + insert, which loses the shot's identity and any concurrent
edits to it.

**Options considered:**

| Option | Pros | Cons |
|--------|------|------|
| A. [[Loro]] (`MovableList` + `Map`) | `MovableList` models *move* as a first-class op preserving element identity — a near-exact fit for `move-segment`; built-in delta sync, [[Version Vector]]s, and an `UndoManager` scoped per peer (fits [[SPEC-003-realtime-collaborative-editing#REQ-086]]); fast, compact encoding | Younger ecosystem than Automerge; binary format churn between major versions |
| B. [[Automerge]] | Mature, widely deployed, strong formal grounding; rich JSON document model | List move is delete+insert (loses identity under concurrency — directly harms [[SPEC-003-realtime-collaborative-editing#REQ-080]]); needs an explicit move-as-tombstone workaround |
| C. yrs (Yjs Rust port) | Battle-tested in production collaborative editors | Y.Array also lacks identity-preserving move; model tuned for text |
| D. Hand-composed per-field CRDTs | Maximum control of merge semantics | Re-implements solved problems at the highest-novelty point of the system; violates Composition-First (Constitutional Principle 15); unbounded verification burden |

**Decision:** **Option A — [[Loro]]**, using a `MovableList` of shot containers,
each shot a `Map` of fields, notes as a `List`, and markers/POIs as `Map`s keyed
by ID (observed-remove via Loro's deletion semantics). The edit document's
materialised JSON view ([[DATA-MODEL]] §3) is computed from the Loro document.

**Rationale:** `MovableList` is the decisive differentiator — it makes
[[SPEC-003-realtime-collaborative-editing#REQ-080]] (identity-preserving
concurrent moves) a library primitive rather than a hand-rolled invariant, and
Loro's per-peer `UndoManager` directly serves
[[SPEC-003-realtime-collaborative-editing#REQ-086]]. The decision is provisional
on the spike confirming move/trim convergence under concurrency.

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-079]]
- [[SPEC-003-realtime-collaborative-editing#REQ-080]]
- [[SPEC-003-realtime-collaborative-editing#REQ-086]]

### ADR-008: Peer-to-Peer Transport — iroh

**Status:** Proposed

**Context:** Peers need direct, authenticated, NAT-traversing transport for both
small frequent CRDT deltas and large source blobs.

**Decision:** Use [[iroh]] ([[QUIC]]-based) for transport and [[iroh-blobs]] for
content-addressed bulk transfer. CRDT deltas travel on a bidirectional control
stream; blobs travel on iroh-blobs' [[BLAKE3]]-verified streams.

**Rationale:** iroh provides authenticated p2p with relay fallback (so pairing
succeeds even behind symmetric NAT), and iroh-blobs gives content-addressing,
dedup, resumability, and integrity verification for free —
directly serving [[SPEC-003-realtime-collaborative-editing#REQ-075]] and
[[SPEC-003-realtime-collaborative-editing#REQ-076]]. Multiplexing control and
bulk on one library avoids a second transport stack.

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-071]]
- [[SPEC-003-realtime-collaborative-editing#REQ-075]]
- [[SPEC-003-realtime-collaborative-editing#REQ-084]]

### ADR-009: Pairing — SPAKE2 Key Agreement

**Status:** Proposed — crypto feasibility **spike-confirmed** (OQ-1: matched
phrases agree, mismatched fail closed, handshake ~0.2 ms); remains a no-go
cryptographic area pending audited-impl + expert + cross-model review.
**Revised v1.1.0:** the handshake now runs over the direct iroh connection
established by [[SPEC-003-realtime-collaborative-editing#ADR-013]] discovery,
not over a rendezvous-server channel. The mechanism is **implemented and
loopback-tested** (`ar-edit-collab` `transport::pair_as_initiator/responder`,
CON-014: matching phrases agree on a session key over iroh, a wrong phrase
fails key confirmation); production acceptance still pends the review below.

**Context:** Users pair by communicating a short, low-entropy phrase
out-of-band. The two instances must locate each other and bootstrap a strong
key from that weak secret without a pre-shared certificate.

**Decision:** The full `<num>-<word>-<word>` phrase ([[BIP39]] words) is the
[[SPAKE2]] password. After the peers find each other via phrase-keyed [[pkarr]]
discovery ([[SPEC-003-realtime-collaborative-editing#ADR-013]]) and open a
direct [[iroh]] connection, they run [[SPAKE2]] **over that connection**,
exchange key-confirmation tags, and only then carry project data
([[SPEC-003-realtime-collaborative-editing#REQ-070]]).

**Alternatives considered:** (i) *Dedicated rendezvous server relaying SPAKE2*
(the original v1.0.0 magic-wormhole-style decision) — sound, but raises "who
hosts it?" (OQ-3) and adds infra; retained only as an optional DHT-blocked
fallback. (ii) *Manual NodeTicket paste* — loses the "read a phrase aloud"
ergonomics; MAY be offered as a fallback.

**Rationale:** [[SPAKE2]] gives the single-guess [[PAKE]] property
([[SPEC-003-realtime-collaborative-editing#NFR-014]]), so the weak phrase is
safe even though [[SPEC-003-realtime-collaborative-editing#ADR-013]] discovery
makes the phrase-derived key enumerable.
This is a **no-go cryptographic area** under [[PROTO-001]] AI Trust Boundaries —
audited implementation + cross-model + human expert review (Tier 1) required.
The in-house [[SPAKE2]] reference (`cbcl-bus` SPEC-007) informs open review
items: proof-of-possession binding the session to the iroh node key, transcript
binding, RFC 9382 / ristretto255 vs Ed25519Group, and a labelled KDF/AEAD.

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-067]]
- [[SPEC-003-realtime-collaborative-editing#REQ-069]]
- [[SPEC-003-realtime-collaborative-editing#REQ-070]]
- [[SPEC-003-realtime-collaborative-editing#ADR-013]]
- [[SPEC-003-realtime-collaborative-editing#NFR-014]]

### ADR-013: Serverless Discovery via Phrase-Keyed pkarr / Mainline DHT

**Status:** Proposed (v1.1.0) — **discovery implemented and loopback-tested**
(`ar-edit-collab` `shell::discovery`, feature `transport`: deterministic phrase
key, CON-017 record build/parse, in-process + pkarr-relay backends; end-to-end
discover-by-phrase → dial → delta verified). The live SPAKE2 over the dialed
connection remains pending the same crypto review as
[[SPEC-003-realtime-collaborative-editing#ADR-009]]

**Context:** ADR-009 v1.0.0 used a dedicated [[Rendezvous Server]] as the
meeting point, which left open "who operates it?" (OQ-3) and added a piece of
infrastructure. The sibling project `../did-crdt` (its ADR-006, REQ-013/014,
CON-006) demonstrates **serverless** discovery: derive a deterministic Ed25519
keypair from a known identifier, publish a signed [[pkarr]] record (DNS-shaped)
to the [[Mainline DHT]] (or a pkarr HTTP relay) advertising an [[iroh]]
NodeAddr, and let anyone who knows the identifier derive the same key and look
it up. [[iroh]] already uses pkarr for its own node discovery.

**Decision:** Adopt the `did-crdt` pattern, keyed by the **pairing phrase**:
- `seed = blake3("ar-edit/pair/discovery/v1" ‖ phrase)`; `(sk, pk) = Ed25519(seed)`.
- The host publishes a [[pkarr]] record under `pk` carrying its iroh NodeId +
  direct addresses + relay (the dialling hints), refreshed before expiry, with a
  `DISABLE_DHT_PUBLISH` opt-out.
- The joiner derives the same `(sk, pk)`, looks the record up on the
  [[Mainline DHT]], dials the host over iroh, then runs SPAKE2 over the
  connection ([[SPEC-003-realtime-collaborative-editing#ADR-009]]).

**Trade-off (the load-bearing review item):** unlike `did-crdt`'s high-entropy
DID key, ar-edit's phrase is **low-entropy, so the discovery key is
enumerable** — an attacker can derive candidate phrases, find live sessions, and
learn the host's IP. This is mitigated and judged acceptable because: SPAKE2
still limits an attacker to one online guess (NFR-014); the phrase has a short
TTL (REQ-068, default 10 min); the session burns on the first failed
confirmation; and `DISABLE_DHT_PUBLISH` lets the privacy-sensitive opt out. The
net posture equals magic-wormhole's low-entropy mailbox channel — security comes
from the PAKE, not from channel secrecy. (This reverses the v1.0.0 rejection of
"iroh-native discovery keyed by the phrase" in ADR-009, now that the concrete
`did-crdt` pattern + these mitigations are in hand.)

**Consequences:** no server to host (**resolves OQ-3**); the s7 TCP relay is
demoted to an optional fallback for networks that block the DHT; host IP is
exposed on a public DHT (privacy, mitigated by opt-out); a real-DHT
discovery+latency run is folded into
[[SPEC-003-realtime-collaborative-editing#OQ-7]].

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-068]]
- [[SPEC-003-realtime-collaborative-editing#REQ-069]]
- [[SPEC-003-realtime-collaborative-editing#REQ-071]]
- [[SPEC-003-realtime-collaborative-editing#CON-017]]

### ADR-014: Session Process Model (TUI host + background daemon)

**Status:** Proposed (v1.1.0)

**Context:** A realtime session needs a persistent owner of the live [[CRDT]]
document, the peer connections, and the [[pkarr]] record refresh. ar-edit's
CLI/agent surface, however, is one-shot: `ar-edit edit add-segment …` opens the
file, mutates, and exits — it cannot hold live state or connections between
invocations. Without a defined process model, `ar-edit share` and discrete CLI
edits cannot participate in the same live session.

**Decision:** Support **both** owners:
- **Interactive / human:** the TUI (or a blocking `ar-edit share`) is itself the
  long-running host — no separate process needed.
- **Headless / agent:** a background **session daemon** (`ar-edit daemon`) owns
  the live `CollabDoc`, the transport, and pkarr refresh, and exposes a local
  IPC endpoint (Unix-domain socket under the project dir;
  [[SPEC-003-realtime-collaborative-editing#CON-018]]). Discrete `edit`/`session`
  commands attach to it over IPC ([[SPEC-003-realtime-collaborative-editing#REQ-090]]),
  and fall back to one-shot on-disk operations when no daemon is running.

Local IPC mutations and remote CRDT deltas are applied to the *same* in-memory
document, so they converge by the CRDT's own merge — no extra coordination.

**Alternatives considered:** *daemon-only* (rejected — heavyweight for plain
single-player CLI/agent file edits, which must keep working with no daemon);
*TUI-only host* (rejected — excludes headless/agent collaboration entirely).

**Consequences:** introduces a daemon lifecycle and the IPC contract
([[SPEC-003-realtime-collaborative-editing#CON-018]]); the daemon owns pkarr
refresh and peer acceptance; reframes the "waiting state" of
[[SPEC-003-realtime-collaborative-editing#REQ-068]]. Lifecycle specifics
(auto-start, idle shutdown, one-per-project) are
[[SPEC-003-realtime-collaborative-editing#OQ-8]].

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-068]]
- [[SPEC-003-realtime-collaborative-editing#REQ-089]]
- [[SPEC-003-realtime-collaborative-editing#REQ-090]]
- [[SPEC-003-realtime-collaborative-editing#CON-018]]

### ADR-010: Full Content-Addressed Media Replication

**Status:** Proposed — hashing/integrity ceiling **spike-confirmed** (OQ-1:
BLAKE3 ≥ 1.85 GB/s on 2 GB, verified-streaming digest matches one-shot);
network-transfer throughput deferred to [[SPEC-003-realtime-collaborative-editing#OQ-7]]

**Context:** Peers may not start with the same source files. To render any shot
locally, a peer needs the underlying media. Source files are large (GB-scale).

**Decision:** Replicate full source media by [[BLAKE3]] content address via
[[iroh-blobs]], deduplicating shared bytes and verifying integrity on receipt
([[SPEC-003-realtime-collaborative-editing#REQ-076]]). Derived artefacts
(transcripts, indices, thumbnails) replicate the same way.

**Alternatives considered:** *derived-only* (assume media present out-of-band)
and *lazy on-demand*. Full replication was chosen because the stakeholder
requires both peers to independently render, and content addressing makes the
"already have it" case free. Lazy fetching MAY be layered on later as an
optimisation (tracked in [[SPEC-003-realtime-collaborative-editing#OQ-2]]).

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-074]]
- [[SPEC-003-realtime-collaborative-editing#REQ-075]]
- [[SPEC-003-realtime-collaborative-editing#REQ-077]]

### ADR-011: Edit-Document Migration — Event-Sourced → CRDT

**Status:** **Accepted** (implemented, IMPL-003) — **supersedes the mutation
model of [[ADR-001-event-sourced-edits]]**

**Context:** [[ADR-001-event-sourced-edits]] assumed a single writer and a total
op order. Multiplayer breaks both. The non-destructive *intent* of ADR-001 must
be preserved.

**Decision:** Make the [[Loro]] [[CRDT]] document the canonical edit
representation ([[SPEC-003-realtime-collaborative-editing#REQ-079]]).
Single-player editing is the one-actor degenerate case. Existing event-sourced
documents migrate on first open with snapshot-identical results
([[SPEC-003-realtime-collaborative-editing#REQ-088]]). Undo/redo move from a
shared head pointer to a per-actor [[Loro]] `UndoManager`
([[SPEC-003-realtime-collaborative-editing#REQ-086]]).

**Consequences:** The on-disk format gains a CRDT payload; the JSON
materialised view remains the read/agent surface, so
[[SPEC-001-transcript-video-editor#REQ-028]] (`--json`) and downstream tooling
are unaffected. [[SPEC-001-transcript-video-editor]] REQ-046–048 require a
version bump (tracked in [[SPEC-003-realtime-collaborative-editing#OQ-4]]).

**Implementation note (IMPL-003): two undo regimes, deliberately decoupled.**
Undo is *durable* only in the single-writer / offline case (the long-standing
local-first norm; it is just persisting the command history). It is implemented
as a head cursor over the [[Loro]] oplog using `revert_to`, which emits *local*
ops to move state — staying attached/editable (unlike `checkout`, which
detaches and rekeys the peer) and durable in the snapshot. In a *live* session
undo is the per-actor [[Loro]] `UndoManager` and is **ephemeral / session-scoped**
— matching every collaborative editor (Yjs, Loro, Figma, Google Docs all wipe
the undo stack on reload and use *version history* for cross-session recovery).
"Durable online undo" — undo coordinated across a live session and surviving
restart — is **explicitly out of scope**; attempting it (coordinating a durable
file cursor with a live daemon's stack) was the root of the review #5–#8
divergence churn and is not how the field solves this. The two regimes never
run for the same edit at once (a one-shot CLI edit operates on the file; a live
host owns the edit — see [[SPEC-003-realtime-collaborative-editing#OQ-8]]).

Trace:
- [[SPEC-003-realtime-collaborative-editing#REQ-079]]
- [[SPEC-003-realtime-collaborative-editing#REQ-086]]
- [[SPEC-003-realtime-collaborative-editing#REQ-088]]
- [[ADR-001-event-sourced-edits]]

---

## Contract Specifications

Every contract below that accepts external input declares a formal grammar and
recognises input fully before any semantic action, per [[LangSec]]
(Constitutional Principle 14). Inputs arriving over the network or from the
human at the pairing boundary are untrusted.

### CON-012: Collaboration CLI

#### `ar-edit share [--name <display-name>] [--ttl <minutes>]`

Opens the current project for collaboration and prints a pairing phrase.

```
Pre-conditions:  Inside a valid project directory; rendezvous reachable
Post-conditions: Pairing phrase generated; host registered on a rendezvous
                 channel; host enters accept loop; phrase invalidated after TTL
Exit codes:      0 = session closed cleanly, 1 = not a project, 2 = rendezvous
                 unreachable
Output (--json): { "phrase": "7-saturn-pioneer", "channel": 7,
                   "node_id": "<iroh-node-id>", "expires": "<iso8601>" }
```

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-067]],
[[SPEC-003-realtime-collaborative-editing#REQ-068]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-074]],
[[SPEC-003-realtime-collaborative-editing#TEST-076]]

#### `ar-edit pair <phrase> [--name <display-name>]`

Joins a session identified by the phrase.

```
Pre-conditions:  <phrase> matches the CON-013 grammar; inside a project
                 directory; rendezvous reachable
Post-conditions: SPAKE2 completed; direct iroh connection established; source
                 sync begun; peer admitted to the session
Exit codes:      0 = joined and synced, 1 = malformed phrase (rejected before
                 any network action), 2 = rendezvous unreachable, 3 = key
                 confirmation failed (wrong phrase), 4 = no iroh path / timeout
Output (--json): { "session": "<id>", "peers": [...], "actor_id": "<id>",
                   "sync": { "sources_total": N, "sources_present": M } }
```

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-069]],
[[SPEC-003-realtime-collaborative-editing#REQ-070]],
[[SPEC-003-realtime-collaborative-editing#REQ-078]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-077]],
[[SPEC-003-realtime-collaborative-editing#TEST-078]],
[[SPEC-003-realtime-collaborative-editing#TEST-079]]

#### `ar-edit session status` / `ar-edit session peers` / `ar-edit session leave`

Reports session membership/sync state, or leaves the session.

```
Output (--json, status): { "session": "<id>", "synced": true,
  "peers": [{ "actor_id": "<id>", "name": "alice", "selected_shot": "shot-003",
              "connected": true }],
  "sync": { "percent": 100, "blobs_remaining": 0 } }
Exit codes:      0 = success, 1 = no active session
```

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-073]],
[[SPEC-003-realtime-collaborative-editing#REQ-085]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-084]],
[[SPEC-003-realtime-collaborative-editing#TEST-104]]

### CON-013: Pairing Phrase Grammar [LangSec]

The pairing phrase is untrusted input recognised **fully** before any network
action. Grammar (ABNF, after [[RFC 5234]]):

```abnf
phrase      = channel "-" word "-" word
channel     = 1*3DIGIT          ; parsed to integer 0..=999, leading zeros ok
word        = 1*( %x61-7A )     ; lowercase ASCII letters
                                ; AND member-of the audited wordlist
```

Recognition rules:
- The recogniser MUST verify `channel` ∈ [0, 999] and each `word` ∈ the
  compiled wordlist before emitting a typed `Phrase { channel: u16,
  words: [WordId; 2] }`. Any deviation ⇒ parse error, exit code 1, **no network
  action** (Constitutional Principle 14, fail-closed).
- No normalisation of malformed input (no trimming of extra segments, no case
  folding beyond the grammar, no fuzzy word matching). Reject, do not repair.
- The `Phrase` value — never the raw string — crosses into the pairing logic.

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-067]],
[[SPEC-003-realtime-collaborative-editing#REQ-069]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-075]],
[[SPEC-003-realtime-collaborative-editing#TEST-078]]

### CON-014: Pairing Handshake over iroh [LangSec]

The [[SPAKE2]] handshake frames exchanged over the **direct iroh connection**
after [[SPEC-003-realtime-collaborative-editing#ADR-013]] discovery. Each frame
is length-prefixed and externally tagged; the recogniser parses the full frame
before any cryptographic action.

```abnf
frame       = u32-len body          ; u32-len = big-endian length of body
body        = tag payload           ; tag = single octet message type
; tags: 0x30 PAKE-MSG(opaque)   ; SPAKE2 message
;       0x31 CONFIRM(mac-32)    ; key-confirmation tag (32 bytes)
;       0x32 ERROR(code)
```

Recognition rules:
- A frame whose declared length exceeds a fixed maximum (default 64 KiB) is
  rejected without buffering (DoS guard).
- `PAKE-MSG` is fed to the [[SPAKE2]] state machine; `CONFIRM` MUST be exactly
  32 octets and is compared in constant time (review item) before any project
  bytes flow.
- Unknown tags ⇒ `ERROR` and connection close. No partial dispatch.

**Optional fallback (DHT-blocked networks):** where the [[Mainline DHT]] is
unreachable, a [[Rendezvous Server]] MAY relay these same opaque frames between
peers (matching them on the `<num>` prefix as a channel), never interpreting
PAKE payloads. This is the demoted v1.0.0 path
([[SPEC-003-realtime-collaborative-editing#ADR-013]]). The relay SHALL free a
channel whose waiting peer disconnects before a partner arrives (so the next
peer is not matched to a dead waiter), and SHALL buffer any frames a waiter
sends before its partner arrives, relaying them once paired rather than
discarding them.

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-070]],
[[SPEC-003-realtime-collaborative-editing#REQ-071]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-081]],
[[SPEC-003-realtime-collaborative-editing#TEST-080]]

### CON-017: pkarr Discovery Record [LangSec]

The signed discovery record published to / looked up from the [[Mainline DHT]]
(or a pkarr relay), keyed by the phrase-derived public key
([[SPEC-003-realtime-collaborative-editing#ADR-013]]). A looked-up record is
**untrusted input** and MUST be fully recognised before any dialling. Format
(a [[pkarr]] signed DNS packet; one `key=value` per TXT character string, after
`../did-crdt` CON-006):

```abnf
record   = "_ar-edit-pair" TXT 1*attr
attr     = "v=1"
         / "nid=" node-id          ; iroh NodeId (z-base-32)
         / "relay=" url            ; optional relay URL
         / "addrs=" addr *("," addr)   ; optional direct socket addresses
addr     = ip ":" port
```

Recognition rules:
- The record's pkarr/Ed25519 **signature MUST verify against the phrase-derived
  public key**, and the embedded timestamp MUST be within the freshness window,
  before any field is used (fail-closed; reject stale or unsigned records).
- `nid` MUST parse as a valid iroh NodeId; `addrs`/`relay` are **unauthenticated
  dialling hints** — a forged hint costs only a failed iroh handshake (which
  authenticates the NodeId), never a trust escalation.
- Unknown attributes are ignored (forward-compat), but a record missing `v` or
  `nid` is rejected.

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-068]],
[[SPEC-003-realtime-collaborative-editing#REQ-069]],
[[SPEC-003-realtime-collaborative-editing#REQ-071]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-081]]

### CON-018: Session Daemon IPC [LangSec]

The local control channel between discrete CLI/agent clients and the session
daemon ([[SPEC-003-realtime-collaborative-editing#ADR-014]]). Transport is a
**Unix-domain socket** at `<project>/.ar-edit/session.sock` (mode 0600; named
pipe on Windows). Each message is a length-prefixed JSON frame, fully recognised
into a typed request before any mutation (fail-closed, Constitutional Principle 14).

```abnf
frame    = u32-len json-body        ; u32-len big-endian; cap 1 MiB
request  = attach / mutate / read   ; externally-tagged on "op"
attach   = %s'{"op":"attach","edit":' string '}'
mutate   = add-shot / move-shot / trim-shot / add-note / remove-shot
add-shot = %s'{"op":"add_shot","shot":' shot-json '}'
read     = %s'{"op":"snapshot"}' / %s'{"op":"status"}'
response = ok / snapshot / status / error    ; tagged JSON
```

Recognition rules:
- A frame whose declared length exceeds the cap is rejected without buffering.
- The JSON body MUST deserialise to a known `op`; an unknown/!malformed request
  yields an `error` response and **performs no mutation**.
- The socket is created mode 0600 in the project directory: the trust boundary
  is local filesystem access (a connecting client is already a local user with
  project access). The daemon never executes payload-supplied paths or code.
- Local-mutation and remote-delta application share one in-memory document, so
  they converge via the CRDT (no IPC-side conflict handling).

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-089]],
[[SPEC-003-realtime-collaborative-editing#REQ-090]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-119]],
[[SPEC-003-realtime-collaborative-editing#TEST-120]]

### CON-015: CRDT Sync Message Envelope [LangSec]

Messages exchanged over the authenticated direct iroh control stream. All
project bytes on this stream are encrypted under the iroh session; the
application envelope is still recognised fully before application.

```abnf
envelope    = u32-len version tag payload
version     = u8                    ; protocol version, reject if unknown
tag         = u8
; tags: 0x10 HELLO(actor-id, version-vector)
;       0x11 DELTA(crdt-update-bytes)     ; opaque to envelope, fed to Loro import
;       0x12 SYNC-REQ(version-vector)     ; request missing changes
;       0x13 PRESENCE(actor-id, ephemeral-state)
;       0x14 BYE(actor-id)
```

Recognition rules:
- `version` mismatch ⇒ structured error and refusal to apply (no best-effort
  partial parse across versions — protocol divergence guard).
- `DELTA` payloads are handed to the [[Loro]] importer, which itself validates
  the CRDT update; a [[Loro]] import error ⇒ the delta is rejected and the
  sender asked to resend (the envelope never half-applies a delta).
- `PRESENCE` state is ephemeral and MUST NOT mutate persisted CRDT state.
- Frame length cap is **`MAX_SYNC_FRAME` (64 MiB)**, not the small control-frame
  cap: a first-contact snapshot or an offline batch of deltas can far exceed
  64 KiB, so the sync envelope uses the larger bound (matching the transport
  read cap). Oversized-beyond-64-MiB frames are still rejected.

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-084]],
[[SPEC-003-realtime-collaborative-editing#REQ-085]],
[[SPEC-003-realtime-collaborative-editing#REQ-087]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-103]],
[[SPEC-003-realtime-collaborative-editing#TEST-107]]

### CON-016: Source-Sync / Blob Request Protocol [LangSec]

Source-inventory reconciliation and blob requests over iroh.

```abnf
sync-msg    = u32-len tag payload
; tags: 0x20 MANIFEST(entry*)        ; entry = src-id blake3-hash duration codec
;       0x21 WANT(blake3-hash*)      ; blobs this peer is missing
;       0x22 HAVE(blake3-hash*)      ; blobs this peer can serve
;       0x23 CONFLICT(src-id, hash-a, hash-b)   ; same id, different content
blake3-hash = 32OCTET               ; fixed 32-byte BLAKE3 digest
```

Recognition rules:
- A `blake3-hash` MUST be exactly 32 octets; any other length ⇒ reject frame.
- A received blob is admitted only after its recomputed [[BLAKE3]] digest equals
  the requested hash ([[SPEC-003-realtime-collaborative-editing#REQ-076]]);
  otherwise quarantine + structured error, never written to `sources/`.
- A `MANIFEST` mapping one `src-id` to two distinct hashes across peers ⇒
  `CONFLICT`, surfaced to the user; no silent overwrite
  ([[SPEC-003-realtime-collaborative-editing#REQ-074]]).

Implements: [[SPEC-003-realtime-collaborative-editing#REQ-074]],
[[SPEC-003-realtime-collaborative-editing#REQ-075]],
[[SPEC-003-realtime-collaborative-editing#REQ-076]],
[[SPEC-003-realtime-collaborative-editing#REQ-077]]
Verified by: [[SPEC-003-realtime-collaborative-editing#TEST-085]],
[[SPEC-003-realtime-collaborative-editing#TEST-088]],
[[SPEC-003-realtime-collaborative-editing#TEST-089]]

---

## Test Specifications

Each requirement is decomposed into positive, negative-input, and
negative-output tests per [[PROTO-001]]'s Requirement-Targeted Test
Decomposition. Property-based and fuzz tests are mandated where noted
(CRDT convergence, parser boundaries). `Validates:` records the
requirement-attribution map π.

**TEST-074: Pairing phrase generation (positive)**
Generate 10,000 phrases; verify all match [[SPEC-003-realtime-collaborative-editing#CON-013]],
`channel` ∈ [0,999], both words ∈ wordlist, and the generator uses a CSPRNG.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-067]]

**TEST-075: Pairing phrase grammar — recognition (negative-input + fuzz)**
Fuzz the phrase recogniser with malformed inputs (extra segments, non-words,
out-of-range channel, unicode, oversized). Verify every reject returns exit 1
and triggers **no network action**. Property: only grammar-conformant,
wordlist-member phrases parse.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-067]],
[[SPEC-003-realtime-collaborative-editing#REQ-069]]

**TEST-076: Session hosting & TTL (positive + negative-output)**
`ar-edit share` registers a channel and emits a valid phrase; after TTL the
phrase is invalidated (a join attempt with it fails). Negative-output: a join
after expiry MUST NOT admit the peer.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-068]]

**TEST-077: Session joining (positive)**
Two instances pair on the same phrase; verify a direct iroh connection and
admission with a unique actor ID.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-069]]

**TEST-078: Join with malformed phrase (negative-input)**
`ar-edit pair "not a phrase"` exits 1 before contacting the rendezvous (assert
no socket opened).
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-069]]

**TEST-079: SPAKE2 key agreement (positive)**
Matching phrases on both peers yield equal session keys; the data channel opens
only after key confirmation.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-070]]

**TEST-080: SPAKE2 wrong phrase / eavesdropper (negative-output)**
A joiner with a wrong phrase fails key confirmation (exit 3) and receives **no**
project bytes. An observer recording all rendezvous traffic cannot derive the
key (cryptographic test vector / property).
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-070]],
[[SPEC-003-realtime-collaborative-editing#NFR-014]]

**TEST-081: Rendezvous relay then direct handoff (positive + negative-output)**
Verify project data flows only after handoff to the direct iroh connection and
that the rendezvous process observes **zero** project bytes and no session key.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-071]]

**TEST-082: Actor ID uniqueness & stability (positive)**
In an N-peer session, every actor ID is unique and stable across reconnects of
the same peer.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-072]]

**TEST-083: Actor-ID tie-break determinism (negative-output)**
Two peers make truly concurrent conflicting changes; verify both converge to the
**same** winner determined by actor ID, and that the tie-break does not imply a
false causal order.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-072]],
[[SPEC-003-realtime-collaborative-editing#REQ-081]]

**TEST-084: Membership lifecycle (positive)**
Peers join/leave/timeout; all peers observe consistent membership; a departed
peer's prior changes remain.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-073]]

**TEST-085: Manifest reconciliation (positive)**
Two peers with overlapping-but-different source sets reconcile to a merged
manifest mapping each src-id to one hash.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-074]]

**TEST-086: Source-id/hash conflict (negative-input)**
Same src-id with different content hashes raises a structured `CONFLICT`, no
silent overwrite.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-074]]

**TEST-087: Content-addressed replication & dedup (positive)**
Missing blobs transfer; identical-hash blobs incur zero transfer; transfer
resumes after an interrupted connection.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-075]]

**TEST-088: Blob integrity accept (positive)**
A blob whose recomputed [[BLAKE3]] matches is admitted to `sources/`.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-076]]

**TEST-089: Blob integrity reject (negative-output + fuzz)**
A corrupted/mismatched blob is rejected, quarantined, never written to
`sources/`, and never referenced by an edit. Fuzz the blob-receive path.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-076]]

**TEST-090: Derived-artefact replication (positive)**
Transcripts/indices/thumbnails replicate by hash; identical artefacts skip
transfer; ranges resolve locally on the receiver.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-077]]

**TEST-091: Sync gating & progress (positive + negative-output)**
Edit mutations are rejected until sync readiness; progress JSON is monotonic and
reaches 100%. Negative-output: a mutation referencing an un-synced source is not
applied.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-078]]

**TEST-092: CRDT ⇄ JSON materialisation roundtrip (positive, property)**
For arbitrary edit states, the materialised JSON equals the JSON produced by the
single-player path; `materialise(crdt)` is deterministic.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-079]]

**TEST-093: Read-side commands unchanged (positive)**
`edit show`, `validate`, `render` produce identical output against a CRDT-backed
doc and its equivalent legacy snapshot.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-079]]

**TEST-094: Concurrent insert convergence (positive, property)**
Random interleavings of concurrent shot inserts by N actors converge to
identical order on all peers; no insert lost or duplicated.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-080]]

**TEST-095: Identity-preserving move (positive)**
A shot moved by peer A while peer B edits its range converges with the move
applied AND B's range edit retained on the **same** shot identity.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-080]],
[[SPEC-003-realtime-collaborative-editing#REQ-081]]

**TEST-096: Concurrent moves of same shot (negative-output)**
Two peers move the same shot to different positions; result is one deterministic
position on all peers, never two copies of the shot.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-080]]

**TEST-097: Concurrent range LWW (positive)**
Concurrent trims of one shot converge to a single range tie-broken by actor ID.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-081]]

**TEST-098: No interleaved invalid range (negative-output)**
Verify a merged range is always one writer's intact `{from,to}` — never a
`from` from one writer with a `to` from another producing `from > to`.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-081]]

**TEST-099: Marker/POI set convergence (positive, property)**
Concurrent add/remove of markers and POIs across peers converge to observed-
remove semantics; no surviving removed item, no lost concurrent add.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-082]]

**TEST-100: Append-only notes never lost (negative-output)**
Concurrent note appends to the same shot all survive merge in a deterministic
order; no note dropped.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-082]]

**TEST-101: Strong eventual consistency (property)**
For random change sets delivered in random orders/duplications to N peers, all
peers reach byte-identical materialised documents.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-083]]

**TEST-102: Merge requires no human resolution (negative-output)**
No sequence of concurrent edits produces a state flagged for manual conflict
resolution on the edit document, markers, or POIs.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-083]]

**TEST-103: Live propagation (positive)**
A commit on peer A appears in peer B's timeline panel via an incremental delta
(no full resend), asserted by message inspection.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-084]]

**TEST-104: Presence (positive + negative-output)**
Peers see each other's connected state and selected shot; on disconnect the
presence entry disappears and presence never persists into the edit document.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-085]]

**TEST-105: Per-actor undo (positive)**
Peer A's undo reverts only A's last change; B's concurrent change is untouched;
the inverse propagates to all peers.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-086]]

**TEST-106: Undo does not revert others (negative-output)**
Verify A's undo cannot revert a change originated by B.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-086]]

**TEST-107: Offline edit + reconnect merge (positive)**
A peer edits while partitioned; on reconnect only deltas are exchanged and all
peers converge with no loss.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-087]]

**TEST-108: Delta-only reconnect (negative-output)**
Assert no full-document resend occurs on reconnect (message-size/inspection
bound).
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-087]]

**TEST-109: Migration snapshot identity (positive)**
Migrating each fixture event-sourced edit yields a materialised shot list
identical to its pre-migration `snapshot`.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-088]]

**TEST-110: Single-player continuity (positive)**
All [[SPEC-001-transcript-video-editor]] edit-document tests pass against the
one-actor CRDT backend unchanged.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-088]]

**TEST-111: Propagation latency (NFR)**
Measure commit→remote-render latency across an 8-peer session; assert p95 ≤
400 ms under ≤ 100 ms RTT.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-009]]

**TEST-112: Pairing latency (NFR)**
Measure pair→connected; assert p95 ≤ 5 s; assert structured timeout on no path.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-010]]

**TEST-113: Media sync throughput (NFR)**
Transfer a 2 GB source set; assert ≥ 80% of available bandwidth and zero
transfer for identical-hash blobs.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-011]]

**TEST-114: Convergence bound (NFR)**
Heal a partition with ≤ 1,000 offline changes/peer; assert convergence p95 ≤
2 s, delta-only.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-012]]

**TEST-115: Scale (NFR)**
8 peers, 500 shots, 50 sources; assert [[SPEC-003-realtime-collaborative-editing#NFR-009]]
holds and CRDT metadata stays bounded across GC points.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-013]]

**TEST-116: Single-guess PAKE (security)**
An active attacker gets at most one online guess per attempt; assert against the
SPAKE2 implementation's test vectors.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-014]]

**TEST-117: Channel lockout (security, negative-output)**
After the configured number of failed key confirmations the channel is
invalidated and further guesses are refused.
Validates: [[SPEC-003-realtime-collaborative-editing#NFR-014]]

**TEST-119: Daemon holds a live session across one-shot clients (positive)**
Start a session daemon over a socket holding an edit; client A applies
`add_shot`; a *separate* client B requests `snapshot` and sees A's shot —
proving the daemon maintains shared live state between discrete invocations
(REQ-089).
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-089]]

**TEST-120: IPC recognition + attach (positive + negative-input)**
A well-formed request mutates and returns `ok`; a malformed or oversized frame
is rejected with an `error` and leaves the document unchanged (LangSec,
CON-018). With no daemon running, an `edit` command falls back to the on-disk
document.
Validates: [[SPEC-003-realtime-collaborative-editing#REQ-090]],
[[SPEC-003-realtime-collaborative-editing#CON-018]]

### Selected Verification Techniques

| Surface | Techniques (per [[PROTO-001]]) |
|---------|-------------------------------|
| CRDT merge (pure core) | **Property-based** (convergence, commutativity, idempotence) + **mutation testing** |
| Phrase / wire-protocol parsers | **Fuzzing** + property-based roundtrip — trust boundaries, auto Tier 1–2 |
| SPAKE2 / key agreement | Cryptographic **test vectors** + cross-model + human expert review (no-go area) |
| Blob transfer | Integrity property tests + fuzz on receive path |
| Latency / throughput / scale | Instrumented benchmarks tied to [[SPEC-003-realtime-collaborative-editing#OBS-003]] |

---

## Observability

**OBS-003: Collaboration Telemetry**

The system SHALL emit structured, opt-in-local metrics/logs for: pairing
outcome and latency, per-peer sync bytes and duration, change-propagation
latency (commit→remote-apply), convergence time after reconnect, active peer
count, and rejected frames/blobs (with reason) — sufficient to verify
[[SPEC-003-realtime-collaborative-editing#NFR-009]] through
[[SPEC-003-realtime-collaborative-editing#NFR-014]] and to diagnose merge or
transport faults. No project content is included in telemetry.

Trace:
- [[SPEC-003-realtime-collaborative-editing#NFR-009]]
- [[SPEC-003-realtime-collaborative-editing#NFR-011]]
- [[SPEC-003-realtime-collaborative-editing#NFR-012]]

---

## Purity Boundary Map

### Pure Core (no I/O, no shared state, deterministic)

- **CRDT merge & materialisation**: apply remote delta → new CRDT state;
  `materialise(crdt) → EditSnapshot` (deterministic; the property-tested heart
  of [[SPEC-003-realtime-collaborative-editing#REQ-083]]).
- **Pairing phrase recogniser**: `&str → Result<Phrase, ParseError>`
  ([[SPEC-003-realtime-collaborative-editing#CON-013]]).
- **Wire-frame recognisers**: `&[u8] → Result<Frame, ParseError>` for CON-014,
  CON-015, CON-016 envelopes.
- **Manifest reconciliation**: `(Manifest, Manifest) → MergedManifest |
  Conflict`.
- **Migration**: `EventSourcedDoc → CrdtDoc` such that
  `materialise == old snapshot`.
- **Actor-ID tie-break ordering**: total order over concurrent changes.

### Effectful Shell (orchestrates I/O, calls pure core)

- SPAKE2 handshake and rendezvous client (network).
- iroh connection management; iroh-blobs transfer (network, disk).
- BLAKE3 hashing of files (disk read).
- CRDT delta send/receive over iroh streams (network).
- Persisting CRDT state and materialised JSON to disk.
- TUI presence rendering and live update dispatch.

### Boundary Contracts (data types crossing the boundary)

- `Phrase` (in): validated pairing phrase.
- `Frame` / `Envelope` / `SyncMsg` (in/out): recognised wire messages.
- `CrdtDelta` (in/out): opaque-to-shell, validated by the [[Loro]] importer.
- `EditSnapshot` (out): materialised view consumed by existing read commands.
- `MergedManifest` / `Conflict` (out).

### Dependency Rule

Dependencies point inward: shell → core. Core MUST NOT import iroh, the
rendezvous client, or any I/O type. Merge/materialisation MUST be testable with
zero network or disk.

### Enforcement

A new `ar-edit-collab` crate (or module) holds the pure core behind a trait
boundary; the effectful transport lives in the shell. CI arch-lint forbids core
→ shell imports.

---

## Traceability Matrix

```
REQ-067 → TEST-074, TEST-075 → CON-012, CON-013
REQ-068 → TEST-076           → CON-012, CON-017 ; ADR-013
REQ-069 → TEST-077, TEST-078 → CON-012, CON-013, CON-017 ; ADR-013
REQ-070 → TEST-079, TEST-080 → CON-014 ; NFR-014
REQ-071 → TEST-081           → CON-014, CON-017 ; ADR-013
REQ-072 → TEST-082, TEST-083
REQ-073 → TEST-084           → CON-012
REQ-074 → TEST-085, TEST-086 → CON-016
REQ-075 → TEST-087           → CON-016 ; NFR-011 ; OBS-003
REQ-076 → TEST-088, TEST-089 → CON-016
REQ-077 → TEST-090           → CON-016
REQ-078 → TEST-091           → CON-012 ; NFR-011 ; OBS-003
REQ-079 → TEST-092, TEST-093 → ADR-007, ADR-011
REQ-080 → TEST-094, TEST-095, TEST-096
REQ-081 → TEST-097, TEST-098
REQ-082 → TEST-099, TEST-100
REQ-083 → TEST-101, TEST-102 ; NFR-012
REQ-084 → TEST-103           → CON-015 ; NFR-009 ; OBS-003
REQ-085 → TEST-104           → CON-012, CON-015
REQ-086 → TEST-105, TEST-106
REQ-087 → TEST-107, TEST-108 → CON-015 ; NFR-012
REQ-088 → TEST-109, TEST-110 → ADR-011
NFR-009 → TEST-111 → OBS-003
NFR-010 → TEST-112 → OBS-003
NFR-011 → TEST-113 → OBS-003
NFR-012 → TEST-114 → OBS-003
NFR-013 → TEST-115 → OBS-003
NFR-014 → TEST-116, TEST-117
```

Every REQ has ≥ 1 TEST per applicable type; the π map is total over the
artefacts introduced here.

---

## Ambiguity Log

| Original Term | Resolution |
|---------------|------------|
| "realtime" | Change visible on peers within 400 ms p95 ([[SPEC-003-realtime-collaborative-editing#NFR-009]]) |
| "multiplayer" | Up to 8 concurrent peers ([[SPEC-003-realtime-collaborative-editing#NFR-013]]); N-peer model, not 2-peer |
| "sync the source materials" | Full content-addressed media + derived-artefact replication via [[iroh-blobs]], verified by [[BLAKE3]] ([[SPEC-003-realtime-collaborative-editing#REQ-075]], [[SPEC-003-realtime-collaborative-editing#REQ-077]]) |
| "collection of CRDTs" | One [[Loro]] document: `MovableList` of shots, `Map` per shot, `List` of notes, `Map` sets for markers/POIs ([[SPEC-003-realtime-collaborative-editing#ADR-007]]) |
| "connect two ar-edit instances" | Pairing is 2-at-a-time but the session is N-peer; each `pair` admits one more peer |
| "secure pairing" | Single-guess [[PAKE]] via [[SPAKE2]] over direct [[iroh]]; phrase-keyed [[pkarr]] discovery; burn-on-failed-confirmation ([[SPEC-003-realtime-collaborative-editing#NFR-014]], [[SPEC-003-realtime-collaborative-editing#ADR-013]]) |
| "wordlist" | [[BIP39]] English (2048 words); the whole `<num>-<word>-<word>` phrase is the secret |
| "converged" | Byte-identical materialised edit documents given the same observed change set ([[SPEC-003-realtime-collaborative-editing#REQ-083]]) |

---

## Open Questions

| # | id | Question | Status |
|---|----|----------|--------|
| 1 | OQ-1 | De-risking spike: confirm [[Loro]] `MovableList` move/trim convergence, SPAKE2 feasibility, and 2 GB BLAKE3 content-addressing. | **Resolved 2026-06-18** — H-a/H-b/H-c PASS (`spike/oq-1/FINDINGS.md`). Transport-layer empirical run split out to OQ-7. |
| 7 | OQ-7 | Transport-characterisation spike: real iroh pairing latency ([[SPEC-003-realtime-collaborative-editing#NFR-010]]) and iroh-blobs 2 GB network throughput / resumability ([[SPEC-003-realtime-collaborative-editing#NFR-011]]). Run on macOS, Linux, **and Windows** to confirm [[SPEC-001-transcript-video-editor#NFR-015]] (terminal backend, source-linking via [[ADR-012-cross-platform-source-linking]]) for the collaboration path. Also covers a real [[Mainline DHT]] / [[pkarr]] discovery round-trip ([[SPEC-003-realtime-collaborative-editing#ADR-013]]). | **Partially addressed** (IMPL-003): the iroh transport + content-addressed blob transfer are implemented and **loopback-tested** (`crates/ar-edit-collab`, feature `transport`; requires rustc ≥ 1.91). Remaining: real-network latency/throughput, pkarr/DHT discovery round-trip, and iroh-blobs dedup/resume. |
| 8 | OQ-8 | Session-daemon lifecycle ([[SPEC-003-realtime-collaborative-editing#ADR-014]]): auto-start on `share`/first attach vs explicit `ar-edit daemon`; idle-shutdown policy; one-daemon-per-project vs global; socket path/permission hardening; Windows named-pipe equivalent ([[SPEC-001-transcript-video-editor#NFR-015]]); how the daemon drives remote sync (push local IPC mutations as deltas, apply remote deltas). | **Largely resolved** (IMPL-003) — core daemon + IPC implemented (REQ-089/090, CON-018); the on-disk edit is the single CRDT-backed store (ADR-011) and one-shot CLI commands operate on it with durable cursor undo. The daemon now **owns** that canonical file: `ar-edit daemon --edit <name>` loads `edits/<name>.edit.json`, applies IPC mutations, and persists it back — one store, so a live session and the file never diverge. The IPC op-id/gating protocol (review #5–#8) is retired. While a host owns an edit, one-shot mutations on it are refused (fail-closed) pending attach-to-host routing. **Remaining:** one-shot commands *attach* to a live host and mutate through it (vs. refuse); lifecycle/policy (auto-start, idle-shutdown, one-vs-many, Windows named pipe); and wiring the daemon to remote sync (apply peer deltas into the owned store). |
| 2 | OQ-2 | Should lazy/on-demand media fetch be layered on full replication to let a peer start editing before a multi-GB sync finishes? | Deferred — optimisation over [[SPEC-003-realtime-collaborative-editing#ADR-010]] |
| 3 | OQ-3 | Is the [[Rendezvous Server]] self-hosted by the user, Anuna-operated, or pluggable via config? Affects trust model and NFR-010. | **Resolved (v1.1.0)** — no dedicated server: discovery is serverless via phrase-keyed [[pkarr]] / [[Mainline DHT]] ([[SPEC-003-realtime-collaborative-editing#ADR-013]]). A pkarr HTTP relay (e.g. `relay.pkarr.org`) is configurable; the TCP rendezvous relay remains an optional DHT-blocked fallback. |
| 4 | OQ-4 | Version-bump and `superseded` bookkeeping on [[SPEC-001-transcript-video-editor]] REQ-046–048 and [[ADR-001-event-sourced-edits]] once SPEC-003 is `implemented`. | Open — tracked, execute at Phase 3 close |
| 5 | OQ-5 | Should source-media replication be opt-in per source (e.g. exclude very large B-roll a peer will never need)? | Deferred — relates to OQ-2 |
| 6 | OQ-6 | Conflict UX when two peers present the same `src-id` with different content ([[SPEC-003-realtime-collaborative-editing#REQ-074]]): auto-rename vs prompt? | Open — needs a SCREEN/UX decision |

---

## Technology Decisions (Preliminary — formalised in the ADRs above)

| Concern | Decision | Rationale |
|---------|----------|-----------|
| CRDT engine | [[Loro]] (`MovableList` + `Map`) | Identity-preserving move; per-peer undo; delta sync |
| P2P transport | [[iroh]] ([[QUIC]]) | Authenticated, NAT-traversing p2p with relay fallback |
| Bulk transfer | [[iroh-blobs]] ([[BLAKE3]]) | Content-addressed, dedup, resumable, integrity-verified |
| Pairing | [[SPAKE2]] over direct [[iroh]] | Single-guess [[PAKE]] from a low-entropy phrase |
| Discovery | phrase-keyed [[pkarr]] on [[Mainline DHT]] ([[SPEC-003-realtime-collaborative-editing#ADR-013]]) | Serverless meeting point; pattern after `../did-crdt` ADR-006; iroh already uses pkarr |
| Wordlist | [[BIP39]] English (2048 words) | Standard, first-four-letters unique; matches `cbcl-bus` mnemonic pairing |
| Async runtime | `tokio` (already a workspace dep) | iroh, pkarr, and the optional relay are async |
| Session process | TUI / blocking `share`, or `ar-edit daemon` + Unix-socket IPC ([[SPEC-003-realtime-collaborative-editing#ADR-014]]) | One-shot CLI can't hold live state; the daemon owns the CRDT + connections + pkarr refresh |
| Platform support | macOS · Linux · Windows 10+ ([[SPEC-001-transcript-video-editor#NFR-015]]) | All collaboration crates (loro, iroh, iroh-blobs, spake2, blake3) are cross-platform Rust; iroh officially supports Windows; source-linking variance per [[ADR-012-cross-platform-source-linking]]; empirical Windows run is [[SPEC-003-realtime-collaborative-editing#OQ-7]] |
