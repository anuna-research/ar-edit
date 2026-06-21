//! Per-source annotation store: markers + POIs as CRDTs (ADR-015 / REQ-082).
//!
//! Markers and points of interest are per-*source*, project-wide annotations —
//! independent of any one edit — so they live in their own CRDT store, separate
//! from the per-edit [`crate::store::PersistentEdit`]. Each source's markers and
//! POIs share one [`CollabDoc`] (observed-remove sets, REQ-082), persisted to
//! `annotations/<source>.annot.json`, and converge between peers exactly like an
//! edit ([[SPEC-003-realtime-collaborative-editing#REQ-083]]).

use crate::crdt::CollabDoc;
use crate::ids::ActorId;
use crate::migrate::MIGRATION_ACTOR;
use ar_edit_core::models::{Marker, Poi};
use base64::Engine;

#[derive(Debug, thiserror::Error)]
pub enum AnnotationError {
    #[error("corrupt annotation store: {0}")]
    Corrupt(String),
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// On-disk envelope: the canonical CRDT payload plus a derived markers/POIs
/// mirror so a plain reader still sees the annotations without a CRDT engine.
#[derive(serde::Serialize, serde::Deserialize)]
struct OnDisk {
    source_id: String,
    actor: u64,
    /// base64 Loro snapshot (canonical state).
    crdt: String,
    #[serde(default)]
    markers: Vec<Marker>,
    #[serde(default)]
    pois: Vec<Poi>,
}

/// The canonical, CRDT-backed annotation store for one source.
pub struct AnnotationStore {
    source_id: String,
    doc: CollabDoc,
}

impl AnnotationStore {
    /// A new, empty store owned by `actor`.
    pub fn create(source_id: impl Into<String>, actor: ActorId) -> Self {
        Self { source_id: source_id.into(), doc: CollabDoc::new(actor) }
    }

    /// Seed a store from legacy plain-JSON annotations. The seeding ops are
    /// written under the fixed [`MIGRATION_ACTOR`] so two peers migrating the
    /// same legacy files independently produce identical ops (idempotent on
    /// merge — REQ-088), then the doc is re-keyed to the caller's `actor`.
    pub fn migrate(
        source_id: impl Into<String>,
        markers: &[Marker],
        pois: &[Poi],
        actor: ActorId,
    ) -> Self {
        let doc = CollabDoc::new(MIGRATION_ACTOR);
        for m in markers {
            doc.put_marker(&m.id, &serde_json::to_string(m).expect("marker json"));
        }
        for p in pois {
            doc.put_poi(&p.id, &serde_json::to_string(p).expect("poi json"));
        }
        let _ = doc.rekey_actor(actor);
        Self { source_id: source_id.into(), doc }
    }

    /// Parse a canonical CRDT annotation store.
    pub fn from_bytes(bytes: &[u8], _actor: ActorId) -> Result<Self, AnnotationError> {
        let on_disk: OnDisk = serde_json::from_slice(bytes)
            .map_err(|e| AnnotationError::Corrupt(format!("not an annotation store: {e}")))?;
        let doc = CollabDoc::new(ActorId(on_disk.actor));
        let crdt = b64()
            .decode(on_disk.crdt.as_bytes())
            .map_err(|e| AnnotationError::Corrupt(format!("crdt base64: {e}")))?;
        doc.import(&crdt)
            .map_err(|e| AnnotationError::Corrupt(format!("crdt import: {e}")))?;
        Ok(Self { source_id: on_disk.source_id, doc })
    }

    /// Serialise the canonical store.
    pub fn to_bytes(&self) -> Vec<u8> {
        let on_disk = OnDisk {
            source_id: self.source_id.clone(),
            actor: self.doc.actor().0,
            crdt: b64().encode(self.doc.export_snapshot()),
            markers: self.markers(),
            pois: self.pois(),
        };
        serde_json::to_vec_pretty(&on_disk).expect("annotation store serialises")
    }

    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    /// Add or replace a marker (REQ-082 observed-replace by id).
    pub fn add_marker(&self, marker: &Marker) {
        self.doc.put_marker(&marker.id, &serde_json::to_string(marker).expect("marker json"));
    }

    /// Build and add a marker (timestamped now, attributed to `author`); returns
    /// it. Keeps `chrono` out of one-shot CLI callers.
    pub fn add_marker_fields(
        &self,
        id: &str,
        range: ar_edit_core::models::ShotRange,
        label: &str,
        note: Option<String>,
        author: &str,
    ) -> Marker {
        let marker = Marker {
            id: id.to_string(),
            range,
            label: label.to_string(),
            note,
            author: author.to_string(),
            created: chrono::Utc::now(),
        };
        self.add_marker(&marker);
        marker
    }

