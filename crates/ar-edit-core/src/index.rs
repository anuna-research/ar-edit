use std::path::Path;
use std::process::Command;

use regex::Regex;
use thiserror::Error;

use crate::models::Scene;

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
}
