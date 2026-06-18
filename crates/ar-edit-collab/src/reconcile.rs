//! Source-inventory reconciliation and blob integrity (SPEC-003 REQ-074,
//! REQ-076 — pure parts; the receive/quarantine I/O lives in the shell).

use std::collections::BTreeMap;

/// One source's entry in a peer's manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    pub src_id: String,
    pub hash: [u8; 32],
    pub duration_ms: u64,
}

/// A source id presented with two different content hashes across peers
/// (REQ-074: surfaced, never silently overwritten).
#[derive(Debug, PartialEq, Eq)]
pub struct Conflict {
    pub src_id: String,
    pub hash_a: [u8; 32],
    pub hash_b: [u8; 32],
}

/// The reconciled inventory: each source id maps to exactly one entry.
#[derive(Debug, PartialEq, Eq)]
pub struct MergedManifest {
    pub entries: BTreeMap<String, ManifestEntry>,
}

/// Reconcile two peers' manifests. Identical entries merge; disjoint entries
/// union; the same `src_id` with a different `hash` yields a [`Conflict`]
/// (REQ-074). Deterministic regardless of argument order for the success case.
pub fn reconcile(
    local: &[ManifestEntry],
    remote: &[ManifestEntry],
) -> Result<MergedManifest, Vec<Conflict>> {
    let mut entries: BTreeMap<String, ManifestEntry> = BTreeMap::new();
    let mut conflicts: Vec<Conflict> = Vec::new();

    for e in local.iter().chain(remote.iter()) {
        match entries.get(&e.src_id) {
            None => {
                entries.insert(e.src_id.clone(), e.clone());
            }
            Some(existing) if existing.hash != e.hash => {
                // Record once per offending id.
                if !conflicts.iter().any(|c| c.src_id == e.src_id) {
                    conflicts.push(Conflict {
                        src_id: e.src_id.clone(),
                        hash_a: existing.hash,
                        hash_b: e.hash,
                    });
                }
            }
            Some(_) => {} // identical — already merged
        }
    }

    if conflicts.is_empty() {
        Ok(MergedManifest { entries })
    } else {
        conflicts.sort_by(|a, b| a.src_id.cmp(&b.src_id));
        Err(conflicts)
    }
}

/// Verify a received blob against its advertised BLAKE3 content address
/// (REQ-076). The shell calls this *before* writing the blob to `sources/`;
/// a `false` result means quarantine + structured error, never apply.
pub fn verify_blob(expected: &[u8; 32], bytes: &[u8]) -> bool {
    blake3::hash(bytes).as_bytes() == expected
}

/// Compute the BLAKE3 content address of a blob.
pub fn content_hash(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}
