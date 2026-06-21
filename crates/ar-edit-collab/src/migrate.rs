//! Migrate an event-sourced [`EditDocument`] into the canonical CRDT form
//! (SPEC-003 REQ-088; ADR-011 supersedes ADR-001's single-writer model).
//!
//! The cached `snapshot` is the materialised state of `ops[0..=head]`, so
//! replaying the snapshot's shots into a fresh one-actor [`CollabDoc`]
//! reproduces it exactly:
//! `materialise(from_event_sourced(d)) == d.snapshot` (TEST-109).
//!
//! Single-player editing is just the one-actor case (TEST-110): there is one
//! canonical representation, and the non-destructive guarantee of
//! [`ADR-001`] is preserved by the CRDT's own history.

use crate::crdt::CollabDoc;
use crate::ids::ActorId;
use ar_edit_core::models::EditDocument;

/// Fixed peer id used for the migration operations themselves, so that two
/// peers migrating the same legacy edit independently produce identical Loro
/// ops (idempotent on merge — no duplicated shots).
pub const MIGRATION_ACTOR: ActorId = ActorId(1);

/// Build a CRDT document from an existing event-sourced edit document, owned by
/// `actor` for subsequent edits.
///
/// The migration ops are written under [`MIGRATION_ACTOR`] (deterministic, so
/// independent migrations merge idempotently); the document is then re-keyed to
/// the caller's node-derived `actor` so later collaborative edits carry a unique
/// peer id rather than colliding on actor 1 (REQ-072, REQ-088).
pub fn from_event_sourced(ed: &EditDocument, actor: ActorId) -> CollabDoc {
    let collab = CollabDoc::new(MIGRATION_ACTOR);
    for shot in &ed.snapshot.shots {
        collab.add_shot(shot);
    }
    let _ = collab.rekey_actor(actor);
    collab
}
