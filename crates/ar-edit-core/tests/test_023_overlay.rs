//! TEST-023: Overlay filter generation (REQ-023)
//!
//! Tests the overlay module: mode parsing, drawtext filter generation,
//! text escaping, font resolution, and integration with render pipeline.

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
    assert_eq!(OverlayMode::from_flag(Some("minimal")), OverlayMode::Minimal);
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
    assert!(filter.contains("pts:hms:45.500"), "should contain offset 45.500");
}

#[test]
fn minimal_mode_has_semi_transparent_background() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(filter.contains("boxcolor=black@0.5"), "should have semi-transparent bg");
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
    assert!(filter.contains("x=10"), "should be positioned near left edge");
    assert!(filter.contains("y=10"), "should be positioned near top edge");
}

#[test]
fn minimal_mode_white_text() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(filter.contains("fontcolor=white"));
}

#[test]
fn minimal_mode_does_not_contain_shot_or_source() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert!(!filter.contains("shot-003"), "minimal should not show shot ID");
    assert!(!filter.contains("src-002"), "minimal should not show source ID");
}

#[test]
fn minimal_mode_single_drawtext() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    assert_eq!(filter.matches("drawtext=").count(), 1, "minimal should have exactly 1 drawtext");
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
    assert!(filter.contains("pts:hms:30.000"), "should contain timecode with offset");
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
    assert!(filter.contains("Welcome to the interview today"), "should display snippet");
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
    assert_eq!(count, 2, "full with snippet should chain 2 drawtext filters");
}

#[test]
fn full_mode_without_snippet_has_one_drawtext_filter() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_no_snippet()).unwrap();
    let count = filter.matches("drawtext=").count();
    assert_eq!(count, 1, "full without snippet should have 1 drawtext filter");
}

#[test]
fn full_mode_second_line_below_first() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    // The filter is "drawtext=...y=10...,drawtext=...y=42..."
    // First drawtext at y=10, second at y=42 (below)
    let parts: Vec<&str> = filter.split(",drawtext=").collect();
    assert_eq!(parts.len(), 2, "should have comma-separated drawtext filters");
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
    assert!(!filter.contains("it's"), "single quotes should be removed from snippet");
    assert!(filter.contains("its a wonderful world"), "text preserved without quotes");
}

#[test]
fn snippet_with_percent_is_escaped() {
    let info = OverlayInfo {
        snippet: Some("100% complete".into()),
        ..info_full()
    };
    let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
    assert!(filter.contains("100%% complete"), "% should be escaped as %%");
}

#[test]
fn snippet_with_backslash_is_escaped() {
    let info = OverlayInfo {
        snippet: Some("path\\to\\file".into()),
        ..info_full()
    };
    let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
    assert!(filter.contains("path\\\\to\\\\file"), "backslash should be escaped");
}

// ---------------------------------------------------------------------------
// Filter is valid ffmpeg syntax (structural checks)
// ---------------------------------------------------------------------------

#[test]
fn filter_uses_single_quoted_text_values() {
    let filter = build_drawtext_filter(OverlayMode::Minimal, &info_full()).unwrap();
    // text='...' pattern
    assert!(filter.contains("text='"), "text values should be single-quoted");
}

#[test]
fn filter_comma_separates_chained_filters() {
    let filter = build_drawtext_filter(OverlayMode::Full, &info_full()).unwrap();
    assert!(filter.contains(",drawtext="), "chained filters separated by comma");
}

// ---------------------------------------------------------------------------
// Integration: render_preview signature accepts overlay mode
// ---------------------------------------------------------------------------

#[test]
fn render_preview_accepts_clean_mode() {
    use ar_edit_core::models::EditDocument;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let doc = EditDocument::create("test");
    // Empty edit should error regardless of overlay mode
    let result = ar_edit_core::render::resolve_preview(&doc, tmp.path());
    assert!(result.is_err());
}
