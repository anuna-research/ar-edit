//! `ar-edit-collab` — realtime collaborative editing for `ar-edit` (SPEC-003).
//!
//! # Dependency rule (Purity Boundary Map, SPEC-003)
//!
//! Every module in this crate **except [`shell`]** is *pure*: deterministic,
//! no I/O, no network, no `iroh`. The pure core is what makes convergence
//! property-testable with zero network or disk. `shell` holds the effectful
//! transport (iroh, rendezvous, blob sync) and depends inward on the core —
//! never the reverse. Pure modules MUST NOT `use` anything from [`shell`] or
//! any network/disk type.
//!
//! On this host the `shell` transport (iroh/quinn/rustls tree + a 2 GB media
//! transfer) exceeds available disk, so it is scaffolded behind trait
//! boundaries and tracked under SPEC-003 OQ-7; the pure core below is fully
//! implemented and tested.

// ---- pure core ----
pub mod crdt;
pub mod ids;
pub mod materialise;
pub mod migrate;
pub mod pairing;
pub mod presence;
pub mod recognise;
pub mod reconcile;
pub mod undo;

// ---- effectful shell (interfaces; impls gated on OQ-7) ----
pub mod shell;

pub use crdt::CollabDoc;
pub use ids::ActorId;
