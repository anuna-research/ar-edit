//! Pure-core convergence / migration tests (SPEC-003).
//! Grows into the full p8 suite. No network, no disk-transfer, deterministic.

use ar_edit_collab::crdt::CollabDoc;
use ar_edit_collab::ids::ActorId;
use ar_edit_collab::{materialise, migrate};
use ar_edit_core::models::{EditDocument, EditSnapshot, Shot, ShotNote, ShotRange};
use chrono::{TimeZone, Utc};

fn ts(secs: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap()
}

fn shot(id: &str, source: &str, range: ShotRange, notes: Vec<ShotNote>) -> Shot {
    Shot {
        id: id.into(),
        source: source.into(),
        range,
        notes,
    }
}

fn words(from: u32, to: u32) -> ShotRange {
    ShotRange::Words { from, to }
}

fn order_ids(snap: &EditSnapshot) -> Vec<String> {
    snap.shots.iter().map(|s| s.id.clone()).collect()
}

fn json(snap: &EditSnapshot) -> serde_json::Value {
    serde_json::to_value(snap).unwrap()
}

/// TEST-109: migrating an event-sourced doc yields a materialised shot list
/// identical to its cached snapshot.
#[test]
fn migration_snapshot_identity() {
    let snapshot = EditSnapshot {
        shots: vec![
            shot(
                "shot-001",
                "src-001",
                words(0, 52),
                vec![
                    ShotNote {
                        text: "too long".into(),
                        created: ts(100),
                    },
                    ShotNote {
                        text: "great energy".into(),
                        created: ts(200),
                    },
                ],
            ),
            shot(
                "shot-002",
                "src-002",
                ShotRange::Time {
                    from_ms: 0,
                    to_ms: 1000,
                },
                vec![],
            ),
            shot("shot-003", "src-002", ShotRange::Scenes { from: 0, to: 2 }, vec![]),
        ],
    };
    let ed = EditDocument {
        name: "rough".into(),
        created: ts(0),
        next_shot_id: 4,
        head: -1,
        ops: vec![],
        snapshot: snapshot.clone(),
    };

    let collab = migrate::from_event_sourced(&ed, ActorId(2));
    let materialised = materialise::materialise(&collab);

    assert_eq!(
        json(&materialised),
        json(&snapshot),
        "materialise(migrate(d)) must equal d.snapshot byte-for-byte"
    );
}

/// TEST-096: two peers concurrently MOVE the same shot to different positions.
/// They must converge to one order with the shot present exactly once.
#[test]
fn concurrent_move_same_shot_converges() {
    let base = base_three_shots();

    let a = CollabDoc::new(ActorId(10));
    a.import(&base).unwrap();
    let b = CollabDoc::new(ActorId(20));
    b.import(&base).unwrap();

    a.move_shot("shot-003", 0); // A: s3 to front
    b.move_shot("shot-003", 1); // B: s3 to middle

    let a_up = a.export_snapshot();
    let b_up = b.export_snapshot();
    a.import(&b_up).unwrap();
    b.import(&a_up).unwrap();

    let oa = order_ids(&materialise::materialise(&a));
    let ob = order_ids(&materialise::materialise(&b));
    assert_eq!(oa, ob, "replicas diverged on concurrent move");
    assert_eq!(oa.iter().filter(|x| *x == "shot-003").count(), 1, "s3 dup/lost");
    assert_eq!(oa.len(), 3, "shot count changed: {oa:?}");
}

/// TEST-095: a MOVE concurrent with a FIELD edit on the same shot — both must
/// survive on the same shot identity.
#[test]
fn move_concurrent_with_trim() {
    let base = base_three_shots();

    let a = CollabDoc::new(ActorId(11));
    a.import(&base).unwrap();
    let b = CollabDoc::new(ActorId(21));
    b.import(&base).unwrap();

    a.move_shot("shot-002", 0); // A moves s2 to front
    b.trim_shot("shot-002", &words(210, 265)); // B trims s2

    let a_up = a.export_snapshot();
    let b_up = b.export_snapshot();
    b.import(&a_up).unwrap();
    a.import(&b_up).unwrap();

    let ma = materialise::materialise(&a);
    let mb = materialise::materialise(&b);
    assert_eq!(json(&ma), json(&mb), "replicas diverged");
    assert_eq!(ma.shots[0].id, "shot-002", "move lost");
    let s2 = ma.shots.iter().find(|s| s.id == "shot-002").unwrap();
    assert_eq!(s2.range, words(210, 265), "field edit lost");
}