    /// Remove a marker by id (REQ-082 observed-remove).
    pub fn remove_marker(&self, id: &str) {
        self.doc.remove_marker(id);
    }

    /// Live markers, ordered by id.
    pub fn markers(&self) -> Vec<Marker> {
        self.doc
            .marker_values()
            .iter()
            .filter_map(|j| serde_json::from_str(j).ok())
            .collect()
    }

    /// Add or replace a POI (REQ-082 observed-replace by id).
    pub fn add_poi(&self, poi: &Poi) {
        self.doc.put_poi(&poi.id, &serde_json::to_string(poi).expect("poi json"));
    }

    /// Build and add a POI (timestamped now); returns it. Keeps `chrono` out of
    /// one-shot CLI callers.
    pub fn add_poi_fields(
        &self,
        id: &str,
        point: ar_edit_core::models::PoiPoint,
        category: ar_edit_core::models::PoiCategory,
        note: Option<String>,
        author: &str,
    ) -> Poi {
        let poi = Poi {
            id: id.to_string(),
            point,
            category,
            note,
            author: author.to_string(),
            created: chrono::Utc::now(),
        };
        self.add_poi(&poi);
        poi
    }

    /// Remove a POI by id (REQ-082 observed-remove).
    pub fn remove_poi(&self, id: &str) {
        self.doc.remove_poi(id);
    }

    /// Live POIs, ordered by id.
    pub fn pois(&self) -> Vec<Poi> {
        self.doc
            .poi_values()
            .iter()
            .filter_map(|j| serde_json::from_str(j).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ar_edit_core::models::{PoiCategory, PoiPoint, ShotRange};
    use chrono::{TimeZone, Utc};

    fn ts() -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(0, 0).single().unwrap()
    }

    fn marker(id: &str, label: &str) -> Marker {
        Marker {
            id: id.into(),
            range: ShotRange::Words { from: 0, to: 10 },
            label: label.into(),
            note: None,
            author: "tester".into(),
            created: ts(),
        }
    }

    fn poi(id: &str, cat: PoiCategory) -> Poi {
        Poi {
            id: id.into(),
            point: PoiPoint::Word(5),
            category: cat,
            note: None,
            author: "tester".into(),
            created: ts(),
        }
    }

    #[test]
    fn add_list_remove_markers_and_pois() {
        let store = AnnotationStore::create("src-001", ActorId(1));
        store.add_marker(&marker("mark-001", "select"));
        store.add_marker(&marker("mark-002", "hero"));
        store.add_poi(&poi("poi-001", PoiCategory::Highlight));

        assert_eq!(store.markers().len(), 2);
        assert_eq!(store.markers()[0].label, "select");
        assert_eq!(store.pois().len(), 1);
        assert_eq!(store.pois()[0].category, PoiCategory::Highlight);

        store.remove_marker("mark-001");
        assert_eq!(store.markers().len(), 1);
        assert_eq!(store.markers()[0].id, "mark-002");
    }

    #[test]
    fn roundtrips_through_persistence() {
        let store = AnnotationStore::create("src-001", ActorId(7));
        store.add_marker(&marker("mark-001", "select"));
        store.add_poi(&poi("poi-001", PoiCategory::Issue));

        let reloaded = AnnotationStore::from_bytes(&store.to_bytes(), ActorId(7)).unwrap();
        assert_eq!(reloaded.source_id(), "src-001");
        assert_eq!(reloaded.markers(), store.markers());
        assert_eq!(reloaded.pois(), store.pois());
    }

    #[test]
    fn migration_is_idempotent_across_peers() {
        // Two peers migrating the same legacy annotations independently converge.
        let markers = [marker("mark-001", "select"), marker("mark-002", "hero")];
        let pois = [poi("poi-001", PoiCategory::Cue)];

        let a = AnnotationStore::migrate("src-001", &markers, &pois, ActorId(10));
        let b = AnnotationStore::migrate("src-001", &markers, &pois, ActorId(20));
        a.doc.import(&b.doc.export_snapshot()).unwrap();

        assert_eq!(a.markers().len(), 2, "no duplicated markers after merge");
        assert_eq!(a.pois().len(), 1, "no duplicated POIs after merge");
    }

    #[test]
    fn concurrent_marker_edits_converge() {
        let a = AnnotationStore::create("src-001", ActorId(1));
        let b = AnnotationStore::create("src-001", ActorId(2));
        a.add_marker(&marker("mark-001", "a"));
        b.add_marker(&marker("mark-002", "b"));
        a.doc.import(&b.doc.export_snapshot()).unwrap();
        b.doc.import(&a.doc.export_snapshot()).unwrap();

        assert_eq!(a.markers(), b.markers(), "annotation stores converge");
        assert_eq!(a.markers().len(), 2);
    }
}
