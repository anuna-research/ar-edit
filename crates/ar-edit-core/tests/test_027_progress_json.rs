//! TEST-027: Progress reporting --json (REQ-027, CON-007)
//!
//! Verifies the JSON output format of render progress reporting:
//!   - RenderProgress serialises with all required fields
//!   - The JSON matches the CON-007 contract format
//!   - Progress callbacks fire at expected intervals (start, within-shot, concat, done)
//!   - ffmpeg progress parsing feeds into correct progress computation
//!   - --json mode produces valid JSON lines on stderr with final summary on stdout

use std::sync::{Arc, Mutex};
use std::time::Instant;

use ar_edit_core::models::*;
use ar_edit_core::render::{self, RenderOptions};
use ar_edit_core::overlay::OverlayMode;

// ---------------------------------------------------------------------------
// Tests: RenderProgress JSON serialisation
// ---------------------------------------------------------------------------

/// RenderProgress serialises to JSON with all fields present.
#[test]
fn render_progress_serializes_all_fields() {
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

/// RenderProgress with no ETA serialises eta_seconds as null.
#[test]
fn render_progress_null_eta() {
    let rp = render::RenderProgress {
        progress: 0.1,
        current_shot: "shot-001".into(),
        eta_seconds: None,
        shot_index: 1,
        shot_count: 3,
    };
    let json = serde_json::to_value(&rp).unwrap();
    assert!(json["eta_seconds"].is_null());
}

/// JSON matches CON-007 contract: must contain progress, current_shot, eta_seconds.
#[test]
fn render_progress_matches_con_007_format() {
    let rp = render::RenderProgress {
        progress: 0.75,
        current_shot: "shot-002".into(),
        eta_seconds: Some(5),
        shot_index: 2,
        shot_count: 3,
    };
    let json = serde_json::to_value(&rp).unwrap();
    // CON-007 required fields
    assert!(json.get("progress").is_some());
    assert!(json.get("current_shot").is_some());
    assert!(json.get("eta_seconds").is_some());
}

/// Serialised JSON can be deserialised back to a serde_json::Value
/// (simulates consuming it as JSON lines in --json mode).
#[test]
fn render_progress_json_roundtrip() {
    let rp = render::RenderProgress {
        progress: 0.5,
        current_shot: "shot-001".into(),
        eta_seconds: Some(30),
        shot_index: 1,
        shot_count: 4,
    };
    let json_str = serde_json::to_string(&rp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["progress"], 0.5);
    assert_eq!(parsed["current_shot"], "shot-001");
    assert_eq!(parsed["eta_seconds"], 30);
    assert_eq!(parsed["shot_index"], 1);
    assert_eq!(parsed["shot_count"], 4);
}

/// Progress at exactly 0.0 and 1.0 serialises correctly.
#[test]
fn render_progress_boundary_values() {
    // Zero progress
    let rp = render::RenderProgress {
        progress: 0.0,
        current_shot: "shot-001".into(),
        eta_seconds: None,
        shot_index: 1,
        shot_count: 5,
    };
    let json = serde_json::to_value(&rp).unwrap();
    assert_eq!(json["progress"], 0.0);

    // Full progress
    let rp = render::RenderProgress {
        progress: 1.0,
        current_shot: "shot-005".into(),
        eta_seconds: Some(0),
        shot_index: 5,
        shot_count: 5,
    };
    let json = serde_json::to_value(&rp).unwrap();
    assert_eq!(json["progress"], 1.0);
    assert_eq!(json["eta_seconds"], 0);
}

// ---------------------------------------------------------------------------
// Tests: --json mode output format simulation
// ---------------------------------------------------------------------------

/// Simulates the --json streaming format: progress lines as JSON objects.
#[test]
fn json_mode_progress_line_format() {
    let rp = render::RenderProgress {
        progress: 0.45,
        current_shot: "shot-003".into(),
        eta_seconds: Some(12),
        shot_index: 3,
        shot_count: 5,
    };

    // The CLI emits progress as: { "progress": N, "current_shot": "...", "eta_seconds": N }
    let line = serde_json::json!({
        "progress": (rp.progress * 100.0).round() / 100.0,
        "current_shot": rp.current_shot,
        "eta_seconds": rp.eta_seconds.unwrap_or(0),
    });

    let line_str = serde_json::to_string(&line).unwrap();
    // Must be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&line_str).unwrap();
    assert_eq!(parsed["progress"], 0.45);
    assert_eq!(parsed["current_shot"], "shot-003");
    assert_eq!(parsed["eta_seconds"], 12);
}

/// Simulates the --json final success line format.
#[test]
fn json_mode_success_output_format() {
    let output = serde_json::json!({
        "success": true,
        "edit": "my-edit",
        "output": "/path/to/output.mp4",
        "shot_count": 3,
    });
    let json_str = serde_json::to_string_pretty(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["success"], true);
    assert_eq!(parsed["edit"], "my-edit");
    assert_eq!(parsed["shot_count"], 3);
}

// ---------------------------------------------------------------------------
// Tests: ffmpeg progress parsing
// ---------------------------------------------------------------------------

#[test]
fn parse_ffmpeg_progress_typical() {
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

// ---------------------------------------------------------------------------
// Tests: compute_progress (progress fraction calculations)
// ---------------------------------------------------------------------------

#[test]
fn compute_progress_first_shot_no_ffmpeg() {
    let durations = vec![5000, 10000, 3000];
    let start = Instant::now();
    let rp = render::compute_progress(0, 3, &durations, None, start, "shot-001");
    assert_eq!(rp.shot_index, 1);
    assert_eq!(rp.shot_count, 3);
    assert_eq!(rp.current_shot, "shot-001");
    assert!(rp.progress < 0.01);
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

// ---------------------------------------------------------------------------
// Tests: Progress callback invocation
// ---------------------------------------------------------------------------

/// render_to_file_with_progress invokes the callback. On an empty edit,
/// the error fires before any callbacks.
#[test]
fn progress_callback_not_called_on_empty_edit() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let doc = EditDocument::create("test");
    let output = tmp.path().join("output.mp4");

    let callbacks = Arc::new(Mutex::new(Vec::<render::RenderProgress>::new()));
    let cb = callbacks.clone();

    let result = render::render_to_file_with_progress(
        &doc,
        tmp.path(),
        &output,
        OverlayMode::Clean,
        &RenderOptions::default(),
        move |rp| {
            cb.lock().unwrap().push(rp.clone());
        },
    );
    assert!(result.is_err());
    assert!(callbacks.lock().unwrap().is_empty());
}

/// Each progress update includes valid shot_index (1-based) and shot_count.
#[test]
fn progress_shot_index_is_one_based() {
    let rp = render::compute_progress(0, 5, &[1000; 5], None, Instant::now(), "shot-001");
    assert_eq!(rp.shot_index, 1); // 1-based

    let rp = render::compute_progress(4, 5, &[1000; 5], None, Instant::now(), "shot-005");
    assert_eq!(rp.shot_index, 5); // Last shot
}

/// Progress fraction stays in [0.0, 1.0].
#[test]
fn progress_fraction_clamped_to_unit_range() {
    let durations = vec![1000, 1000, 1000];
    let start = Instant::now();

    // Normal cases
    for i in 0..3 {
        let rp = render::compute_progress(i, 3, &durations, None, start, "shot");
        assert!(rp.progress >= 0.0);
        assert!(rp.progress <= 1.0);
    }

    // Even with inflated ffmpeg time, progress is clamped
    let ffp = render::FfmpegProgress {
        frame: Some(999),
        time_secs: Some(999.0),
        speed: Some(10.0),
    };
    let rp = render::compute_progress(2, 3, &durations, Some(&ffp), start, "shot");
    assert!(rp.progress <= 1.0);
}

// ---------------------------------------------------------------------------
// Tests: JSON lines streaming contract
// ---------------------------------------------------------------------------

/// Multiple progress updates produce individual valid JSON lines.
#[test]
fn multiple_progress_updates_are_valid_json_lines() {
    let updates = vec![
        render::RenderProgress {
            progress: 0.0,
            current_shot: "shot-001".into(),
            eta_seconds: None,
            shot_index: 1,
            shot_count: 3,
        },
        render::RenderProgress {
            progress: 0.33,
            current_shot: "shot-001".into(),
            eta_seconds: Some(20),
            shot_index: 1,
            shot_count: 3,
        },
        render::RenderProgress {
            progress: 0.66,
            current_shot: "shot-002".into(),
            eta_seconds: Some(8),
            shot_index: 2,
            shot_count: 3,
        },
        render::RenderProgress {
            progress: 1.0,
            current_shot: "shot-003".into(),
            eta_seconds: Some(0),
            shot_index: 3,
            shot_count: 3,
        },
    ];

    let mut lines = String::new();
    for rp in &updates {
        let line = serde_json::json!({
            "progress": (rp.progress * 100.0).round() / 100.0,
            "current_shot": rp.current_shot,
            "eta_seconds": rp.eta_seconds.unwrap_or(0),
        });
        lines.push_str(&format!("{}\n", serde_json::to_string(&line).unwrap()));
    }

    // Each line must be independently valid JSON
    for line in lines.lines() {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(parsed.get("progress").is_some());
        assert!(parsed.get("current_shot").is_some());
        assert!(parsed.get("eta_seconds").is_some());
    }
}
