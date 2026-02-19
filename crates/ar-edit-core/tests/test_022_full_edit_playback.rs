//! TEST-022: Full edit playback (REQ-022, CON-006)
//!
//! Given an edit document with multiple shots,
//! when a full preview is resolved,
//! then all shots resolve in sequence with correct timestamps.
//!
//! This integration test verifies the full edit playback pipeline:
//!   edit document → resolve all shots → verify ordering/durations →
//!   build shot timings → verify feedback resolution.

use ar_edit_core::display;
use ar_edit_core::feedback;
use ar_edit_core::models::*;
use ar_edit_core::render;

use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 124500,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 5230,
                text: "Welcome to the interview".into(),
                words: vec![
                    Word { index: 0, text: "Welcome".into(), start_ms: 0, end_ms: 420, confidence: 0.95 },
                    Word { index: 1, text: "to".into(), start_ms: 420, end_ms: 540, confidence: 0.97 },
                    Word { index: 2, text: "the".into(), start_ms: 540, end_ms: 650, confidence: 0.98 },
                    Word { index: 3, text: "interview".into(), start_ms: 650, end_ms: 1200, confidence: 0.96 },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 5230,
                end_ms: 12400,
                text: "Today we discuss climate".into(),
                words: vec![
                    Word { index: 4, text: "Today".into(), start_ms: 5230, end_ms: 5600, confidence: 0.94 },
                    Word { index: 5, text: "we".into(), start_ms: 5600, end_ms: 5750, confidence: 0.99 },
                    Word { index: 6, text: "discuss".into(), start_ms: 5750, end_ms: 6200, confidence: 0.93 },
                    Word { index: 7, text: "climate".into(), start_ms: 6200, end_ms: 6800, confidence: 0.91 },
                ],
            },
        ],
        word_count: 8,
    }
}

fn make_source_index(source_id: &str) -> SourceIndex {
    SourceIndex {
        source_id: source_id.into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms: 90000,
            resolution: (1920, 1080),
            codec: "h264".into(),
            file_size_bytes: 52428800,
        },
        thumbnails: vec![],
        scene_count: 3,
        scenes: vec![
            Scene {
                index: 0,
                start_ms: 0,
                end_ms: 18000,
                thumbnail: format!("thumbnails/{source_id}_00m00s.jpg").into(),
                description: Some("Interior office, wide shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 18000,
                end_ms: 45000,
                thumbnail: format!("thumbnails/{source_id}_00m18s.jpg").into(),
                description: None,
            },
            Scene {
                index: 2,
                start_ms: 45000,
                end_ms: 90000,
                thumbnail: format!("thumbnails/{source_id}_00m45s.jpg").into(),
                description: Some("Close-up interview".into()),
            },
        ],
    }
}

fn setup_project(dir: &Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("edits")).unwrap();

    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();

    let index = make_source_index("src-002");
    std::fs::write(
        dir.join("index/src-002.index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Full edit: resolve all shots in sequence
// ---------------------------------------------------------------------------

#[test]
fn full_playback_resolves_all_shots_in_order() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 });
    doc.add_shot("src-001", ShotRange::Time { from_ms: 10000, to_ms: 15000 });

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);

    assert_eq!(resolved[0].id, "shot-001");
    assert_eq!(resolved[0].source, "src-001");

    assert_eq!(resolved[1].id, "shot-002");
    assert_eq!(resolved[1].source, "src-002");

    assert_eq!(resolved[2].id, "shot-003");
    assert_eq!(resolved[2].source, "src-001");
}

#[test]
fn full_playback_total_duration_is_sum_of_shots() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("rough-cut");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });     // 0..1200 = 1200ms
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 0 });    // 0..18000 = 18000ms
    doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 8000 }); // 3000ms

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();

    let total: u64 = resolved.iter().map(|s| s.duration_ms).sum();
    assert_eq!(total, 1200 + 18000 + 3000);
    assert_eq!(total, 22200);
}

#[test]
fn full_playback_mixed_range_types() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("mixed-edit");
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 2, to: 2 });
    doc.add_shot("src-001", ShotRange::Time { from_ms: 0, to_ms: 500 });

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);

    // Words: word 4 (5230ms) to word 7 (6800ms)
    assert_eq!(resolved[0].start_ms, 5230);
    assert_eq!(resolved[0].end_ms, 6800);
    assert_eq!(resolved[0].duration_ms, 1570);

    // Scenes: scene 2 (45000..90000)
    assert_eq!(resolved[1].start_ms, 45000);
    assert_eq!(resolved[1].end_ms, 90000);
    assert_eq!(resolved[1].duration_ms, 45000);

    // Time: direct passthrough
    assert_eq!(resolved[2].start_ms, 0);
    assert_eq!(resolved[2].end_ms, 500);
    assert_eq!(resolved[2].duration_ms, 500);
}

// ---------------------------------------------------------------------------
// Full edit: resolve_preview API
// ---------------------------------------------------------------------------

#[test]
fn full_playback_resolve_preview_returns_shots() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("preview-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

    let resolved = render::resolve_preview(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].id, "shot-001");
    assert_eq!(resolved[1].id, "shot-002");
}

#[test]
fn full_playback_empty_edit_returns_error() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let doc = EditDocument::create("empty-edit");
    let result = render::resolve_preview(&doc, tmp.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no shots"));
}

