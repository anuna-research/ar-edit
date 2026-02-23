//! TEST-057: Agent reads markers in transcript
//!
//! Verifies that markers are correctly interleaved into transcript output
//! so that an LLM agent can consume a single chronological stream of
//! segments and markers. Tests the `interleave_transcript_with_markers`
//! and `resolve_markers` APIs.

use ar_edit_core::display::{
    interleave_transcript_with_markers, resolve_markers, InterleavedTranscript, ResolvedMarker,
    TranscriptItem,
};
use ar_edit_core::marker::{add_marker, list_markers};
use ar_edit_core::models::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 30000,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 10000,
                text: "Welcome to the interview about climate policy".into(),
                words: vec![
                    Word {
                        index: 0,
                        text: "Welcome".into(),
                        start_ms: 0,
                        end_ms: 500,
                        confidence: 0.95,
                    },
                    Word {
                        index: 1,
                        text: "to".into(),
                        start_ms: 500,
                        end_ms: 700,
                        confidence: 0.97,
                    },
                    Word {
                        index: 2,
                        text: "the".into(),
                        start_ms: 700,
                        end_ms: 900,
                        confidence: 0.98,
                    },
                    Word {
                        index: 3,
                        text: "interview".into(),
                        start_ms: 900,
                        end_ms: 1500,
                        confidence: 0.96,
                    },
                    Word {
                        index: 4,
                        text: "about".into(),
                        start_ms: 1500,
                        end_ms: 2000,
                        confidence: 0.94,
                    },
                    Word {
                        index: 5,
                        text: "climate".into(),
                        start_ms: 2000,
                        end_ms: 2600,
                        confidence: 0.93,
                    },
                    Word {
                        index: 6,
                        text: "policy".into(),
                        start_ms: 2600,
                        end_ms: 3200,
                        confidence: 0.95,
                    },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 10000,
                end_ms: 20000,
                text: "Today we discuss renewable energy solutions".into(),
                words: vec![
                    Word {
                        index: 7,
                        text: "Today".into(),
                        start_ms: 10000,
                        end_ms: 10500,
                        confidence: 0.94,
                    },
                    Word {
                        index: 8,
                        text: "we".into(),
                        start_ms: 10500,
                        end_ms: 10700,
                        confidence: 0.99,
                    },
                    Word {
                        index: 9,
                        text: "discuss".into(),
                        start_ms: 10700,
                        end_ms: 11200,
                        confidence: 0.93,
                    },
                    Word {
                        index: 10,
                        text: "renewable".into(),
                        start_ms: 11200,
                        end_ms: 11800,
                        confidence: 0.91,
                    },
                    Word {
                        index: 11,
                        text: "energy".into(),
                        start_ms: 11800,
                        end_ms: 12300,
                        confidence: 0.92,
                    },
                    Word {
                        index: 12,
                        text: "solutions".into(),
                        start_ms: 12300,
                        end_ms: 13000,
                        confidence: 0.90,
                    },
                ],
            },
            TranscriptSegment {
                index: 2,
                start_ms: 20000,
                end_ms: 30000,
                text: "Thank you for watching".into(),
                words: vec![
                    Word {
                        index: 13,
                        text: "Thank".into(),
                        start_ms: 20000,
                        end_ms: 20500,
                        confidence: 0.96,
                    },
                    Word {
                        index: 14,
                        text: "you".into(),
                        start_ms: 20500,
                        end_ms: 20800,
                        confidence: 0.97,
                    },
                    Word {
                        index: 15,
                        text: "for".into(),
                        start_ms: 20800,
                        end_ms: 21000,
                        confidence: 0.98,
                    },
                    Word {
                        index: 16,
                        text: "watching".into(),
                        start_ms: 21000,
                        end_ms: 21600,
                        confidence: 0.95,
                    },
                ],
            },
        ],
        word_count: 17,
    }
}

fn setup_project_with_transcript(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("annotations")).unwrap();

    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();
}

fn make_resolved_marker(id: &str, label: &str, start_ms: u64, end_ms: u64) -> ResolvedMarker {
    ResolvedMarker {
        id: id.into(),
        source_id: "src-001".into(),
        range: ShotRange::Time {
            from_ms: start_ms,
            to_ms: end_ms,
        },
        label: label.into(),
        note: None,
        created: "2026-02-19T14:00:00Z".parse().unwrap(),
        start_ms,
        end_ms,
        duration_ms: end_ms.saturating_sub(start_ms),
        text_preview: None,
        scene_preview: None,
    }
}

