//! TEST-049: Render progress
//!
//! Verifies the data formatting used by the render progress bar in the TUI
//! status bar: time formatting, progress fraction calculations, and shot
//! count tracking across a multi-shot edit timeline.
//! Also tests ffmpeg stderr parsing and progress computation (REQ-027).

use std::time::Instant;

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::ShotRange;
use ar_edit_core::render;

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
        author: String::new(),
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

// ---------------------------------------------------------------------------
// Tests: ffmpeg stderr progress parsing (REQ-027)
// ---------------------------------------------------------------------------

#[test]
fn parse_ffmpeg_progress_typical_line() {
    let line = "frame=  120 fps= 30 q=28.0 size=    1024kB time=00:00:04.00 bitrate= 2048.0kbits/s speed=1.50x";
    let p = render::parse_ffmpeg_progress(line).unwrap();
    assert_eq!(p.frame, Some(120));
    assert!((p.time_secs.unwrap() - 4.0).abs() < 0.01);
    assert!((p.speed.unwrap() - 1.5).abs() < 0.01);
}

#[test]
fn parse_ffmpeg_progress_no_time_returns_none() {
    let line = "Input #0, mov,mp4,m4a,3gp from 'test.mp4':";
    assert!(render::parse_ffmpeg_progress(line).is_none());
}

#[test]
fn parse_ffmpeg_progress_zero_time() {
    let line = "frame=    0 fps=0.0 q=0.0 size=       0kB time=00:00:00.00 bitrate=N/A speed=N/A";
    let p = render::parse_ffmpeg_progress(line).unwrap();
    assert_eq!(p.frame, Some(0));
    assert!((p.time_secs.unwrap() - 0.0).abs() < 0.01);
    assert!(p.speed.is_none()); // "N/A" doesn't parse as a float
}

#[test]
fn parse_ffmpeg_progress_large_time() {
    let line = "frame= 5400 fps= 60 q=23.0 size=   50000kB time=01:30:00.00 bitrate= 1234.0kbits/s speed=2.00x";
    let p = render::parse_ffmpeg_progress(line).unwrap();
    assert_eq!(p.frame, Some(5400));
    assert!((p.time_secs.unwrap() - 5400.0).abs() < 0.01);
    assert!((p.speed.unwrap() - 2.0).abs() < 0.01);
}

#[test]
fn parse_ffmpeg_progress_fractional_speed() {
    let line = "frame=   10 fps=5.0 q=20.0 size=     128kB time=00:00:02.50 bitrate= 512.0kbits/s speed=0.83x";
    let p = render::parse_ffmpeg_progress(line).unwrap();
    assert!((p.speed.unwrap() - 0.83).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// Tests: compute_progress (REQ-027)
// ---------------------------------------------------------------------------

#[test]
fn compute_progress_first_shot_no_ffmpeg() {
    let durations = vec![5000, 10000, 3000];
    let start = Instant::now();
    let rp = render::compute_progress(0, 3, &durations, None, start, "shot-001");
    assert_eq!(rp.shot_index, 1);
    assert_eq!(rp.shot_count, 3);
    assert_eq!(rp.current_shot, "shot-001");
    assert!(rp.progress < 0.01); // At the very start
}

#[test]
fn compute_progress_second_shot_no_ffmpeg() {
    let durations = vec![5000, 10000, 3000]; // total = 18000
    let start = Instant::now();
    let rp = render::compute_progress(1, 3, &durations, None, start, "shot-002");
    // completed_ms = 5000, within_shot_ms = 0 → rendered_ms = 5000 / 18000 ≈ 0.278
    assert!((rp.progress - 5000.0 / 18000.0).abs() < 0.01);
    assert_eq!(rp.shot_index, 2);
}

#[test]
fn compute_progress_with_ffmpeg_time() {
    let durations = vec![10000, 10000]; // total = 20000
    let start = Instant::now();
    let ffp = render::FfmpegProgress {
        frame: Some(150),
        time_secs: Some(5.0), // 5000ms into a 10000ms shot
        speed: Some(1.0),
    };
    let rp = render::compute_progress(0, 2, &durations, Some(&ffp), start, "shot-001");
    // rendered_ms = 0 + 5000 = 5000, total = 20000 → 0.25
    assert!((rp.progress - 0.25).abs() < 0.01);
}

#[test]
fn compute_progress_within_shot_clamped_to_duration() {
    let durations = vec![3000]; // total = 3000
    let start = Instant::now();
    let ffp = render::FfmpegProgress {
        frame: Some(200),
        time_secs: Some(10.0), // 10000ms > 3000ms shot duration
        speed: Some(2.0),
    };
    let rp = render::compute_progress(0, 1, &durations, Some(&ffp), start, "shot-001");
    // Should be clamped to 3000/3000 = 1.0
    assert!((rp.progress - 1.0).abs() < 0.01);
}

#[test]
fn compute_progress_all_shots_done() {
    let durations = vec![5000, 5000];
    let start = Instant::now();
    let ffp = render::FfmpegProgress {
        frame: Some(300),
        time_secs: Some(5.0),
        speed: Some(1.5),
    };
    // shot_idx = 1 (last shot), ffmpeg reports 5s = full duration
    let rp = render::compute_progress(1, 2, &durations, Some(&ffp), start, "shot-002");
    // completed = 5000, within = min(5000, 5000) = 5000, total = 10000 → 1.0
    assert!((rp.progress - 1.0).abs() < 0.01);
}

#[test]
fn render_progress_serializes_to_json() {
    let rp = render::RenderProgress {
        progress: 0.45,
        current_shot: "shot-003".into(),
        eta_seconds: Some(12),
        shot_index: 3,
        shot_count: 5,
    };
    let json = serde_json::to_value(&rp).unwrap();
    assert_eq!(json["progress"], 0.45);
    assert_eq!(json["current_shot"], "shot-003");
    assert_eq!(json["eta_seconds"], 12);
    assert_eq!(json["shot_index"], 3);
    assert_eq!(json["shot_count"], 5);
}

#[test]
fn render_progress_json_matches_con_007_format() {
    // CON-007 specifies: { "progress": 0.45, "current_shot": "shot-003", "eta_seconds": 12 }
    let rp = render::RenderProgress {
        progress: 0.45,
        current_shot: "shot-003".into(),
        eta_seconds: Some(12),
        shot_index: 3,
        shot_count: 5,
    };
    let json = serde_json::to_value(&rp).unwrap();
    // Must contain the three fields specified by CON-007
    assert!(json.get("progress").is_some());
    assert!(json.get("current_shot").is_some());
    assert!(json.get("eta_seconds").is_some());
}