// ---------------------------------------------------------------------------
// Full edit: shot timings for feedback (CON-006, REQ-037)
// ---------------------------------------------------------------------------

#[test]
fn full_playback_shot_timings_are_contiguous() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("timeline");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });     // 1200ms
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 0 });    // 18000ms
    doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 8000 }); // 3000ms

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();

    // Build timeline offsets (cumulative)
    let mut timings: Vec<(String, String, u64, u64)> = Vec::new();
    let mut offset = 0u64;
    for shot in &resolved {
        timings.push((
            shot.id.clone(),
            shot.source.clone(),
            offset,
            offset + shot.duration_ms,
        ));
        offset += shot.duration_ms;
    }

    // Verify contiguous timeline
    assert_eq!(timings[0], ("shot-001".into(), "src-001".into(), 0, 1200));
    assert_eq!(timings[1], ("shot-002".into(), "src-002".into(), 1200, 19200));
    assert_eq!(timings[2], ("shot-003".into(), "src-001".into(), 19200, 22200));

    // Each shot starts where the previous one ended
    for i in 1..timings.len() {
        assert_eq!(timings[i].2, timings[i - 1].3, "shot {i} should start at end of shot {}", i - 1);
    }
}

#[test]
fn full_playback_edit_feedback_finds_correct_shot() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("feedback-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();

    // Build shot timings (cumulative timeline)
    let mut timings: Vec<(String, String, u64, u64)> = Vec::new();
    let mut offset = 0u64;
    for shot in &resolved {
        timings.push((
            shot.id.clone(),
            shot.source.clone(),
            offset,
            offset + shot.duration_ms,
        ));
        offset += shot.duration_ms;
    }

    // Position within first shot
    let fb = feedback::build_edit_feedback(600, &timings, tmp.path());
    assert_eq!(fb.shot_id.as_deref(), Some("shot-001"));
    assert_eq!(fb.source_id, "src-001");
    assert_eq!(fb.last_position_ms, 600);

    // Position within second shot
    let fb2 = feedback::build_edit_feedback(1500, &timings, tmp.path());
    assert_eq!(fb2.shot_id.as_deref(), Some("shot-002"));
    assert_eq!(fb2.source_id, "src-001");
}

#[test]
fn full_playback_edit_feedback_at_shot_boundary() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let timings = vec![
        ("shot-001".into(), "src-001".into(), 0u64, 5000u64),
        ("shot-002".into(), "src-001".into(), 5000, 12000),
    ];

    // Exactly at boundary (5000ms = start of shot-002)
    let fb = feedback::build_edit_feedback(5000, &timings, tmp.path());
    assert_eq!(fb.shot_id.as_deref(), Some("shot-002"));
}

#[test]
fn full_playback_edit_feedback_past_end() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let timings = vec![
        ("shot-001".into(), "src-001".into(), 0u64, 5000u64),
    ];

    // Position past all shots — falls back to last shot
    let fb = feedback::build_edit_feedback(50000, &timings, tmp.path());
    assert_eq!(fb.shot_id.as_deref(), Some("shot-001"));
}

// ---------------------------------------------------------------------------
// Full edit: feedback serialization (CON-006 JSON output)
// ---------------------------------------------------------------------------

#[test]
fn full_playback_feedback_json_structure() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let fb = feedback::build_feedback(600, Some("shot-001"), "src-001", tmp.path());

    let json = serde_json::to_value(&fb).unwrap();
    assert_eq!(json["last_position_ms"], 600);
    assert_eq!(json["shot_id"], "shot-001");
    assert_eq!(json["source_id"], "src-001");
    // word_index and scene_index are resolved from transcript/index files
    assert!(json.get("word_index").is_some());
    assert!(json.get("scene_index").is_some());
}

// ---------------------------------------------------------------------------
// Full edit: concat list format
// ---------------------------------------------------------------------------

#[test]
fn full_playback_concat_list_format() {
    let tmp = TempDir::new().unwrap();
    let concat_path = tmp.path().join("concat.txt");

    // Simulate the concat list that render_preview would write
    let segments = vec![
        tmp.path().join("segment_0000.mp4"),
        tmp.path().join("segment_0001.mp4"),
        tmp.path().join("segment_0002.mp4"),
    ];

    let mut f = std::fs::File::create(&concat_path).unwrap();
    for path in &segments {
        writeln!(f, "file '{}'", path.display()).unwrap();
    }

    let content = std::fs::read_to_string(&concat_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3);

    for (i, line) in lines.iter().enumerate() {
        assert!(line.starts_with("file '"), "line {i} should start with file '");
        assert!(line.ends_with('\''), "line {i} should end with '");
        assert!(line.contains(&format!("segment_{i:04}.mp4")));
    }
}

// ---------------------------------------------------------------------------
// Full edit: previews included in resolved shots
// ---------------------------------------------------------------------------

#[test]
fn full_playback_resolved_shots_have_previews() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("preview-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });
    doc.add_shot("src-001", ShotRange::Time { from_ms: 1000, to_ms: 3000 });

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();

    // Words shot has text preview
    assert!(resolved[0].text_preview.is_some());
    assert!(resolved[0].scene_preview.is_none());

    // Scenes shot has scene preview
    assert!(resolved[1].scene_preview.is_some());
    assert!(resolved[1].text_preview.is_none());

    // Time shot has no preview
    assert!(resolved[2].text_preview.is_none());
    assert!(resolved[2].scene_preview.is_none());
}
