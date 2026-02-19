//! TEST-049: Render progress
//!
//! Verifies the data formatting used by the render progress bar in the TUI
//! status bar: time formatting, progress fraction calculations, and shot
//! count tracking across a multi-shot edit timeline.

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::ShotRange;

// ---------------------------------------------------------------------------
// Tests: Time formatting for progress display
// ---------------------------------------------------------------------------

#[test]
fn format_time_zero() {
    assert_eq!(display::format_time(0), "00:00.000");
}

#[test]
fn format_time_one_second() {
    assert_eq!(display::format_time(1000), "00:01.000");
}

#[test]
fn format_time_fractional_seconds() {
    assert_eq!(display::format_time(1500), "00:01.500");
}

#[test]
fn format_time_one_minute() {
    assert_eq!(display::format_time(60000), "01:00.000");
}

#[test]
fn format_time_complex() {
    assert_eq!(display::format_time(93456), "01:33.456");
}

#[test]
fn format_time_large_duration() {
    // 10 minutes, 30 seconds, 250ms
    assert_eq!(display::format_time(630250), "10:30.250");
}

// ---------------------------------------------------------------------------
// Tests: Shot count and total duration calculation
// ---------------------------------------------------------------------------

fn make_resolved_shot(id: &str, duration_ms: u64) -> ResolvedShot {
    ResolvedShot {
        id: id.into(),
        source: "src-001".into(),
        range: ShotRange::Time {
            from_ms: 0,
            to_ms: duration_ms,
        },
        start_ms: 0,
        end_ms: duration_ms,
        duration_ms,
        text_preview: None,
        scene_preview: None,
        notes: vec![],
    }
}

/// Total duration is the sum of all shot durations.
#[test]
fn total_duration_is_sum_of_shots() {
    let shots = vec![
        make_resolved_shot("shot-001", 5000),
        make_resolved_shot("shot-002", 10000),
        make_resolved_shot("shot-003", 3000),
    ];

    let total_ms: u64 = shots.iter().map(|s| s.duration_ms).sum();
    assert_eq!(total_ms, 18000);
    assert_eq!(display::format_time(total_ms), "00:18.000");
}

/// Position is calculated from shots before the selected index.
#[test]
fn position_at_first_shot() {
    let shots = vec![
        make_resolved_shot("shot-001", 5000),
        make_resolved_shot("shot-002", 10000),
    ];

    let idx = 0;
    let position_ms: u64 = shots.iter().take(idx).map(|s| s.duration_ms).sum();
    assert_eq!(position_ms, 0);
}

#[test]
fn position_at_second_shot() {
    let shots = vec![
        make_resolved_shot("shot-001", 5000),
        make_resolved_shot("shot-002", 10000),
        make_resolved_shot("shot-003", 3000),
    ];

    let idx = 1;
    let position_ms: u64 = shots.iter().take(idx).map(|s| s.duration_ms).sum();
    assert_eq!(position_ms, 5000);
}

#[test]
fn position_at_last_shot() {
    let shots = vec![
        make_resolved_shot("shot-001", 5000),
        make_resolved_shot("shot-002", 10000),
        make_resolved_shot("shot-003", 3000),
    ];

    let idx = 2;
    let position_ms: u64 = shots.iter().take(idx).map(|s| s.duration_ms).sum();
    assert_eq!(position_ms, 15000);
}

// ---------------------------------------------------------------------------
// Tests: Progress fraction calculations
// ---------------------------------------------------------------------------

/// Progress fraction for 0 of 5 shots processed.
#[test]
fn progress_fraction_zero() {
    let fraction = 0.0_f64;
    let pct = (fraction * 100.0).round() as u32;
    assert_eq!(pct, 0);
}

/// Progress fraction for 3 of 5 shots processed.
#[test]
fn progress_fraction_partial() {
    let current = 3;
    let total = 5;
    let fraction = current as f64 / total as f64;
    let pct = (fraction * 100.0).round() as u32;
    assert_eq!(pct, 60);
}

/// Progress fraction for 5 of 5 shots processed (100%).
#[test]
fn progress_fraction_complete() {
    let fraction = 1.0_f64;
    let pct = (fraction * 100.0).round() as u32;
    assert_eq!(pct, 100);
}

/// Progress bar width calculation at different terminal widths.
#[test]
fn progress_bar_width_narrow() {
    let width: u16 = 20;
    let bar_width = ((width as usize) / 3).clamp(8, 30);
    assert_eq!(bar_width, 8); // Minimum clamp
}

#[test]
fn progress_bar_width_medium() {
    let width: u16 = 80;
    let bar_width = ((width as usize) / 3).clamp(8, 30);
    assert_eq!(bar_width, 26);
}

#[test]
fn progress_bar_width_wide() {
    let width: u16 = 200;
    let bar_width = ((width as usize) / 3).clamp(8, 30);
    assert_eq!(bar_width, 30); // Maximum clamp
}

/// Filled/empty block counts match the fraction.
#[test]
fn progress_bar_fill_calculation() {
    let bar_width = 20;
    let fraction = 0.5;
    let filled = ((fraction * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;
    assert_eq!(filled, 10);
    assert_eq!(empty, 10);
}

#[test]
fn progress_bar_fill_zero() {
    let bar_width = 20;
    let fraction = 0.0;
    let filled = ((fraction * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;
    assert_eq!(filled, 0);
    assert_eq!(empty, 20);
}

#[test]
fn progress_bar_fill_full() {
    let bar_width = 20;
    let fraction = 1.0;
    let filled = ((fraction * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;
    assert_eq!(filled, 20);
    assert_eq!(empty, 0);
}

// ---------------------------------------------------------------------------
// Tests: ETA formatting
// ---------------------------------------------------------------------------

/// ETA displays as MM:SS.
#[test]
fn eta_seconds() {
    let eta_secs: u64 = 45;
    let mins = eta_secs / 60;
    let secs = eta_secs % 60;
    let formatted = format!("ETA {mins:02}:{secs:02}");
    assert_eq!(formatted, "ETA 00:45");
}

#[test]
fn eta_minutes() {
    let eta_secs: u64 = 130;
    let mins = eta_secs / 60;
    let secs = eta_secs % 60;
    let formatted = format!("ETA {mins:02}:{secs:02}");
    assert_eq!(formatted, "ETA 02:10");
}

#[test]
fn eta_zero() {
    let eta_secs: u64 = 0;
    let mins = eta_secs / 60;
    let secs = eta_secs % 60;
    let formatted = format!("ETA {mins:02}:{secs:02}");
    assert_eq!(formatted, "ETA 00:00");
}

// ---------------------------------------------------------------------------
// Tests: Shot position display ("shot X/Y")
// ---------------------------------------------------------------------------

/// Shot position format at various indices.
#[test]
fn shot_position_display_first() {
    let idx = 0;
    let total = 5;
    let display = format!("shot {}/{}", idx + 1, total);
    assert_eq!(display, "shot 1/5");
}

#[test]
fn shot_position_display_middle() {
    let idx = 2;
    let total = 5;
    let display = format!("shot {}/{}", idx + 1, total);
    assert_eq!(display, "shot 3/5");
}

#[test]
fn shot_position_display_last() {
    let idx = 4;
    let total = 5;
    let display = format!("shot {}/{}", idx + 1, total);
    assert_eq!(display, "shot 5/5");
}

/// Single-shot timeline shows "shot 1/1".
#[test]
fn shot_position_single_shot() {
    let idx = 0;
    let total = 1;
    let display = format!("shot {}/{}", idx + 1, total);
    assert_eq!(display, "shot 1/1");
}
