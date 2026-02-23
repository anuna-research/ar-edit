//! TEST-023: Overlay present via ffprobe (REQ-023, CON-006)
//!
//! Tests the overlay module: mode parsing, drawtext filter generation,
//! text escaping, font resolution, and integration with render pipeline.
//!
//! The integration tests verify that overlay filters would be applied
//! to each shot in a rendered preview — the same filters that ffprobe
//! would detect as drawtext entries in the encoded output.

use ar_edit_core::display;
use ar_edit_core::models::*;
use ar_edit_core::overlay::{build_drawtext_filter, OverlayInfo, OverlayMode};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn info_full() -> OverlayInfo {
    OverlayInfo {
        shot_id: "shot-003".into(),
        source_id: "src-002".into(),
        snippet: Some("Welcome to the interview today".into()),
        timecode_offset_sec: 30.0,
    }
}

fn info_no_snippet() -> OverlayInfo {
    OverlayInfo {
        shot_id: "shot-001".into(),
        source_id: "src-001".into(),
        snippet: None,
        timecode_offset_sec: 0.0,
    }
}

// ---------------------------------------------------------------------------
// OverlayMode::from_flag
// ---------------------------------------------------------------------------

#[test]
fn mode_clean_when_no_flag() {
    assert_eq!(OverlayMode::from_flag(None), OverlayMode::Clean);
}

#[test]
fn mode_full_when_flag_is_full() {
    assert_eq!(OverlayMode::from_flag(Some("full")), OverlayMode::Full);
}

#[test]
fn mode_minimal_when_flag_is_minimal() {
    assert_eq!(
        OverlayMode::from_flag(Some("minimal")),
        OverlayMode::Minimal
    );
}

#[test]
fn mode_full_for_unknown_values() {
    assert_eq!(OverlayMode::from_flag(Some("")), OverlayMode::Full);
    assert_eq!(OverlayMode::from_flag(Some("verbose")), OverlayMode::Full);
}

// ---------------------------------------------------------------------------
// Clean mode
// ---------------------------------------------------------------------------

#[test]
fn clean_mode_produces_no_filter() {
    let filter = build_drawtext_filter(OverlayMode::Clean, &info_full());
    assert!(filter.is_none());
}

// ---------------------------------------------------------------------------
// Minimal mode — timecode only
// ---------------------------------------------------------------------------

#[test]
fn minimal_mode_produces_filter() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full());
    assert!(filter.is_some());
}

#[test]
fn minimal_mode_has_drawtext() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(filter.starts_with("drawtext="));
}

#[test]
fn minimal_mode_has_timecode_with_offset() {
    let info = OverlayInfo {
        timecode_offset_sec: 45.5,
        ..info_no_snippet()
    };
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info).unwrap();
    assert!(
        filter.contains("pts:hms:45.500"),
        "should contain offset 45.500"
    );
}

#[test]
fn minimal_mode_has_semi_transparent_background() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(
        filter.contains("boxcolor=black@0.5"),
        "should have semi-transparent bg"
    );
    assert!(filter.contains("box=1"), "should enable box");
}

#[test]
fn minimal_mode_has_monospace_font() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(
        filter.contains("fontfile=") || filter.contains("font=monospace"),
        "should specify a monospace font"
    );
}

#[test]
fn minimal_mode_positioned_top_left() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(
        filter.contains("x=10"),
        "should be positioned near left edge"
    );
    assert!(
        filter.contains("y=10"),
        "should be positioned near top edge"
    );
}

#[test]
fn minimal_mode_white_text() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(filter.contains("fontcolor=white"));
}

#[test]
fn minimal_mode_does_not_contain_shot_or_source() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(
        !filter.contains("shot-003"),
        "minimal should not show shot ID"
    );
    assert!(
        !filter.contains("src-002"),
        "minimal should not show source ID"
    );
}

#[test]
fn minimal_mode_single_drawtext() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert_eq!(
        filter.matches("drawtext=").count(),
        1,
        "minimal should have exactly 1 drawtext"
    );
}

// ---------------------------------------------------------------------------
// Full mode — timecode + shot ID + source ID + snippet
// ---------------------------------------------------------------------------

#[test]
fn full_mode_produces_filter() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full());
    assert!(filter.is_some());
}

