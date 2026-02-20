use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use regex::Regex;
use thiserror::Error;

use crate::models::{Scene, Source, SourceIndex, SourceMetadata, Thumbnail};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("failed to parse ffmpeg output: {0}")]
    ParseFailed(String),
    #[error("source not indexed: {0}")]
    NotIndexed(String),
    #[error("scene index {index} out of range (source has {count} scenes)")]
    SceneOutOfRange { index: u32, count: u32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse index JSON: {0}")]
    Json(#[from] serde_json::Error),
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

/// Build a complete SourceIndex for a source: detect scenes, extract thumbnails,
/// and write the index file to `index/<source_id>.index.json`.
///
/// Scene thumbnails are assigned by matching each scene's `start_ms` to the
/// corresponding thumbnail path. The source file size is read from disk for
/// the metadata block.
pub fn build_source_index(
    project_dir: &Path,
    source: &Source,
    threshold: f64,
    interval_sec: u32,
) -> Result<SourceIndex, IndexError> {
    let source_path = project_dir.join(&source.path);
    let thumbs_dir = project_dir.join("thumbnails");

    let file_size_bytes = std::fs::metadata(&source_path)?.len();

    let mut scenes = detect_scenes(&source_path, source.duration_ms, threshold)?;

    let thumbnails = generate_thumbnails(
        &source_path,
        &source.id,
        source.duration_ms,
        &scenes,
        interval_sec,
        &thumbs_dir,
    )?;

    // Map timestamp → thumbnail path so we can assign each scene its thumbnail.
    let thumb_map: HashMap<u64, &PathBuf> =
        thumbnails.iter().map(|t| (t.timestamp_ms, &t.path)).collect();

    for scene in &mut scenes {
        if let Some(path) = thumb_map.get(&scene.start_ms) {
            scene.thumbnail = (*path).clone();
        }
    }

    let scene_count = scenes.len() as u32;

    let index = SourceIndex {
        source_id: source.id.clone(),
        indexed_at: Utc::now(),
        metadata: SourceMetadata {
            duration_ms: source.duration_ms,
            resolution: source.resolution,
            codec: source.video_codec.clone(),
            file_size_bytes,
        },
        thumbnails,
        scene_count,
        scenes,
    };

    save_index(project_dir, &index)?;

    Ok(index)
}

/// Load a previously saved SourceIndex from `index/<source_id>.index.json`.
pub fn load_index(project_dir: &Path, source_id: &str) -> Result<SourceIndex, IndexError> {
    let path = index_path(project_dir, source_id);
    if !path.exists() {
        return Err(IndexError::NotIndexed(source_id.to_string()));
    }
    let content = std::fs::read_to_string(&path)?;
    let index: SourceIndex = serde_json::from_str(&content)?;
    Ok(index)
}

/// Write a SourceIndex to `index/<source_id>.index.json`.
pub fn save_index(project_dir: &Path, index: &SourceIndex) -> Result<(), IndexError> {
    let path = index_path(project_dir, &index.source_id);
    let json = serde_json::to_string_pretty(index)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Set the description for a scene in an existing index.
///
/// Loads the index, updates the scene description, and saves it back.
/// Returns a tuple of (updated SourceIndex, old description).
/// The old value is preserved for audit logging (append-only semantics).
pub fn set_scene_description(
    project_dir: &Path,
    source_id: &str,
    scene_index: u32,
    text: &str,
) -> Result<(SourceIndex, Option<String>), IndexError> {
    let mut index = load_index(project_dir, source_id)?;

    if scene_index >= index.scene_count {
        return Err(IndexError::SceneOutOfRange {
            index: scene_index,
            count: index.scene_count,
        });
    }

    let old_description = index.scenes[scene_index as usize].description.clone();
    index.scenes[scene_index as usize].description = Some(text.to_string());
    save_index(project_dir, &index)?;

    Ok((index, old_description))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// On-disk path for an index file.
fn index_path(project_dir: &Path, source_id: &str) -> PathBuf {
    project_dir
        .join("index")
        .join(format!("{source_id}.index.json"))
}

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

/// Minimum scene duration in milliseconds. Scenes shorter than this are merged
/// into the previous scene to avoid zero-duration or near-zero-duration entries.
const MIN_SCENE_DURATION_MS: u64 = 500;

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
///
/// Scenes shorter than `MIN_SCENE_DURATION_MS` (500 ms) are merged into the
/// previous scene by extending its `end_ms`.
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

    // Merge sub-threshold scenes into the previous scene.
    let mut merged: Vec<Scene> = Vec::with_capacity(scenes.len());
    for scene in scenes {
        let duration = scene.end_ms - scene.start_ms;
        if duration < MIN_SCENE_DURATION_MS && !merged.is_empty() {
            // Extend the previous scene's end to absorb this short scene.
            merged.last_mut().unwrap().end_ms = scene.end_ms;
        } else {
            merged.push(scene);
        }
    }

    // Re-index after merging.
    for (i, scene) in merged.iter_mut().enumerate() {
        scene.index = i as u32;
    }

    merged
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
    use tempfile::TempDir;

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

    #[test]
    fn build_scenes_merges_zero_duration_scene() {
        // Two timestamps at the same ms produce a zero-width scene that should be merged.
        // ts=128100 appears twice — the guard `ts > start` filters the duplicate,
        // but if two distinct timestamps are <500ms apart, the short scene is merged.
        let scenes = build_scenes(&[10000, 10200], 60000);

        // 10200 - 10000 = 200ms < 500ms, so the scene [10000, 10200) is merged
        // into [0, 10200). Then [10200, 60000).
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 10200);
        assert_eq!(scenes[1].start_ms, 10200);
        assert_eq!(scenes[1].end_ms, 60000);
    }

    #[test]
    fn build_scenes_merges_sub_threshold_final_scene() {
        // Final scene is shorter than 500ms — merged into the previous one.
        let scenes = build_scenes(&[59800], 60000);

        // [59800, 60000) = 200ms < 500ms, merged into [0, 60000).
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 60000);
    }

    #[test]
    fn build_scenes_keeps_scenes_above_threshold() {
        // All scenes are >= 500ms, nothing merged.
        let scenes = build_scenes(&[5000, 10000], 60000);

        assert_eq!(scenes.len(), 3);
        assert_eq!(scenes[0].end_ms, 5000);
        assert_eq!(scenes[1].end_ms, 10000);
        assert_eq!(scenes[2].end_ms, 60000);
    }

    #[test]
    fn build_scenes_reindexes_after_merge() {
        // Multiple sub-threshold scenes get merged; indices should be sequential.
        let scenes = build_scenes(&[100, 200, 300, 50000], 60000);

        // [0,100)=100ms is first scene so kept, [100,200)=100ms merged into prev,
        // [200,300)=100ms merged into prev → [0, 300).
        // Then [300, 50000) and [50000, 60000) are above threshold.
        assert_eq!(scenes.len(), 3);
        assert_eq!(scenes[0].index, 0);
        assert_eq!(scenes[1].index, 1);
        assert_eq!(scenes[2].index, 2);
        assert_eq!(scenes[0].start_ms, 0);
        assert_eq!(scenes[0].end_ms, 300);
        assert_eq!(scenes[1].start_ms, 300);
        assert_eq!(scenes[1].end_ms, 50000);
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

    // -- index_path ------------------------------------------------------------

    #[test]
    fn index_path_format() {
        let p = index_path(Path::new("/project"), "src-001");
        assert_eq!(p, PathBuf::from("/project/index/src-001.index.json"));
    }

    // -- save_index / load_index roundtrip ------------------------------------

    #[test]
    fn save_and_load_index_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let index_dir = tmp.path().join("index");
        std::fs::create_dir(&index_dir).unwrap();

        let index = SourceIndex {
            source_id: "src-001".into(),
            indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
            metadata: SourceMetadata {
                duration_ms: 124500,
                resolution: (1920, 1080),
                codec: "h264".into(),
                file_size_bytes: 52428800,
            },
            thumbnails: vec![
                Thumbnail {
                    path: PathBuf::from("thumbnails/src-001_00m00s.jpg"),
                    timestamp_ms: 0,
                    description: None,
                },
                Thumbnail {
                    path: PathBuf::from("thumbnails/src-001_00m18s.jpg"),
                    timestamp_ms: 18000,
                    description: Some("Scene change".into()),
                },
            ],
            scene_count: 2,
            scenes: vec![
                Scene {
                    index: 0,
                    start_ms: 0,
                    end_ms: 18000,
                    thumbnail: PathBuf::from("thumbnails/src-001_00m00s.jpg"),
                    description: Some("Wide shot".into()),
                },
                Scene {
                    index: 1,
                    start_ms: 18000,
                    end_ms: 124500,
                    thumbnail: PathBuf::from("thumbnails/src-001_00m18s.jpg"),
                    description: None,
                },
            ],
        };

        save_index(tmp.path(), &index).unwrap();

        // Verify the file exists
        assert!(index_dir.join("src-001.index.json").exists());

        // Load and compare
        let loaded = load_index(tmp.path(), "src-001").unwrap();
        assert_eq!(loaded, index);
    }

    #[test]
    fn load_index_not_indexed() {
        let tmp = TempDir::new().unwrap();
        let index_dir = tmp.path().join("index");
        std::fs::create_dir(&index_dir).unwrap();

        let err = load_index(tmp.path(), "src-999").unwrap_err();
        assert!(matches!(err, IndexError::NotIndexed(ref id) if id == "src-999"));
    }

    // -- set_scene_description ------------------------------------------------

    #[test]
    fn set_scene_description_updates_scene() {
        let tmp = TempDir::new().unwrap();
        let index_dir = tmp.path().join("index");
        std::fs::create_dir(&index_dir).unwrap();

        let index = SourceIndex {
            source_id: "src-002".into(),
            indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
            metadata: SourceMetadata {
                duration_ms: 60000,
                resolution: (1280, 720),
                codec: "h264".into(),
                file_size_bytes: 10000000,
            },
            thumbnails: vec![],
            scene_count: 3,
            scenes: vec![
                Scene {
                    index: 0,
                    start_ms: 0,
                    end_ms: 20000,
                    thumbnail: Default::default(),
                    description: None,
                },
                Scene {
                    index: 1,
                    start_ms: 20000,
                    end_ms: 40000,
                    thumbnail: Default::default(),
                    description: None,
                },
                Scene {
                    index: 2,
                    start_ms: 40000,
                    end_ms: 60000,
                    thumbnail: Default::default(),
                    description: None,
                },
            ],
        };
        save_index(tmp.path(), &index).unwrap();

        let (updated, old_desc) =
            set_scene_description(tmp.path(), "src-002", 1, "Close-up interview").unwrap();
        assert_eq!(old_desc, None);
        assert_eq!(
            updated.scenes[1].description.as_deref(),
            Some("Close-up interview")
        );
        // Other scenes unchanged
        assert_eq!(updated.scenes[0].description, None);
        assert_eq!(updated.scenes[2].description, None);

        // Persisted to disk
        let reloaded = load_index(tmp.path(), "src-002").unwrap();
        assert_eq!(
            reloaded.scenes[1].description.as_deref(),
            Some("Close-up interview")
        );

        // Overwrite returns old value
        let (updated2, old_desc2) =
            set_scene_description(tmp.path(), "src-002", 1, "Wide shot exterior").unwrap();
        assert_eq!(old_desc2.as_deref(), Some("Close-up interview"));
        assert_eq!(
            updated2.scenes[1].description.as_deref(),
            Some("Wide shot exterior")
        );
    }

    #[test]
    fn set_scene_description_out_of_range() {
        let tmp = TempDir::new().unwrap();
        let index_dir = tmp.path().join("index");
        std::fs::create_dir(&index_dir).unwrap();

        let index = SourceIndex {
            source_id: "src-003".into(),
            indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
            metadata: SourceMetadata {
                duration_ms: 30000,
                resolution: (1920, 1080),
                codec: "h264".into(),
                file_size_bytes: 5000000,
            },
            thumbnails: vec![],
            scene_count: 2,
            scenes: vec![
                Scene {
                    index: 0,
                    start_ms: 0,
                    end_ms: 15000,
                    thumbnail: Default::default(),
                    description: None,
                },
                Scene {
                    index: 1,
                    start_ms: 15000,
                    end_ms: 30000,
                    thumbnail: Default::default(),
                    description: None,
                },
            ],
        };
        save_index(tmp.path(), &index).unwrap();

        let err = set_scene_description(tmp.path(), "src-003", 5, "nope").unwrap_err();
        assert!(matches!(
            err,
            IndexError::SceneOutOfRange {
                index: 5,
                count: 2
            }
        ));
    }

    #[test]
    fn set_scene_description_not_indexed() {
        let tmp = TempDir::new().unwrap();
        let index_dir = tmp.path().join("index");
        std::fs::create_dir(&index_dir).unwrap();

        let err = set_scene_description(tmp.path(), "src-999", 0, "text").unwrap_err();
        assert!(matches!(err, IndexError::NotIndexed(_)));
    }
}