// ---------------------------------------------------------------------------
// Tests: interleave_transcript_with_markers
// ---------------------------------------------------------------------------

#[test]
fn interleave_places_marker_between_segments() {
    let t = make_transcript();
    let markers = vec![make_resolved_marker("mark-001", "select", 5000, 8000)];

    let result = interleave_transcript_with_markers(&t, &markers);
    assert_eq!(result.items.len(), 4); // 3 segments + 1 marker
    assert_eq!(result.marker_count, 1);

    // seg 0 (0ms), marker (5000ms), seg 1 (10000ms), seg 2 (20000ms)
    assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
    assert!(matches!(&result.items[1], TranscriptItem::Marker(m) if m.id == "mark-001"));
    assert!(matches!(&result.items[2], TranscriptItem::Segment(s) if s.index == 1));
    assert!(matches!(&result.items[3], TranscriptItem::Segment(s) if s.index == 2));
}

#[test]
fn interleave_multiple_markers_chronological() {
    let t = make_transcript();
    let markers = vec![
        make_resolved_marker("mark-001", "select", 1000, 3000),
        make_resolved_marker("mark-002", "avoid", 15000, 18000),
        make_resolved_marker("mark-003", "hero", 25000, 28000),
    ];

    let result = interleave_transcript_with_markers(&t, &markers);
    assert_eq!(result.items.len(), 6); // 3 segments + 3 markers
    assert_eq!(result.marker_count, 3);

    // seg 0 (0ms), mark-001 (1000ms), seg 1 (10000ms), mark-002 (15000ms),
    // seg 2 (20000ms), mark-003 (25000ms)
    assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
    assert!(matches!(&result.items[1], TranscriptItem::Marker(m) if m.id == "mark-001"));
    assert!(matches!(&result.items[2], TranscriptItem::Segment(s) if s.index == 1));
    assert!(matches!(&result.items[3], TranscriptItem::Marker(m) if m.id == "mark-002"));
    assert!(matches!(&result.items[4], TranscriptItem::Segment(s) if s.index == 2));
    assert!(matches!(&result.items[5], TranscriptItem::Marker(m) if m.id == "mark-003"));
}

#[test]
fn interleave_segment_before_marker_at_same_time() {
    let t = make_transcript();
    // Marker at exactly the same start_ms as segment 1 (10000ms)
    let markers = vec![make_resolved_marker("mark-001", "review", 10000, 12000)];

    let result = interleave_transcript_with_markers(&t, &markers);
    assert_eq!(result.items.len(), 4);

    // Segment should appear before marker at same timestamp
    assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
    assert!(matches!(&result.items[1], TranscriptItem::Segment(s) if s.index == 1));
    assert!(matches!(&result.items[2], TranscriptItem::Marker(m) if m.id == "mark-001"));
    assert!(matches!(&result.items[3], TranscriptItem::Segment(s) if s.index == 2));
}

#[test]
fn interleave_no_markers_returns_segments_only() {
    let t = make_transcript();
    let result = interleave_transcript_with_markers(&t, &[]);

    assert_eq!(result.items.len(), 3);
    assert_eq!(result.marker_count, 0);
    assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
    assert!(matches!(&result.items[1], TranscriptItem::Segment(s) if s.index == 1));
    assert!(matches!(&result.items[2], TranscriptItem::Segment(s) if s.index == 2));
}

#[test]
fn interleave_preserves_metadata() {
    let t = make_transcript();
    let markers = vec![make_resolved_marker("mark-001", "select", 5000, 8000)];

    let result = interleave_transcript_with_markers(&t, &markers);
    assert_eq!(result.source_id, "src-001");
    assert_eq!(result.duration_ms, 30000);
    assert_eq!(result.word_count, 17);
    assert_eq!(result.marker_count, 1);
}

#[test]
fn interleave_json_has_type_tag() {
    let t = make_transcript();
    let markers = vec![make_resolved_marker("mark-001", "select", 5000, 8000)];

    let result = interleave_transcript_with_markers(&t, &markers);
    let json = serde_json::to_value(&result).unwrap();

    // Segments get type "segment", markers get type "marker"
    assert_eq!(json["items"][0]["type"], "segment");
    assert_eq!(json["items"][1]["type"], "marker");
    assert_eq!(json["items"][1]["label"], "select");
    assert_eq!(json["items"][1]["id"], "mark-001");
}