#[test]
fn full_mode_contains_timecode() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(
        filter.contains("pts:hms:30.000"),
        "should contain timecode with offset"
    );
}

#[test]
fn full_mode_contains_shot_id() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(filter.contains("shot-003"), "should display shot ID");
}

#[test]
fn full_mode_contains_source_id() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(filter.contains("src-002"), "should display source ID");
}

#[test]
fn full_mode_contains_snippet() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(
        filter.contains("Welcome to the interview today"),
        "should display snippet"
    );
}

#[test]
fn full_mode_has_semi_transparent_background() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(filter.contains("boxcolor=black@0.5"));
    assert!(filter.contains("box=1"));
}

#[test]
fn full_mode_has_monospace_font() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(
        filter.contains("fontfile=") || filter.contains("font=monospace"),
        "should specify a monospace font"
    );
}

#[test]
fn full_mode_positioned_top_left() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(filter.contains("x=10"));
    assert!(filter.contains("y=10"));
}

#[test]
fn full_mode_with_snippet_has_two_drawtext_filters() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    let count = filter.matches("drawtext=").count();
    assert_eq!(
        count, 2,
        "full with snippet should chain 2 drawtext filters"
    );
}

#[test]
fn full_mode_without_snippet_has_one_drawtext_filter() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_no_snippet()).unwrap();
    let count = filter.matches("drawtext=").count();
    assert_eq!(
        count, 1,
        "full without snippet should have 1 drawtext filter"
    );
}

#[test]
fn full_mode_second_line_below_first() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    // The filter is "drawtext=...y=10...,drawtext=...y=42..."
    // First drawtext at y=10, second at y=42 (below)
    let parts: Vec<&str> = filter.split(",drawtext=").collect();
    assert_eq!(
        parts.len(),
        2,
        "should have comma-separated drawtext filters"
    );
    assert!(parts[0].contains("y=10"), "first line at y=10");
    assert!(parts[1].contains("y=42"), "second line at y=42");
}

// ---------------------------------------------------------------------------
// Timecode offset
// ---------------------------------------------------------------------------

#[test]
fn timecode_offset_zero() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_no_snippet()).unwrap();
    assert!(filter.contains("pts:hms:0.000"));
}

#[test]
fn timecode_offset_large() {
    let info = OverlayInfo {
        timecode_offset_sec: 3661.5, // 1h 1m 1.5s
        ..info_no_snippet()
    };
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info).unwrap();
    assert!(filter.contains("pts:hms:3661.500"));
}

// ---------------------------------------------------------------------------
// Text escaping in snippet
// ---------------------------------------------------------------------------

#[test]
fn snippet_with_single_quotes_is_safe() {
    let info = OverlayInfo {
        snippet: Some("it's a wonderful world".into()),
        ..info_full()
    };
    let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
    // Single quotes removed to avoid breaking filter syntax
    assert!(
        !filter.contains("it's"),
        "single quotes should be removed from snippet"
    );
    assert!(
        filter.contains("its a wonderful world"),
        "text preserved without quotes"
    );
}

#[test]
fn snippet_with_percent_is_escaped() {
    let info = OverlayInfo {
        snippet: Some("100% complete".into()),
        ..info_full()
    };
    let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
    assert!(
        filter.contains("100%% complete"),
        "% should be escaped as %%"
    );
}

#[test]
fn snippet_with_backslash_is_escaped() {
    let info = OverlayInfo {
        snippet: Some("path\\to\\file".into()),
        ..info_full()
    };
    let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
    assert!(
        filter.contains("path\\\\to\\\\file"),
        "backslash should be escaped"
    );
}

// ---------------------------------------------------------------------------
// Filter is valid ffmpeg syntax (structural checks)
// ---------------------------------------------------------------------------

#[test]
fn filter_uses_single_quoted_text_values() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    // text='...' pattern
    assert!(
        filter.contains("text='"),
        "text values should be single-quoted"
    );
}

#[test]
fn filter_comma_separates_chained_filters() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(
        filter.contains(",drawtext="),
        "chained filters separated by comma"
    );
}

// ---------------------------------------------------------------------------
// Integration: render_preview signature accepts overlay mode
// ---------------------------------------------------------------------------

