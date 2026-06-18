//! Manifest reconciliation + blob integrity (SPEC-003 REQ-074/076,
//! TEST-085/086/088/089).

use ar_edit_collab::reconcile::{self, Conflict, ManifestEntry};

fn entry(id: &str, h: u8) -> ManifestEntry {
    ManifestEntry {
        src_id: id.into(),
        hash: [h; 32],
        duration_ms: 1000,
    }
}

#[test]
fn reconcile_merges_disjoint_and_identical() {
    // TEST-085: overlapping-but-different inventories merge to one entry per id.
    let local = vec![entry("src-001", 1), entry("src-002", 2)];
    let remote = vec![entry("src-002", 2), entry("src-003", 3)];
    let merged = reconcile::reconcile(&local, &remote).expect("no conflict");
    assert_eq!(merged.entries.len(), 3);
    assert_eq!(merged.entries["src-001"].hash, [1; 32]);
    assert_eq!(merged.entries["src-003"].hash, [3; 32]);
}

#[test]
fn reconcile_detects_conflict() {
    // TEST-086: same id, different hash -> Conflict, no silent overwrite.
    let local = vec![entry("src-001", 1)];
    let remote = vec![entry("src-001", 9)];
    let err = reconcile::reconcile(&local, &remote).unwrap_err();
    assert_eq!(
        err,
        vec![Conflict {
            src_id: "src-001".into(),
            hash_a: [1; 32],
            hash_b: [9; 32],
        }]
    );
}

#[test]
fn blob_integrity() {
    // TEST-088/089: matching hash accepted, mismatched rejected.
    let bytes = b"some source media bytes";
    let h = reconcile::content_hash(bytes);
    assert!(reconcile::verify_blob(&h, bytes));
    assert!(!reconcile::verify_blob(&[0u8; 32], bytes));
    assert!(!reconcile::verify_blob(&h, b"tampered"));
}