#[test]
fn interleave_roundtrip_serialization() {
    let t = make_transcript();
    let markers = vec![
        make_resolved_marker("mark-001", "select", 5000, 8000),
        make_resolved_marker("mark-002", "avoid", 15000, 18000),
    ];

    let result = interleave_transcript_with_markers(&t, &markers);
    let json = serde_json::to_string(&result).unwrap();
    let back: InterleavedTranscript = serde_json::from_str(&json).unwrap();

    assert_eq!(back.items.len(), result.items.len());
    assert_eq!(back.marker_count, 2);
    assert_eq!(back.source_id, "src-001");
}

// ---------------------------------------------------------------------------
// Tests: resolve_markers
// ---------------------------------------------------------------------------

#[test]
fn resolve_markers_from_disk() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_transcript(tmp.path());

    // Create markers using the add_marker API
    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 3 },
        "select",
        Some("Good intro"),
    )
    .unwrap();

    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 7, to: 12 },
        "hero",
        None,
    )
    .unwrap();

    // Load and resolve
    let loaded = list_markers(tmp.path(), "src-001").unwrap();
    let resolved = resolve_markers(&loaded.markers, "src-001", tmp.path()).unwrap();

    assert_eq!(resolved.len(), 2);

    // First marker: words 0..3 -> "Welcome to the interview"
    assert_eq!(resolved[0].id, "mark-001");
    assert_eq!(resolved[0].label, "select");
    assert_eq!(resolved[0].start_ms, 0);
    assert_eq!(resolved[0].end_ms, 1500);
    assert!(resolved[0].text_preview.is_some());
    assert_eq!(resolved[0].note.as_deref(), Some("Good intro"));

    // Second marker: words 7..12
    assert_eq!(resolved[1].id, "mark-002");
    assert_eq!(resolved[1].label, "hero");
    assert_eq!(resolved[1].start_ms, 10000);
    assert_eq!(resolved[1].end_ms, 13000);
    assert!(resolved[1].text_preview.is_some());
}

#[test]
fn resolve_then_interleave_end_to_end() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_transcript(tmp.path());

    // Create markers
    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 5, to: 6 },
        "select",
        Some("Key phrase: climate policy"),
    )
    .unwrap();

    // Load, resolve, then interleave
    let loaded = list_markers(tmp.path(), "src-001").unwrap();
    let resolved = resolve_markers(&loaded.markers, "src-001", tmp.path()).unwrap();

    let transcript = make_transcript();
    let interleaved = interleave_transcript_with_markers(&transcript, &resolved);

    assert_eq!(interleaved.marker_count, 1);
    assert_eq!(interleaved.items.len(), 4); // 3 segments + 1 marker

    // Marker for words 5-6 starts at 2000ms, should be between seg 0 (0ms) and seg 1 (10000ms)
    assert!(matches!(&interleaved.items[0], TranscriptItem::Segment(s) if s.index == 0));
    assert!(matches!(&interleaved.items[1], TranscriptItem::Marker(m) if m.label == "select"));
    assert!(matches!(&interleaved.items[2], TranscriptItem::Segment(s) if s.index == 1));
}

#[test]
fn resolved_marker_includes_note() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_transcript(tmp.path());

    add_marker(
        tmp.path(),
        "src-001",
        ShotRange::Words { from: 0, to: 3 },
        "select",
        Some("Best take of the opening"),
    )
    .unwrap();

    let loaded = list_markers(tmp.path(), "src-001").unwrap();
    let resolved = resolve_markers(&loaded.markers, "src-001", tmp.path()).unwrap();

    assert_eq!(
        resolved[0].note.as_deref(),
        Some("Best take of the opening")
    );
}

#[test]
fn agent_sees_marker_labels_in_json() {
    let t = make_transcript();
    let markers = vec![
        make_resolved_marker("mark-001", "select", 1000, 3000),
        make_resolved_marker("mark-002", "avoid", 15000, 18000),
    ];

    let result = interleave_transcript_with_markers(&t, &markers);
    let json = serde_json::to_value(&result).unwrap();
    let items = json["items"].as_array().unwrap();

    // Find marker items and verify agent-readable fields
    let marker_items: Vec<_> = items
        .iter()
        .filter(|item| item["type"] == "marker")
        .collect();

    assert_eq!(marker_items.len(), 2);
    assert_eq!(marker_items[0]["label"], "select");
    assert_eq!(marker_items[0]["start_ms"], 1000);
    assert_eq!(marker_items[0]["end_ms"], 3000);
    assert_eq!(marker_items[1]["label"], "avoid");
    assert_eq!(marker_items[1]["start_ms"], 15000);
}