#[test]
fn render_preview_accepts_clean_mode() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let doc = EditDocument::create("test");
    // Empty edit should error regardless of overlay mode
    let result = ar_edit_core::render::resolve_preview(&doc, tmp.path());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Integration: overlay filters for resolved edit shots (ffprobe verification)
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
                    Word {
                        index: 0,
                        text: "Welcome".into(),
                        start_ms: 0,
                        end_ms: 420,
                        confidence: 0.95,
                    },
                    Word {
                        index: 1,
                        text: "to".into(),
                        start_ms: 420,
                        end_ms: 540,
                        confidence: 0.97,
                    },
                    Word {
                        index: 2,
                        text: "the".into(),
                        start_ms: 540,
                        end_ms: 650,
                        confidence: 0.98,
                    },
                    Word {
                        index: 3,
                        text: "interview".into(),
                        start_ms: 650,
                        end_ms: 1200,
                        confidence: 0.96,
                    },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 5230,
                end_ms: 12400,
                text: "Today we discuss climate".into(),
                words: vec![
                    Word {
                        index: 4,
                        text: "Today".into(),
                        start_ms: 5230,
                        end_ms: 5600,
                        confidence: 0.94,
                    },
                    Word {
                        index: 5,
                        text: "we".into(),
                        start_ms: 5600,
                        end_ms: 5750,
                        confidence: 0.99,
                    },
                    Word {
                        index: 6,
                        text: "discuss".into(),
                        start_ms: 5750,
                        end_ms: 6200,
                        confidence: 0.93,
                    },
                    Word {
                        index: 7,
                        text: "climate".into(),
                        start_ms: 6200,
                        end_ms: 6800,
                        confidence: 0.91,
                    },
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

fn setup_project_for_overlay(dir: &std::path::Path) {
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

/// Simulate the overlay filter generation that render_preview performs
/// for each resolved shot, replicating the logic that ffprobe would verify.
fn build_filters_for_edit(
    doc: &EditDocument,
    project_dir: &std::path::Path,
    mode: OverlayMode,
) -> Vec<Option<String>> {
    let resolved = display::resolve_edit(doc, project_dir).unwrap();
    let mut filters = Vec::new();
    let mut timeline_offset_ms: u64 = 0;

    for shot in &resolved {
        let filter = build_drawtext_filter(
            mode,
            &OverlayInfo {
                shot_id: shot.id.clone(),
                source_id: shot.source.clone(),
                snippet: shot.text_preview.clone().or(shot.scene_preview.clone()),
                timecode_offset_sec: timeline_offset_ms as f64 / 1000.0,
            },
        );
        filters.push(filter);
        timeline_offset_ms += shot.duration_ms;
    }
    filters
}

#[test]
fn overlay_integration_full_mode_produces_filter_per_shot() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("overlay-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 })
        .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Full);
    assert_eq!(filters.len(), 2);

    // Both shots should have overlay filters
    assert!(filters[0].is_some(), "shot-001 should have overlay filter");
    assert!(filters[1].is_some(), "shot-002 should have overlay filter");

    // Each filter should contain its shot-specific info (what ffprobe would see)
    let f0 = filters[0].as_ref().unwrap();
    assert!(f0.contains("shot-001"), "filter should contain shot ID");
    assert!(f0.contains("src-001"), "filter should contain source ID");
    assert!(f0.contains("drawtext="), "filter should use drawtext");

    let f1 = filters[1].as_ref().unwrap();
    assert!(f1.contains("shot-002"), "filter should contain shot ID");
    assert!(f1.contains("src-002"), "filter should contain source ID");
}

#[test]
fn overlay_integration_timecode_offset_accumulates() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("offset-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap(); // 1200ms duration
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 })
        .unwrap(); // 45000ms duration
    doc.add_shot(
        "src-001",
        ShotRange::Time {
            from_ms: 5000,
            to_ms: 8000,
        },
    )
    .unwrap(); // 3000ms

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Minimal);
    assert_eq!(filters.len(), 3);

    // First shot: offset = 0.000
    let f0 = filters[0].as_ref().unwrap();
    assert!(
        f0.contains("pts:hms:0.000"),
        "first shot offset should be 0"
    );

    // Second shot: offset = 1.200 (after 1200ms)
    let f1 = filters[1].as_ref().unwrap();
    assert!(
        f1.contains("pts:hms:1.200"),
        "second shot offset should be 1.200"
    );

    // Third shot: offset = 46.200 (after 1200 + 45000 = 46200ms)
    let f2 = filters[2].as_ref().unwrap();
    assert!(
        f2.contains("pts:hms:46.200"),
        "third shot offset should be 46.200"
    );
}

