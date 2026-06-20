use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Project Manifest (manifest.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    pub version: String,
    pub name: String,
    pub created: DateTime<Utc>,
    pub sources: Vec<Source>,
    pub next_source_id: u32,
    pub defaults: Defaults,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub id: String,
    pub path: PathBuf,
    pub original_filename: String,
    pub duration_ms: u64,
    pub video_codec: String,
    pub audio_codec: String,
    pub resolution: (u32, u32),
    pub frame_rate: f64,
    pub audio_channels: u8,
    pub audio_sample_rate: u32,
    pub added: DateTime<Utc>,
    pub transcribed: bool,
    pub indexed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Defaults {
    pub whisper_model: String,
    pub thumbnail_interval_sec: u32,
    pub render_codec: String,
    pub render_container: String,
}

// ---------------------------------------------------------------------------
// Transcript (src-NNN.transcript.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transcript {
    pub source_id: String,
    pub model: String,
    pub language: String,
    pub duration_ms: u64,
    pub segments: Vec<TranscriptSegment>,
    pub word_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptSegment {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Word {
    pub index: u32,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// Shot Range — tagged union per ADR-002
// ---------------------------------------------------------------------------

/// A shot's range: exactly one of words, scenes, or time.
///
/// Serialises with serde's default externally-tagged format:
/// ```json
/// { "words":  { "from": 0, "to": 52 } }
/// { "scenes": { "from": 0, "to": 2 } }
/// { "time":   { "from_ms": 15000, "to_ms": 22000 } }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShotRange {
    Words { from: u32, to: u32 },
    Scenes { from: u32, to: u32 },
    Time { from_ms: u64, to_ms: u64 },
}

// ---------------------------------------------------------------------------
// Edit Document (*.edit.json) — event-sourced per ADR-001
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditDocument {
    pub name: String,
    pub created: DateTime<Utc>,
    pub next_shot_id: u32,
    pub head: i32,
    pub ops: Vec<EditOp>,
    pub snapshot: EditSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditSnapshot {
    pub shots: Vec<Shot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Shot {
    pub id: String,
    pub source: String,
    pub range: ShotRange,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ShotNote>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShotNote {
    pub text: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Edit Operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditOp {
    pub id: u32,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub op: EditOpKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOpKind {
    AddShot {
        shot: Shot,
    },
    RemoveShot {
        shot_id: String,
        shot: Shot,
    },
    MoveShot {
        shot_id: String,
        from_position: u32,
        to_position: u32,
    },
    TrimShot {
        shot_id: String,
        old_range: ShotRange,
        new_range: ShotRange,
    },
    ReplaceRangeType {
        shot_id: String,
        old_range: ShotRange,
        new_range: ShotRange,
    },
    AddNote {
        shot_id: String,
        note: ShotNote,
    },
}

// ---------------------------------------------------------------------------
// Source Index (src-NNN.index.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceIndex {
    pub source_id: String,
    pub indexed_at: DateTime<Utc>,
    pub metadata: SourceMetadata,
    pub thumbnails: Vec<Thumbnail>,
    pub scene_count: u32,
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceMetadata {
    pub duration_ms: u64,
    pub resolution: (u32, u32),
    pub codec: String,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Thumbnail {
    pub path: PathBuf,
    pub timestamp_ms: u64,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scene {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub thumbnail: PathBuf,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Source Markers (annotations/src-NNN.markers.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceMarkers {
    pub source_id: String,
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Marker {
    pub id: String,
    pub range: ShotRange,
    pub label: String,
    pub note: Option<String>,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Points of Interest (annotations/src-NNN.pois.json) — SPEC-002, ADR-005
// ---------------------------------------------------------------------------

/// A single point in a source, mirroring ShotRange but for instants not ranges.
///
/// Serialises with serde's default externally-tagged format:
/// ```json
/// { "word": 45 }
/// { "scene": 3 }
/// { "time_ms": 62500 }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PoiPoint {
    Word(u32),
    Scene(u32),
    TimeMs(u64),
}

/// Controlled vocabulary for POI categories (REQ-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PoiCategory {
    Highlight,
    Issue,
    Transition,
    Cue,
    Note,
}

impl std::fmt::Display for PoiCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Highlight => write!(f, "highlight"),
            Self::Issue => write!(f, "issue"),
            Self::Transition => write!(f, "transition"),
            Self::Cue => write!(f, "cue"),
            Self::Note => write!(f, "note"),
        }
    }
}

impl std::str::FromStr for PoiCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "highlight" => Ok(Self::Highlight),
            "issue" => Ok(Self::Issue),
            "transition" => Ok(Self::Transition),
            "cue" => Ok(Self::Cue),
            "note" => Ok(Self::Note),
            other => Err(format!(
                "unknown POI category '{other}'; valid categories: highlight, issue, transition, cue, note"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Poi {
    pub id: String,
    pub point: PoiPoint,
    pub category: PoiCategory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourcePois {
    pub source_id: String,
    pub pois: Vec<Poi>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- ShotRange -----------------------------------------------------------

    #[test]
    fn shot_range_words_roundtrip() {
        let range = ShotRange::Words { from: 0, to: 52 };
        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(json, json!({ "words": { "from": 0, "to": 52 } }));

        let back: ShotRange = serde_json::from_value(json).unwrap();
        assert_eq!(back, range);
    }

    #[test]
    fn shot_range_scenes_roundtrip() {
        let range = ShotRange::Scenes { from: 0, to: 2 };
        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(json, json!({ "scenes": { "from": 0, "to": 2 } }));

        let back: ShotRange = serde_json::from_value(json).unwrap();
        assert_eq!(back, range);
    }

    #[test]
    fn shot_range_time_roundtrip() {
        let range = ShotRange::Time {
            from_ms: 15000,
            to_ms: 22000,
        };
        let json = serde_json::to_value(&range).unwrap();
        assert_eq!(
            json,
            json!({ "time": { "from_ms": 15000, "to_ms": 22000 } })
        );

        let back: ShotRange = serde_json::from_value(json).unwrap();
        assert_eq!(back, range);
    }

    // -- EditOp (flatten + internally-tagged EditOpKind) ---------------------

    #[test]
    fn edit_op_add_shot_roundtrip() {
        let op = EditOp {
            id: 0,
            ts: "2026-02-19T13:00:01Z".parse().unwrap(),
            op: EditOpKind::AddShot {
                shot: Shot {
                    id: "shot-001".into(),
                    source: "src-001".into(),
                    range: ShotRange::Words { from: 0, to: 52 },
                    notes: vec![],
                },
            },
        };

        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], "add_shot");
        assert_eq!(json["id"], 0);
        assert_eq!(json["shot"]["id"], "shot-001");
        assert_eq!(
            json["shot"]["range"],
            json!({ "words": { "from": 0, "to": 52 } })
        );
        // notes omitted when empty
        assert!(json["shot"].get("notes").is_none());

        let back: EditOp = serde_json::from_value(json).unwrap();
        assert_eq!(back, op);
    }

    #[test]
    fn edit_op_move_shot_roundtrip() {
        let op = EditOp {
            id: 3,
            ts: "2026-02-19T13:02:30Z".parse().unwrap(),
            op: EditOpKind::MoveShot {
                shot_id: "shot-003".into(),
                from_position: 2,
                to_position: 1,
            },
        };

        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], "move_shot");
        assert_eq!(json["shot_id"], "shot-003");
        assert_eq!(json["from_position"], 2);
        assert_eq!(json["to_position"], 1);

        let back: EditOp = serde_json::from_value(json).unwrap();
        assert_eq!(back, op);
    }

    #[test]
    fn edit_op_trim_shot_roundtrip() {
        let op = EditOp {
            id: 4,
            ts: "2026-02-19T13:03:45Z".parse().unwrap(),
            op: EditOpKind::TrimShot {
                shot_id: "shot-002".into(),
                old_range: ShotRange::Words { from: 200, to: 280 },
                new_range: ShotRange::Words { from: 210, to: 265 },
            },
        };

        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], "trim_shot");
        assert_eq!(
            json["old_range"],
            json!({ "words": { "from": 200, "to": 280 } })
        );
        assert_eq!(
            json["new_range"],
            json!({ "words": { "from": 210, "to": 265 } })
        );

        let back: EditOp = serde_json::from_value(json).unwrap();
        assert_eq!(back, op);
    }

    #[test]
    fn edit_op_replace_range_type_roundtrip() {
        let op = EditOp {
            id: 5,
            ts: "2026-02-19T13:04:00Z".parse().unwrap(),
            op: EditOpKind::ReplaceRangeType {
                shot_id: "shot-001".into(),
                old_range: ShotRange::Words { from: 0, to: 52 },
                new_range: ShotRange::Time {
                    from_ms: 0,
                    to_ms: 12400,
                },
            },
        };

        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], "replace_range_type");

        let back: EditOp = serde_json::from_value(json).unwrap();
        assert_eq!(back, op);
    }

    // -- Full document round-trips from spec JSON ----------------------------

    #[test]
    fn manifest_from_spec_json() {
        let input = json!({
            "version": "1.0.0",
            "name": "my-project",
            "created": "2026-02-19T12:00:00Z",
            "sources": [
                {
                    "id": "src-001",
                    "path": "sources/src-001.mp4",
                    "original_filename": "interview-alice.mp4",
                    "duration_ms": 124500,
                    "video_codec": "h264",
                    "audio_codec": "aac",
                    "resolution": [1920, 1080],
                    "frame_rate": 29.97,
                    "audio_channels": 2,
                    "audio_sample_rate": 48000,
                    "added": "2026-02-19T12:01:00Z",
                    "transcribed": true,
                    "indexed": true
                }
            ],
            "next_source_id": 2,
            "defaults": {
                "whisper_model": "base",
                "thumbnail_interval_sec": 10,
                "render_codec": "h264",
                "render_container": "mp4"
            }
        });

        let manifest: Manifest = serde_json::from_value(input).unwrap();
        assert_eq!(manifest.name, "my-project");
        assert_eq!(manifest.sources.len(), 1);
        assert_eq!(manifest.sources[0].id, "src-001");
        assert_eq!(manifest.sources[0].resolution, (1920, 1080));
        assert_eq!(manifest.defaults.whisper_model, "base");

        // round-trip
        let json = serde_json::to_value(&manifest).unwrap();
        let back: Manifest = serde_json::from_value(json).unwrap();
        assert_eq!(back, manifest);
    }

    #[test]
    fn transcript_from_spec_json() {
        let input = json!({
            "source_id": "src-001",
            "model": "base",
            "language": "en",
            "duration_ms": 124500,
            "segments": [
                {
                    "index": 0,
                    "start_ms": 0,
                    "end_ms": 5230,
                    "text": "Welcome to the interview",
                    "words": [
                        { "index": 0, "text": "Welcome", "start_ms": 0, "end_ms": 420, "confidence": 0.95 },
                        { "index": 1, "text": "to", "start_ms": 420, "end_ms": 540, "confidence": 0.97 }
                    ]
                }
            ],
            "word_count": 487
        });

        let transcript: Transcript = serde_json::from_value(input).unwrap();
        assert_eq!(transcript.source_id, "src-001");
        assert_eq!(transcript.segments[0].words[0].confidence, 0.95);

        let json = serde_json::to_value(&transcript).unwrap();
        let back: Transcript = serde_json::from_value(json).unwrap();
        assert_eq!(back, transcript);
    }

    #[test]
    fn edit_document_from_spec_json() {
        let input = json!({
            "name": "rough-cut",
            "created": "2026-02-19T13:00:00Z",
            "next_shot_id": 6,
            "head": 4,
            "ops": [
                {
                    "id": 0,
                    "ts": "2026-02-19T13:00:01Z",
                    "op": "add_shot",
                    "shot": {
                        "id": "shot-001",
                        "source": "src-001",
                        "range": { "words": { "from": 0, "to": 52 } }
                    }
                },
                {
                    "id": 2,
                    "ts": "2026-02-19T13:01:02Z",
                    "op": "add_shot",
                    "shot": {
                        "id": "shot-003",
                        "source": "src-002",
                        "range": { "scenes": { "from": 0, "to": 2 } }
                    }
                },
                {
                    "id": 3,
                    "ts": "2026-02-19T13:02:30Z",
                    "op": "move_shot",
                    "shot_id": "shot-003",
                    "from_position": 2,
                    "to_position": 1
                },
                {
                    "id": 4,
                    "ts": "2026-02-19T13:03:45Z",
                    "op": "trim_shot",
                    "shot_id": "shot-002",
                    "old_range": { "words": { "from": 200, "to": 280 } },
                    "new_range": { "words": { "from": 210, "to": 265 } }
                }
            ],
            "snapshot": {
                "shots": [
                    { "id": "shot-001", "source": "src-001", "range": { "words": { "from": 0, "to": 52 } } },
                    { "id": "shot-003", "source": "src-002", "range": { "scenes": { "from": 0, "to": 2 } } },
                    { "id": "shot-002", "source": "src-003", "range": { "words": { "from": 210, "to": 265 } } }
                ]
            }
        });

        let doc: EditDocument = serde_json::from_value(input).unwrap();
        assert_eq!(doc.name, "rough-cut");
        assert_eq!(doc.ops.len(), 4);
        assert_eq!(doc.snapshot.shots.len(), 3);

        // Verify op variants parsed correctly
        assert!(matches!(&doc.ops[0].op, EditOpKind::AddShot { .. }));
        assert!(matches!(&doc.ops[2].op, EditOpKind::MoveShot { .. }));
        assert!(matches!(&doc.ops[3].op, EditOpKind::TrimShot { .. }));

        let json = serde_json::to_value(&doc).unwrap();
        let back: EditDocument = serde_json::from_value(json).unwrap();
        assert_eq!(back, doc);
    }

    #[test]
    fn source_index_from_spec_json() {
        let input = json!({
            "source_id": "src-001",
            "indexed_at": "2026-02-19T12:05:00Z",
            "metadata": {
                "duration_ms": 124500,
                "resolution": [1920, 1080],
                "codec": "h264",
                "file_size_bytes": 52428800
            },
            "thumbnails": [
                { "path": "thumbnails/src-001_00m00s.jpg", "timestamp_ms": 0, "description": null },
                { "path": "thumbnails/src-001_00m18s.jpg", "timestamp_ms": 18000, "description": "Scene change detected" }
            ],
            "scene_count": 4,
            "scenes": [
                {
                    "index": 0,
                    "start_ms": 0,
                    "end_ms": 18000,
                    "thumbnail": "thumbnails/src-001_00m00s.jpg",
                    "description": "Interior office, wide shot"
                },
                {
                    "index": 1,
                    "start_ms": 18000,
                    "end_ms": 45000,
                    "thumbnail": "thumbnails/src-001_00m18s.jpg",
                    "description": null
                }
            ]
        });

        let idx: SourceIndex = serde_json::from_value(input).unwrap();
        assert_eq!(idx.source_id, "src-001");
        assert_eq!(idx.metadata.resolution, (1920, 1080));
        assert_eq!(idx.thumbnails.len(), 2);
        assert_eq!(
            idx.scenes[0].description.as_deref(),
            Some("Interior office, wide shot")
        );
        assert_eq!(idx.scenes[1].description, None);

        let json = serde_json::to_value(&idx).unwrap();
        let back: SourceIndex = serde_json::from_value(json).unwrap();
        assert_eq!(back, idx);
    }

    #[test]
    fn source_markers_from_spec_json() {
        let input = json!({
            "source_id": "src-001",
            "markers": [
                {
                    "id": "mark-001",
                    "range": { "words": { "from": 45, "to": 120 } },
                    "label": "select",
                    "note": "Best take of the climate answer",
                    "created": "2026-02-19T14:00:00Z"
                },
                {
                    "id": "mark-002",
                    "range": { "time": { "from_ms": 62000, "to_ms": 68000 } },
                    "label": "avoid",
                    "note": null,
                    "created": "2026-02-19T14:01:00Z"
                }
            ]
        });

        let markers: SourceMarkers = serde_json::from_value(input).unwrap();
        assert_eq!(markers.markers.len(), 2);
        assert_eq!(markers.markers[0].label, "select");
        assert_eq!(
            markers.markers[0].range,
            ShotRange::Words { from: 45, to: 120 }
        );
        assert_eq!(markers.markers[1].note, None);

        let json = serde_json::to_value(&markers).unwrap();
        let back: SourceMarkers = serde_json::from_value(json).unwrap();
        assert_eq!(back, markers);
    }

    #[test]
    fn edit_op_add_note_roundtrip() {
        let op = EditOp {
            id: 6,
            ts: "2026-02-19T15:00:00Z".parse().unwrap(),
            op: EditOpKind::AddNote {
                shot_id: "shot-002".into(),
                note: ShotNote {
                    text: "Too long, trim the first half".into(),
                    created: "2026-02-19T15:00:00Z".parse().unwrap(),
                },
            },
        };

        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], "add_note");
        assert_eq!(json["shot_id"], "shot-002");
        assert_eq!(json["note"]["text"], "Too long, trim the first half");

        let back: EditOp = serde_json::from_value(json).unwrap();
        assert_eq!(back, op);
    }

    #[test]
    fn shot_notes_included_when_present() {
        let shot = Shot {
            id: "shot-002".into(),
            source: "src-003".into(),
            range: ShotRange::Words { from: 200, to: 280 },
            notes: vec![ShotNote {
                text: "Too long, trim the first half".into(),
                created: "2026-02-19T15:00:00Z".parse().unwrap(),
            }],
        };

        let json = serde_json::to_value(&shot).unwrap();
        assert!(json.get("notes").is_some());
        assert_eq!(json["notes"][0]["text"], "Too long, trim the first half");

        let back: Shot = serde_json::from_value(json).unwrap();
        assert_eq!(back, shot);
    }

    #[test]
    fn shot_notes_omitted_when_empty() {
        let shot = Shot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 52 },
            notes: vec![],
        };

        let json = serde_json::to_value(&shot).unwrap();
        assert!(json.get("notes").is_none());

        let back: Shot = serde_json::from_value(json).unwrap();
        assert_eq!(back, shot);
    }

    // -- PoiPoint -------------------------------------------------------------

    #[test]
    fn poi_point_word_roundtrip() {
        let point = PoiPoint::Word(45);
        let json = serde_json::to_value(&point).unwrap();
        assert_eq!(json, json!({ "word": 45 }));

        let back: PoiPoint = serde_json::from_value(json).unwrap();
        assert_eq!(back, point);
    }

    #[test]
    fn poi_point_scene_roundtrip() {
        let point = PoiPoint::Scene(3);
        let json = serde_json::to_value(&point).unwrap();
        assert_eq!(json, json!({ "scene": 3 }));

        let back: PoiPoint = serde_json::from_value(json).unwrap();
        assert_eq!(back, point);
    }

    #[test]
    fn poi_point_time_ms_roundtrip() {
        let point = PoiPoint::TimeMs(62500);
        let json = serde_json::to_value(&point).unwrap();
        assert_eq!(json, json!({ "time_ms": 62500 }));

        let back: PoiPoint = serde_json::from_value(json).unwrap();
        assert_eq!(back, point);
    }

    // -- PoiCategory ----------------------------------------------------------

    #[test]
    fn poi_category_serde_roundtrip() {
        for (cat, expected) in [
            (PoiCategory::Highlight, "highlight"),
            (PoiCategory::Issue, "issue"),
            (PoiCategory::Transition, "transition"),
            (PoiCategory::Cue, "cue"),
            (PoiCategory::Note, "note"),
        ] {
            let json = serde_json::to_value(cat).unwrap();
            assert_eq!(json, json!(expected));
            let back: PoiCategory = serde_json::from_value(json).unwrap();
            assert_eq!(back, cat);
        }
    }

    #[test]
    fn poi_category_from_str_valid() {
        assert_eq!("highlight".parse::<PoiCategory>().unwrap(), PoiCategory::Highlight);
        assert_eq!("issue".parse::<PoiCategory>().unwrap(), PoiCategory::Issue);
        assert_eq!("transition".parse::<PoiCategory>().unwrap(), PoiCategory::Transition);
        assert_eq!("cue".parse::<PoiCategory>().unwrap(), PoiCategory::Cue);
        assert_eq!("note".parse::<PoiCategory>().unwrap(), PoiCategory::Note);
    }

    #[test]
    fn poi_category_from_str_rejects_unknown() {
        let err = "unknown".parse::<PoiCategory>().unwrap_err();
        assert!(err.contains("unknown POI category"));
        assert!(err.contains("highlight"));
    }

    #[test]
    fn poi_category_display() {
        assert_eq!(PoiCategory::Highlight.to_string(), "highlight");
        assert_eq!(PoiCategory::Issue.to_string(), "issue");
        assert_eq!(PoiCategory::Transition.to_string(), "transition");
        assert_eq!(PoiCategory::Cue.to_string(), "cue");
        assert_eq!(PoiCategory::Note.to_string(), "note");
    }

    // -- SourcePois -----------------------------------------------------------

    #[test]
    fn source_pois_from_spec_json() {
        let input = json!({
            "source_id": "src-001",
            "pois": [
                {
                    "id": "poi-001",
                    "point": { "word": 45 },
                    "category": "highlight",
                    "note": "Perfect delivery of the key statistic",
                    "created": "2026-03-13T10:30:00Z"
                },
                {
                    "id": "poi-002",
                    "point": { "time_ms": 62500 },
                    "category": "issue",
                    "note": "Microphone bump",
                    "created": "2026-03-13T10:31:15Z"
                },
                {
                    "id": "poi-003",
                    "point": { "scene": 4 },
                    "category": "transition",
                    "created": "2026-03-13T10:32:00Z"
                }
            ]
        });

        let pois: SourcePois = serde_json::from_value(input).unwrap();
        assert_eq!(pois.source_id, "src-001");
        assert_eq!(pois.pois.len(), 3);

        assert_eq!(pois.pois[0].point, PoiPoint::Word(45));
        assert_eq!(pois.pois[0].category, PoiCategory::Highlight);
        assert_eq!(
            pois.pois[0].note.as_deref(),
            Some("Perfect delivery of the key statistic")
        );

        assert_eq!(pois.pois[1].point, PoiPoint::TimeMs(62500));
        assert_eq!(pois.pois[1].category, PoiCategory::Issue);

        assert_eq!(pois.pois[2].point, PoiPoint::Scene(4));
        assert_eq!(pois.pois[2].category, PoiCategory::Transition);
        assert_eq!(pois.pois[2].note, None);

        let json = serde_json::to_value(&pois).unwrap();
        let back: SourcePois = serde_json::from_value(json).unwrap();
        assert_eq!(back, pois);
    }

    #[test]
    fn poi_note_omitted_when_none() {
        let poi = Poi {
            id: "poi-001".into(),
            point: PoiPoint::Word(10),
            category: PoiCategory::Cue,
            note: None,
            created: "2026-03-13T10:00:00Z".parse().unwrap(),
        };

        let json = serde_json::to_value(&poi).unwrap();
        assert!(json.get("note").is_none());

        let back: Poi = serde_json::from_value(json).unwrap();
        assert_eq!(back, poi);
    }
}
