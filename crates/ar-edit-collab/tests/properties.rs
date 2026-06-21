//! Property-based tests (SPEC-003) — the invariants the spec calls the
//! "property-tested heart" of REQ-080/083/088, exercised over *generated*
//! operation sequences rather than hand-picked examples (PROTO-001 mandates
//! property + mutation testing for AI-synthesised code).
//!
//! Validates:
//!   - [[SPEC-003-realtime-collaborative-editing#REQ-083]] convergence (SEC)
//!   - [[SPEC-003-realtime-collaborative-editing#REQ-088]] persistence roundtrip
//!   - [[SPEC-003-realtime-collaborative-editing#REQ-080]] identity / no id reuse

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::materialise::materialise;
use ar_edit_collab::store::PersistentEdit;
use ar_edit_core::models::ShotRange;
use proptest::prelude::*;
use std::collections::HashSet;

/// A generated edit operation, interpreted against the current shot list (the
/// index ops are taken modulo the live shot count so they always target a real
/// shot when one exists).
#[derive(Clone, Debug)]
enum Op {
    Add { src: u8, from: u16, len: u16 },
    Remove(usize),
    Move { from: usize, to: usize },
    Trim { nth: usize, from: u16, len: u16 },
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0u8..6, 0u16..200, 1u16..40).prop_map(|(src, from, len)| Op::Add { src, from, len }),
        2 => (0usize..16).prop_map(Op::Remove),
        2 => (0usize..16, 0usize..16).prop_map(|(from, to)| Op::Move { from, to }),
        2 => (0usize..16, 0u16..200, 1u16..40).prop_map(|(nth, from, len)| Op::Trim { nth, from, len }),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn ops(max: usize) -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(op(), 0..max)
}

fn words(from: u16, len: u16) -> ShotRange {
    ShotRange::Words {
        from: from as u32,
        to: (from + len) as u32,
    }
}

fn ids(e: &PersistentEdit) -> Vec<String> {
    e.snapshot().shots.into_iter().map(|s| s.id).collect()
}

/// Apply an op to the canonical store; record a minted id if one was created.
fn apply(e: &mut PersistentEdit, op: &Op, minted: &mut Vec<String>) {
    let live = ids(e);
    match op {
        Op::Add { src, from, len } => {
            if let Ok(id) = e.add_shot(&format!("src-{src}"), words(*from, *len), None, "") {
                minted.push(id);
            }
        }
        Op::Remove(n) if !live.is_empty() => e.remove_shot(&live[n % live.len()], None),
        Op::Move { from, to } if !live.is_empty() => {
            e.move_shot(&live[from % live.len()], to % live.len(), None)
        }
        Op::Trim { nth, from, len } if !live.is_empty() => {
            let _ = e.trim_shot(&live[nth % live.len()], words(*from, *len), None);
        }
        Op::Undo => {
            e.undo();
        }
        Op::Redo => {
            e.redo();
        }
        _ => {}
    }
}

/// Apply an op to a bare CRDT doc (no undo cursor), tracking live ids locally.
fn apply_crdt(doc: &CollabDoc, op: &Op, live: &mut Vec<String>) {
    match op {
        Op::Add { src, from, len } => {
            live.push(doc.add_new_shot(&format!("src-{src}"), &words(*from, *len), ""))
        }
        Op::Remove(n) if !live.is_empty() => {
            let id = live.remove(*n % live.len());
            doc.remove_shot(&id);
        }
        Op::Move { from, to } if !live.is_empty() => {
            doc.move_shot(&live[from % live.len()], to % live.len())
        }
        Op::Trim { nth, from, len } if !live.is_empty() => {
            doc.trim_shot(&live[nth % live.len()], &words(*from, *len))
        }
        _ => {} // undo/redo are not CRDT-level operations
    }
}

proptest! {
    /// REQ-083 (Strong Eventual Consistency): two peers that apply DIFFERENT
    /// operation sequences and then exchange state converge to byte-identical
    /// materialised documents — regardless of what each did.
    #[test]
    fn convergence_is_byte_identical(a_ops in ops(24), b_ops in ops(24)) {
        let a = CollabDoc::new(ActorId(0xA));
        let b = CollabDoc::new(ActorId(0xB));
        let (mut la, mut lb) = (Vec::new(), Vec::new());
        for o in &a_ops { apply_crdt(&a, o, &mut la); }
        for o in &b_ops { apply_crdt(&b, o, &mut lb); }

        // Exchange state both ways (each export predates importing the other).
        let (ea, eb) = (a.export_snapshot(), b.export_snapshot());
        a.import(&eb).unwrap();
        b.import(&ea).unwrap();

        prop_assert_eq!(
            serde_json::to_value(materialise(&a)).unwrap(),
            serde_json::to_value(materialise(&b)).unwrap(),
        );
    }

    /// REQ-088: persisting and reloading the canonical store preserves the
    /// materialised state AND the undo cursor — for any operation sequence.
    #[test]
    fn persistence_roundtrips(seq in ops(40)) {
        let mut e = PersistentEdit::create("p", ActorId(7));
        let mut minted = Vec::new();
        for o in &seq { apply(&mut e, o, &mut minted); }

        let before = serde_json::to_value(e.snapshot()).unwrap();
        let (cu, cr) = (e.can_undo(), e.can_redo());

        let e2 = PersistentEdit::from_bytes(&e.to_bytes(), ActorId(7)).unwrap();
        prop_assert_eq!(serde_json::to_value(e2.snapshot()).unwrap(), before);
        prop_assert_eq!(e2.can_undo(), cu);
        prop_assert_eq!(e2.can_redo(), cr);
    }

    /// REQ-080: a minted shot id is never reused — not after removal, and not
    /// after an undo+reload cycle (the exact regression class from the merge
    /// review). Every id this store ever mints is unique over the whole run,
    /// including across a persistence boundary inserted mid-sequence.
    #[test]
    fn ids_are_never_reused(head in ops(20), tail in ops(20)) {
        let mut e = PersistentEdit::create("p", ActorId(9));
        let mut minted = Vec::new();
        for o in &head { apply(&mut e, o, &mut minted); }
        // Persist + reload mid-stream (a one-shot CLI "restart").
        let mut e = PersistentEdit::from_bytes(&e.to_bytes(), ActorId(9)).unwrap();
        for o in &tail { apply(&mut e, o, &mut minted); }

        let unique: HashSet<&String> = minted.iter().collect();
        prop_assert_eq!(unique.len(), minted.len(), "a tombstoned id was re-minted: {:?}", minted);
    }

    /// REQ-080: moving a shot is a permutation — it never duplicates or drops a
    /// shot. After any single move, the SET of live ids is unchanged.
    #[test]
    fn move_preserves_the_id_set(seq in ops(20), pick in 0usize..32, to in 0usize..32) {
        let mut e = PersistentEdit::create("p", ActorId(3));
        let mut minted = Vec::new();
        for o in &seq { apply(&mut e, o, &mut minted); }
        let before: HashSet<String> = ids(&e).into_iter().collect();
        if !before.is_empty() {
            let live = ids(&e);
            e.move_shot(&live[pick % live.len()], to % live.len(), None);
            let after: HashSet<String> = ids(&e).into_iter().collect();
            prop_assert_eq!(after, before, "move changed the id set");
        }
    }
}
