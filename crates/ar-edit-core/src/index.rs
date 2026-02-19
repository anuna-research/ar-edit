use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use thiserror::Error;

use crate::models::{Scene, Thumbnail};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("failed to parse ffmpeg output: {0}")]
    ParseFailed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect scene changes in a video file using ffmpeg's scene detection filter.
///
/// Runs `ffmpeg -i <source> -vf "select='gt(scene,<threshold>)',showinfo" -vsync vfr -f null -`
/// and parses stderr for scene-change timestamps reported by the `showinfo` filter.
///
/// Returns `Scene` structs with sequential indices and start_ms/end_ms boundaries.
/// The first scene starts at 0 ms, each subsequent scene starts at the detected
/// change point, and the final scene ends at `duration_ms`.
pub fn detect_scenes(
    source: &Path,
    duration_ms: u64,
    threshold: f64,
) -> Result<Vec<Scene>, IndexError> {
    let filter = format!("select='gt(scene,{threshold})',showinfo");

    let output = Command::new("ffmpeg")
        .args(["-i"])
        .arg(source)
        .args(["-vf", &filter, "-vsync", "vfr", "-f", "null", "-"])
        .output()
        .map_err(|e| IndexError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    let stderr = String::from_utf8_lossy(&output.stderr);

    let timestamps = parse_scene_timestamps(&stderr)?;
    let scenes = build_scenes(&timestamps, duration_ms);

    Ok(scenes)
}

/// Extract representative JPEG thumbnails at scene changes and fixed intervals.
///
/// Generates 640px-wide JPEG frames at:
/// - The start of each detected scene
/// - Fixed intervals (default 10 s, configurable via `interval_sec`)
///
/// Timestamps are merged and deduplicated. Filenames follow the pattern
/// `src-NNN_MMmSSs.jpg` inside the `thumbs_dir` directory.
///
/// Returns the list of generated thumbnails sorted by timestamp.
pub fn generate_thumbnails(
    source: &Path,
    source_id: &str,
    duration_ms: u64,
    scenes: &[Scene],
    interval_sec: u32,
    thumbs_dir: &Path,
) -> Result<Vec<Thumbnail>, IndexError> {
    let timestamps = collect_timestamps(scenes, duration_ms, interval_sec);

    let mut thumbnails = Vec::new();
    for ts_ms in &timestamps {
        let filename = format_thumbnail_filename(source_id, *ts_ms);
        let out_path = thumbs_dir.join(&filename);
        let rel_path = PathBuf::from("thumbnails").join(&filename);

        extract_frame(source, *ts_ms, &out_path)?;

        thumbnails.push(Thumbnail {
            path: rel_path,
            timestamp_ms: *ts_ms,
            description: None,
        });
    }

    Ok(thumbnails)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse scene-change timestamps (in ms) from ffmpeg showinfo output.
///
/// The `showinfo` filter writes lines like:
/// ```text
/// [Parsed_showinfo_1 @ 0x...] n:   0 pts:    450 pts_time:18.000000 ...
/// ```
/// We extract the `pts_time` values, which are the timestamps of frames that
/// passed the scene-change threshold.
fn parse_scene_timestamps(stderr: &str) -> Result<Vec<u64>, IndexError> {
    let re = Regex::new(r"pts_time:\s*(\d+\.?\d*)")
        .map_err(|e| IndexError::ParseFailed(format!("regex error: {e}")))?;

    let mut timestamps = Vec::new();
    for cap in re.captures_iter(stderr) {
        let secs: f64 = cap[1]
            .parse()
            .map_err(|e| IndexError::ParseFailed(format!("failed to parse timestamp: {e}")))?;
        timestamps.push((secs * 1000.0) as u64);
    }

    Ok(timestamps)
}

/// Build `Scene` structs from sorted scene-change timestamps.
///
/// Scene boundaries are derived as:
/// - Scene 0: `[0, first_change)`
/// - Scene 1: `[first_change, second_change)`
/// - …
/// - Scene N: `[last_change, duration_ms)`
///
/// If no scene changes are detected, a single scene spanning the full duration
/// is returned. Returns an empty vec if `duration_ms` is 0.
fn build_scenes(timestamps: &[u64], duration_ms: u64) -> Vec<Scene> {
    if duration_ms == 0 {
        return vec![];
    }

    let mut scenes = Vec::new();
    let mut start = 0u64;

    for &ts in timestamps {
        if ts > start && ts < duration_ms {
            scenes.push(Scene {
                index: scenes.len() as u32,
                start_ms: start,
                end_ms: ts,
                thumbnail: Default::default(),
                description: None,
            });
            start = ts;
        }
    }

    // Final scene from last change point (or 0) to end of video.
    scenes.push(Scene {
        index: scenes.len() as u32,
        start_ms: start,
        end_ms: duration_ms,
        thumbnail: Default::default(),
        description: None,
    });

    scenes
}

/// Collect and deduplicate thumbnail timestamps from scene boundaries
/// and fixed intervals.
fn collect_timestamps(scenes: &[Scene], duration_ms: u64, interval_sec: u32) -> Vec<u64> {
    let mut timestamps = std::collections::BTreeSet::new();

    // Scene-change timestamps (start of each scene).
    for scene in scenes {
        timestamps.insert(scene.start_ms);
    }

    // Fixed-interval timestamps.
    if interval_sec > 0 && duration_ms > 0 {
        let interval_ms = u64::from(interval_sec) * 1000;
        let mut t = 0u64;
        while t < duration_ms {
            timestamps.insert(t);
            t += interval_ms;
        }
    }

    timestamps.into_iter().collect()
}

/// Format a thumbnail filename: `src-NNN_MMmSSs.jpg`.
fn format_thumbnail_filename(source_id: &str, timestamp_ms: u64) -> String {
    let total_secs = timestamp_ms / 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{source_id}_{minutes:02}m{seconds:02}s.jpg")
}

/// Extract a single JPEG frame from `source` at `timestamp_ms`, scaled to 640px wide.
fn extract_frame(source: &Path, timestamp_ms: u64, out_path: &Path) -> Result<(), IndexError> {
    let secs = timestamp_ms as f64 / 1000.0;
    let ss = format!("{secs:.3}");

    let output = Command::new("ffmpeg")
        .args(["-y", "-ss", &ss, "-i"])
        .arg(source)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=640:-2",
            "-q:v",
            "2",
        ])
        .arg(out_path)
        .output()
        .map_err(|e| IndexError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IndexError::FfmpegFailed(format!(
            "ffmpeg frame extraction failed (ts={ss}s): {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_scene_timestamps -----------------------------------------------

    #[test]
    fn parse_timestamps_from_showinfo_output() {
        let stderr = r#"
[Parsed_showinfo_1 @ 0x600003370000] n:   0 pts:    540 pts_time:18.000000 fmt:yuv420p sar:1/1 s:1920x1080 i:P iskey:0 type:P
[Parsed_showinfo_1 @ 0x600003370000] n:   1 pts:   1350 pts_time:45.000000 fmt:yuv420p sar:1/1 s:1920x1080 i:P iskey:0 type:P
[Parsed_showinfo_1 @ 0x600003370000] n:   2 pts:   2610 pts_time:87.000000 fmt:yuv420p sar:1/1 s:1920x1080 i:P iskey:0 type:P
"#;

        let timestamps = parse_scene_timestamps(stderr).unwrap();
        assert_eq!(timestamps, vec![18000, 45000, 87000]);
    }

    #[test]
    fn parse_timestamps_empty_output() {
        let stderr = "frame=  300 fps=120 q=-0.0 Lsize=N/A time=00:00:10.00 bitrate=N/A\n";
        let timestamps = parse_scene_timestamps(stderr).unwrap();
        assert!(timestamps.is_empty());
    }

    #[test]
    fn parse_timestamps_with_fractional_seconds() {
        let stderr =
            "[Parsed_showinfo_1 @ 0x1] n: 0 pts: 90 pts_time:3.500000 fmt:yuv420p\n";

        let timestamps = parse_scene_timestamps(stderr).unwrap();
        assert_eq!(timestamps, vec![3500]);
    }

    // -- build_scenes ---------------------------------------------------------

    #[test]
    fn build_scenes_no_changes() {
        let scenes = build_scenes(&[], 120000);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].index, 0);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 120000);
    }

    #[test]
    fn build_scenes_zero_duration() {
        let scenes = build_scenes(&[5000], 0);
        assert!(scenes.is_empty());
    }

    #[test]
    fn build_scenes_multiple_changes() {
        let scenes = build_scenes(&[18000, 45000, 87000], 124500);

        assert_eq!(scenes.len(), 4);

        assert_eq!(scenes[0].index, 0);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 18000);

        assert_eq!(scenes[1].index, 1);
        assert_eq!(scenes[1].start_ms, 18000);
        assert_eq!(scenes[1].end_ms, 45000);

        assert_eq!(scenes[2].index, 2);
        assert_eq!(scenes[2].start_ms, 45000);
        assert_eq!(scenes[2].end_ms, 87000);

        assert_eq!(scenes[3].index, 3);
        assert_eq!(scenes[3].start_ms, 87000);
        assert_eq!(scenes[3].end_ms, 124500);
    }

    #[test]
    fn build_scenes_skips_change_at_zero() {
        // If ffmpeg reports a change at t=0, skip it (no zero-width scene).
        let scenes = build_scenes(&[0, 30000], 60000);

        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 30000);
        assert_eq!(scenes[1].start_ms, 30000);
        assert_eq!(scenes[1].end_ms, 60000);
    }

    #[test]
    fn build_scenes_skips_change_beyond_duration() {
        let scenes = build_scenes(&[10000, 200000], 50000);

        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].end_ms, 10000);
        assert_eq!(scenes[1].start_ms, 10000);
        assert_eq!(scenes[1].end_ms, 50000);
    }

    #[test]
    fn build_scenes_single_change() {
        let scenes = build_scenes(&[5000], 10000);

        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].index, 0);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 5000);
        assert_eq!(scenes[1].index, 1);
        assert_eq!(scenes[1].start_ms, 5000);
        assert_eq!(scenes[1].end_ms, 10000);
    }

    #[test]
    fn build_scenes_descriptions_are_none() {
        let scenes = build_scenes(&[5000], 10000);
        for scene in &scenes {
            assert_eq!(scene.description, None);
        }
    }

    #[test]
    fn build_scenes_indices_are_sequential() {
        let scenes = build_scenes(&[10000, 20000, 30000], 40000);
        for (i, scene) in scenes.iter().enumerate() {
            assert_eq!(scene.index, i as u32);
        }
    }

    // -- collect_timestamps ---------------------------------------------------

    #[test]
    fn collect_timestamps_merges_scenes_and_intervals() {
        let scenes = build_scenes(&[18000, 45000], 60000);
        let ts = collect_timestamps(&scenes, 60000, 10);

        // Interval: 0, 10000, 20000, 30000, 40000, 50000
        // Scene starts: 0, 18000, 45000
        // Merged (sorted, deduped): 0, 10000, 18000, 20000, 30000, 40000, 45000, 50000
        assert_eq!(ts, vec![0, 10000, 18000, 20000, 30000, 40000, 45000, 50000]);
    }

    #[test]
    fn collect_timestamps_no_scenes() {
        let ts = collect_timestamps(&[], 30000, 10);
        assert_eq!(ts, vec![0, 10000, 20000]);
    }

    #[test]
    fn collect_timestamps_zero_interval() {
        let scenes = build_scenes(&[5000], 10000);
        let ts = collect_timestamps(&scenes, 10000, 0);
        // Only scene starts: 0, 5000
        assert_eq!(ts, vec![0, 5000]);
    }

    #[test]
    fn collect_timestamps_zero_duration() {
        let ts = collect_timestamps(&[], 0, 10);
        assert!(ts.is_empty());
    }

    // -- format_thumbnail_filename --------------------------------------------

    #[test]
    fn format_filename_zero() {
        assert_eq!(format_thumbnail_filename("src-001", 0), "src-001_00m00s.jpg");
    }

    #[test]
    fn format_filename_seconds() {
        assert_eq!(
            format_thumbnail_filename("src-001", 18000),
            "src-001_00m18s.jpg"
        );
    }

    #[test]
    fn format_filename_minutes_and_seconds() {
        assert_eq!(
            format_thumbnail_filename("src-002", 124000),
            "src-002_02m04s.jpg"
        );
    }

    #[test]
    fn format_filename_truncates_sub_second() {
        // 18500ms → 18s (integer division)
        assert_eq!(
            format_thumbnail_filename("src-001", 18500),
            "src-001_00m18s.jpg"
        );
    }
}