/// TEST-101 (SEC): three concurrent changes delivered to two observers in
/// different orders converge to identical state.
#[test]
fn strong_eventual_consistency() {
    let base = base_three_shots();
    let p1 = CollabDoc::new(ActorId(31));
    p1.import(&base).unwrap();
    let p2 = CollabDoc::new(ActorId(32));
    p2.import(&base).unwrap();
    let p3 = CollabDoc::new(ActorId(33));
    p3.import(&base).unwrap();

    p1.add_shot(&shot("shot-004", "src-001", words(5, 9), vec![]));
    p2.move_shot("shot-001", 2);
    p3.trim_shot("shot-003", &ShotRange::Scenes { from: 5, to: 9 });

    let (u1, u2, u3) = (p1.export_snapshot(), p2.export_snapshot(), p3.export_snapshot());

    let x = CollabDoc::new(ActorId(40));
    x.import(&base).unwrap();
    let y = CollabDoc::new(ActorId(41));
    y.import(&base).unwrap();
    for u in [&u1, &u2, &u3] {
        x.import(u).unwrap();
    }
    for u in [&u3, &u1, &u2] {
        y.import(u).unwrap();
    }

    assert_eq!(
        json(&materialise::materialise(&x)),
        json(&materialise::materialise(&y)),
        "SEC violated: delivery order changed final state"
    );
}

/// TEST-094: concurrent inserts by two peers both survive, no dup/loss.
#[test]
fn concurrent_inserts_converge() {
    let base = base_three_shots();
    let a = CollabDoc::new(ActorId(10));
    a.import(&base).unwrap();
    let b = CollabDoc::new(ActorId(20));
    b.import(&base).unwrap();

    a.add_shot(&shot("shot-010", "src-001", words(1, 2), vec![]));
    b.add_shot(&shot("shot-020", "src-002", words(3, 4), vec![]));

    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    let oa = order_ids(&materialise::materialise(&a));
    let ob = order_ids(&materialise::materialise(&b));
    assert_eq!(oa, ob, "diverged on concurrent insert");
    assert_eq!(oa.len(), 5, "both inserts must survive: {oa:?}");
    assert!(oa.contains(&"shot-010".to_string()) && oa.contains(&"shot-020".to_string()));
}

/// TEST-099: markers/POIs are an observed-remove set — concurrent adds all
/// survive; a remove takes effect on merge.
#[test]
fn marker_orset_converges() {
    let a = CollabDoc::new(ActorId(10));
    let b = CollabDoc::new(ActorId(20));
    a.put_marker("mark-001", "{\"label\":\"select\"}");
    b.import(&a.export_snapshot()).unwrap();

    a.put_marker("mark-002", "{\"label\":\"hero\"}");
    b.put_marker("mark-003", "{\"label\":\"avoid\"}");
    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    assert_eq!(a.marker_ids(), b.marker_ids(), "marker sets diverged");
    assert_eq!(a.marker_ids().len(), 3, "all concurrent adds survive");

    a.remove_marker("mark-002");
    b.import(&a.export_snapshot()).unwrap();
    assert!(!b.marker_ids().contains(&"mark-002".to_string()), "remove propagated");
}

/// TEST-100: concurrent note appends to the SAME shot are never lost.
#[test]
fn notes_never_lost_on_merge() {
    let base = base_three_shots();
    let a = CollabDoc::new(ActorId(10));
    a.import(&base).unwrap();
    let b = CollabDoc::new(ActorId(20));
    b.import(&base).unwrap();

    a.add_note("shot-001", &note("from A", 100));
    b.add_note("shot-001", &note("from B", 100));

    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    let ma = materialise::materialise(&a);
    let mb = materialise::materialise(&b);
    assert_eq!(json(&ma), json(&mb), "diverged on concurrent notes");
    let s1 = ma.shots.iter().find(|s| s.id == "shot-001").unwrap();
    let texts: Vec<&str> = s1.notes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        texts.contains(&"from A") && texts.contains(&"from B"),
        "both notes must survive: {texts:?}"
    );
}

/// TEST-107/108 (REQ-087): peers edit while partitioned, then on reconnect
/// exchange only the changes each side lacks (delta sync via version vectors)
/// and converge with no loss — no full-document resend.
#[test]
fn offline_edits_reconcile_with_delta_sync() {
    let base = base_three_shots();
    let a = CollabDoc::new(ActorId(10));
    a.import(&base).unwrap();
    let b = CollabDoc::new(ActorId(20));
    b.import(&base).unwrap();

    // Partition: each peer edits offline.
    a.add_shot(&shot("shot-A1", "src-001", words(1, 2), vec![]));
    a.move_shot("shot-001", 2);
    b.add_shot(&shot("shot-B1", "src-002", words(3, 4), vec![]));
    b.trim_shot("shot-002", &words(9, 9));

    // Reconnect: capture versions, exchange ONLY missing deltas.
    let (va, vb) = (a.version(), b.version());
    let delta_for_b = a.export_from(&vb); // what B lacks
    let delta_for_a = b.export_from(&va); // what A lacks

    // Delta-only: smaller than a full snapshot resend (TEST-108).
    assert!(
        delta_for_b.len() < a.export_snapshot().len(),
        "reconnect must send a delta, not the whole doc"
    );

    b.import(&delta_for_b).unwrap();
    a.import(&delta_for_a).unwrap();

    assert_eq!(
        json(&materialise::materialise(&a)),
        json(&materialise::materialise(&b)),
        "peers must converge after delta sync"
    );
    let ids = order_ids(&materialise::materialise(&a));
    assert!(
        ids.contains(&"shot-A1".to_string()) && ids.contains(&"shot-B1".to_string()),
        "no offline edit may be lost: {ids:?}"
    );
}