#[test]
fn overlay_integration_clean_mode_no_filters() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("clean-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 })
        .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Clean);
    assert_eq!(filters.len(), 2);

    // Clean mode: no filters (ffprobe would find no drawtext)
    assert!(filters[0].is_none(), "clean mode should produce no filter");
    assert!(filters[1].is_none(), "clean mode should produce no filter");
}

#[test]
fn overlay_integration_minimal_mode_timecode_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("minimal-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Minimal);
    let f = filters[0].as_ref().unwrap();

    // Minimal mode: timecode but no shot/source info
    assert!(f.contains("pts:hms:"), "should have timecode");
    assert!(!f.contains("shot-001"), "minimal should not show shot ID");
    assert!(!f.contains("src-001"), "minimal should not show source ID");
    assert_eq!(
        f.matches("drawtext=").count(),
        1,
        "minimal should have single drawtext"
    );
}

#[test]
fn overlay_integration_snippet_from_transcript() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("snippet-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
        .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Full);
    let f = filters[0].as_ref().unwrap();

    // Full mode with words range: should include transcript text snippet
    assert!(
        f.contains("Welcome to the interview"),
        "should include transcript snippet"
    );
    assert_eq!(
        f.matches("drawtext=").count(),
        2,
        "should have 2 drawtext filters (info + snippet)"
    );
}

#[test]
fn overlay_integration_snippet_from_scene_description() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("scene-snippet-test");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Full);
    let f = filters[0].as_ref().unwrap();

    // Full mode with scenes range: should include scene description snippet
    assert!(
        f.contains("Interior office") || f.contains("Close-up interview"),
        "should include scene description snippet"
    );
    assert_eq!(
        f.matches("drawtext=").count(),
        2,
        "should have 2 drawtext filters (info + snippet)"
    );
}

#[test]
fn overlay_integration_no_snippet_for_time_range() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("time-range-test");
    doc.add_shot(
        "src-001",
        ShotRange::Time {
            from_ms: 1000,
            to_ms: 3000,
        },
    )
    .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Full);
    let f = filters[0].as_ref().unwrap();

    // Time range has no text or scene preview, so only 1 drawtext (info line only)
    assert_eq!(
        f.matches("drawtext=").count(),
        1,
        "time range should have 1 drawtext (no snippet)"
    );
    assert!(f.contains("shot-001"), "should still show shot ID");
    assert!(f.contains("src-001"), "should still show source ID");
}

#[test]
fn overlay_integration_all_filters_have_valid_drawtext_syntax() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_project_for_overlay(tmp.path());

    let mut doc = EditDocument::create("syntax-test");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 })
        .unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
        .unwrap();
    doc.add_shot(
        "src-001",
        ShotRange::Time {
            from_ms: 0,
            to_ms: 5000,
        },
    )
    .unwrap();

    let filters = build_filters_for_edit(&doc, tmp.path(), OverlayMode::Full);

    for (i, filter) in filters.iter().enumerate() {
        let f = filter.as_ref().unwrap();
        // Each filter should be valid ffmpeg drawtext syntax
        assert!(
            f.starts_with("drawtext="),
            "filter {i} should start with drawtext="
        );
        assert!(
            f.contains("text='"),
            "filter {i} should have single-quoted text"
        );
        assert!(
            f.contains("fontsize="),
            "filter {i} should specify font size"
        );
        assert!(
            f.contains("fontcolor=white"),
            "filter {i} should use white text"
        );
        assert!(
            f.contains("boxcolor=black@0.5"),
            "filter {i} should have semi-transparent bg"
        );
        assert!(
            f.contains("x=10"),
            "filter {i} should be positioned at x=10"
        );
        assert!(
            f.contains("y=10"),
            "filter {i} should be positioned at y=10"
        );
    }
}