/// Regression (P1): generated shot ids are actor-scoped, so concurrent inserts
/// on different replicas never collide — both survive with their own fields.
#[test]
fn generated_shot_ids_avoid_collision_across_replicas() {
    let base = base_three_shots();
    let a = CollabDoc::new(ActorId(10));
    a.import(&base).unwrap();
    let b = CollabDoc::new(ActorId(20));
    b.import(&base).unwrap();

    let id_a = a.add_new_shot("src-A", &words(1, 2));
    let id_b = b.add_new_shot("src-B", &words(3, 4));
    assert_ne!(id_a, id_b, "generated ids must differ across actors");

    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    let ma = materialise::materialise(&a);
    assert_eq!(ma.shots.len(), 5, "both inserts survive: {:?}", order_ids(&ma));
    assert_eq!(ma.shots.iter().find(|s| s.id == id_a).unwrap().source, "src-A");
    assert_eq!(ma.shots.iter().find(|s| s.id == id_b).unwrap().source, "src-B");
    assert_eq!(json(&ma), json(&materialise::materialise(&b)));
}

/// Regression (P1): a locally-duplicated shot id is disambiguated rather than
/// overwriting the first shot's fields.
#[test]
fn add_shot_disambiguates_local_duplicate_id() {
    let d = CollabDoc::new(ActorId(1));
    d.add_shot(&shot("shot-001", "src-A", words(0, 1), vec![]));
    d.add_shot(&shot("shot-001", "src-B", words(2, 3), vec![])); // same id locally
    let m = materialise::materialise(&d);
    assert_eq!(m.shots.len(), 2, "duplicate id must not overwrite: {:?}", order_ids(&m));
    let srcs: Vec<&str> = m.shots.iter().map(|s| s.source.as_str()).collect();
    assert!(
        srcs.contains(&"src-A") && srcs.contains(&"src-B"),
        "both sources preserved: {srcs:?}"
    );
}

/// Regression (P1/P2): two peers migrate the SAME legacy edit independently,
/// then edit and sync — migrated shots are not duplicated (idempotent migration
/// under the fixed migration actor) and each peer's live inserts survive (their
/// re-keyed node actors keep ids unique).
#[test]
fn independent_migrations_converge_without_duplication() {
    let snapshot = EditSnapshot {
        shots: vec![
            shot("shot-001", "src-001", words(0, 52), vec![]),
            shot("shot-002", "src-002", words(0, 10), vec![]),
        ],
    };
    let ed = EditDocument {
        name: "legacy".into(),
        created: ts(0),
        next_shot_id: 3,
        head: -1,
        ops: vec![],
        snapshot,
    };

    let a = migrate::from_event_sourced(&ed, ActorId(10));
    let b = migrate::from_event_sourced(&ed, ActorId(20));
    a.add_new_shot("src-A", &words(1, 2));
    b.add_new_shot("src-B", &words(3, 4));

    a.import(&b.export_snapshot()).unwrap();
    b.import(&a.export_snapshot()).unwrap();

    let ma = materialise::materialise(&a);
    let order = order_ids(&ma);
    assert_eq!(order.iter().filter(|x| *x == "shot-001").count(), 1, "no dup: {order:?}");
    assert_eq!(order.iter().filter(|x| *x == "shot-002").count(), 1, "no dup: {order:?}");
    assert_eq!(ma.shots.len(), 4, "2 migrated + 2 live inserts: {order:?}");
    assert_eq!(json(&ma), json(&materialise::materialise(&b)));
}

/// Regression (P1): notes must survive a reload by the same actor — the note
/// counter is restored from imported ids so add_note doesn't reuse `actor:0`.
#[test]
fn notes_survive_reload_same_actor() {
    let a = CollabDoc::new(ActorId(7));
    a.add_shot(&shot("shot-001", "src-001", words(0, 1), vec![]));
    a.add_note("shot-001", &note("first", 100));
    let snap = a.export_snapshot();

    // Fresh process, SAME actor id, import the persisted snapshot.
    let b = CollabDoc::new(ActorId(7));
    b.import(&snap).unwrap();
    b.add_note("shot-001", &note("second", 200)); // must not overwrite "first"

    let m = materialise::materialise(&b);
    let s1 = m.shots.iter().find(|s| s.id == "shot-001").unwrap();
    let texts: Vec<&str> = s1.notes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        texts.contains(&"first") && texts.contains(&"second"),
        "both notes must survive reload with the same actor: {texts:?}"
    );
    assert_eq!(s1.notes.len(), 2, "no note lost on reload");
}

fn note(text: &str, secs: i64) -> ShotNote {
    ShotNote {
        text: text.into(),
        created: ts(secs),
    }
}

fn base_three_shots() -> Vec<u8> {
    let d = CollabDoc::new(ActorId(1));
    d.add_shot(&shot("shot-001", "src-001", words(0, 52), vec![]));
    d.add_shot(&shot("shot-002", "src-003", words(200, 280), vec![]));
    d.add_shot(&shot("shot-003", "src-002", ShotRange::Scenes { from: 0, to: 2 }, vec![]));
    d.export_snapshot()
}
