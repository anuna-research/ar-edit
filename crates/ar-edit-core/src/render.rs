use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use regex::Regex;
use serde::Serialize;
use thiserror::Error;

use crate::display::{self, ResolvedShot};
use crate::models::EditDocument;
use crate::overlay::{self, OverlayInfo, OverlayMode};
use crate::playback;
use crate::project;
use crate::subtitles;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("edit has no shots")]
    EmptyEdit,
    #[error("invalid resolution format: {0} (expected WxH, e.g. 1920x1080)")]
    InvalidResolution(String),
    #[error(transparent)]
    Display(#[from] display::DisplayError),
    #[error(transparent)]
    Playback(#[from] playback::PlaybackError),
    #[error(transparent)]
    Project(#[from] project::ProjectError),
    #[error(transparent)]
    Subtitle(#[from] subtitles::SubtitleError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Render options (REQ-026)
// ---------------------------------------------------------------------------

/// Options controlling the output format of a render.
///
/// When `video_codec` or `resolution` is `None`, the render pipeline uses
/// stream copy when possible (all sources match) or defaults to H.264/AAC
/// when re-encoding is needed.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Target video codec (e.g. "h264", "h265"). `None` means "match sources
    /// or default to h264".
    pub video_codec: Option<String>,
    /// Target output resolution `(width, height)`. `None` means "use highest
    /// input resolution".
    pub resolution: Option<(u32, u32)>,
    /// Embed SRT subtitles generated from transcripts (REQ-036).
    pub subtitles: bool,
}

// ---------------------------------------------------------------------------
// Progress reporting (REQ-027)
// ---------------------------------------------------------------------------

/// Progress update emitted during rendering.
#[derive(Debug, Clone, Serialize)]
pub struct RenderProgress {
    /// Overall progress fraction (0.0 to 1.0).
    pub progress: f64,
    /// ID of the shot currently being rendered.
    pub current_shot: String,
    /// Estimated seconds remaining, if calculable.
    pub eta_seconds: Option<u64>,
    /// 1-based index of the current shot.
    pub shot_index: usize,
    /// Total number of shots.
    pub shot_count: usize,
}

/// Progress parsed from a single ffmpeg stderr progress line.
#[derive(Debug, Clone, Default)]
pub struct FfmpegProgress {
    /// Frame count reported by ffmpeg.
    pub frame: Option<u64>,
    /// Current time in seconds parsed from `time=HH:MM:SS.ss`.
    pub time_secs: Option<f64>,
    /// Encoding speed multiplier (e.g. 1.5x).
    pub speed: Option<f64>,
}

/// Parse an ffmpeg stderr line for progress information.
///
/// ffmpeg progress lines look like:
/// `frame=  120 fps= 30 q=28.0 size=    1024kB time=00:00:04.00 bitrate= 2048.0kbits/s speed=1.50x`
pub fn parse_ffmpeg_progress(line: &str) -> Option<FfmpegProgress> {
    // Must contain "time=" to be a progress line
    if !line.contains("time=") {
        return None;
    }

    let mut progress = FfmpegProgress::default();

    // Parse frame=NNN
    if let Some(caps) = Regex::new(r"frame=\s*(\d+)").ok()?.captures(line) {
        progress.frame = caps.get(1)?.as_str().parse().ok();
    }

    // Parse time=HH:MM:SS.ss
    if let Some(caps) = Regex::new(r"time=(\d+):(\d+):(\d+\.\d+)")
        .ok()?
        .captures(line)
    {
        let h: f64 = caps.get(1)?.as_str().parse().ok()?;
        let m: f64 = caps.get(2)?.as_str().parse().ok()?;
        let s: f64 = caps.get(3)?.as_str().parse().ok()?;
        progress.time_secs = Some(h * 3600.0 + m * 60.0 + s);
    }

    // Parse speed=N.NNx
    if let Some(caps) = Regex::new(r"speed=\s*([\d.]+)x").ok()?.captures(line) {
        progress.speed = caps.get(1)?.as_str().parse().ok();
    }

    Some(progress)
}

/// Compute a [`RenderProgress`] from per-shot timing and ffmpeg progress.
///
/// - `shot_idx`: 0-based index of the current shot being rendered
/// - `shot_count`: total number of shots
/// - `shot_durations_ms`: duration of each shot in milliseconds
/// - `ffmpeg_progress`: latest progress from ffmpeg stderr (for within-shot progress)
/// - `start_time`: wall-clock instant when rendering began
pub fn compute_progress(
    shot_idx: usize,
    shot_count: usize,
    shot_durations_ms: &[u64],
    ffmpeg_progress: Option<&FfmpegProgress>,
    start_time: Instant,
    current_shot_id: &str,
) -> RenderProgress {
    let total_duration_ms: u64 = shot_durations_ms.iter().sum();
    let completed_ms: u64 = shot_durations_ms.iter().take(shot_idx).sum();

    // Within-shot progress from ffmpeg time
    let current_shot_duration_ms = shot_durations_ms.get(shot_idx).copied().unwrap_or(0);
    let within_shot_ms = ffmpeg_progress
        .and_then(|p| p.time_secs)
        .map(|t| (t * 1000.0) as u64)
        .unwrap_or(0)
        .min(current_shot_duration_ms);

    let rendered_ms = completed_ms + within_shot_ms;
    let progress = if total_duration_ms > 0 {
        (rendered_ms as f64 / total_duration_ms as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // ETA from wall-clock elapsed
    let elapsed = start_time.elapsed();
    let eta_seconds = if progress > 0.01 {
        let total_estimated = elapsed.as_secs_f64() / progress;
        let remaining = total_estimated - elapsed.as_secs_f64();
        Some(remaining.max(0.0).ceil() as u64)
    } else {
        None
    };

    RenderProgress {
        progress,
        current_shot: current_shot_id.to_string(),
        eta_seconds,
        shot_index: shot_idx + 1,
        shot_count,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return the path where a preview render for the given edit name would be stored.
///
/// The preview lives in a temp directory named after the edit so it persists
/// across invocations and can be reused when the edit hasn't changed.
pub fn preview_output_path(edit_name: &str) -> PathBuf {
    let preview_dir = std::env::temp_dir().join(format!("ar-edit-preview-{edit_name}"));
    preview_dir.join("preview.mp4")
}

/// Render a full edit preview by extracting and concatenating all shots via ffmpeg.
///
/// Steps:
///   1. Resolve all shots to absolute timestamps via `display::resolve_edit`
///   2. Extract each shot segment from its source video
///      - `Clean` mode: stream copy (`-c copy`) for speed
///      - `Full`/`Minimal` mode: re-encode with drawtext overlay filter (REQ-023)
///   3. Write a concat demuxer file listing all segments
///   4. Concatenate all segments into a single preview file (`ffmpeg -f concat`)
///
/// The preview file is written to a temporary directory and its path is returned.
/// Callers are responsible for cleaning up the temp directory if desired.
pub fn render_preview(
    doc: &EditDocument,
    project_dir: &Path,
    overlay_mode: OverlayMode,
    options: &RenderOptions,
) -> Result<PathBuf, RenderError> {
    let output_path = preview_output_path(&doc.name);
    if let Some(preview_dir) = output_path.parent() {
        std::fs::create_dir_all(preview_dir)?;
    }
    render_to_file(doc, project_dir, &output_path, overlay_mode, options)?;

    Ok(output_path)
}

/// Render an edit document to a specific output file using the ffmpeg concat demuxer.
///
/// This is the core concat pipeline (REQ-025, REQ-026):
///   1. Resolve all shots to absolute timestamps via `display::resolve_edit`
///   2. For each shot, decide stream copy vs re-encode based on codec/resolution match
///   3. Extract each segment (stream copy or re-encoded to target codec/resolution)
///   4. Generate a `filelist.txt` listing all segments in edit-snapshot order
///   5. Run `ffmpeg -f concat -safe 0 -i filelist.txt -c copy <output>`
///
/// Segment ordering matches the edit snapshot exactly — shots appear in the
/// concat file in the same order as `doc.snapshot.shots`.
pub fn render_to_file(
    doc: &EditDocument,
    project_dir: &Path,
    output: &Path,
    overlay_mode: OverlayMode,
    options: &RenderOptions,
) -> Result<(), RenderError> {
    let resolved = display::resolve_edit(doc, project_dir)?;
    if resolved.is_empty() {
        return Err(RenderError::EmptyEdit);
    }

    // Use a work directory next to the output file for intermediate segments
    let work_dir = output.parent().unwrap_or(Path::new(".")).join(format!(
        ".ar-edit-render-{}",
        output.file_stem().and_then(|s| s.to_str()).unwrap_or("out")
    ));
    std::fs::create_dir_all(&work_dir)?;

    let result = render_segments_and_concat(
        &resolved,
        doc,
        project_dir,
        &work_dir,
        output,
        overlay_mode,
        options,
    );

    // Clean up work directory regardless of success/failure
    let _ = std::fs::remove_dir_all(&work_dir);

    result?;

    // Embed subtitles if requested (REQ-036)
    if options.subtitles {
        embed_subtitles_into_output(&resolved, project_dir, output)?;
    }

    Ok(())
}

/// Render an edit document with progress reporting via a callback (REQ-027).
///
/// Same pipeline as [`render_to_file`] but invokes `on_progress` after each
/// shot extraction and as ffmpeg reports progress within each shot.
pub fn render_to_file_with_progress<F>(
    doc: &EditDocument,
    project_dir: &Path,
    output: &Path,
    overlay_mode: OverlayMode,
    options: &RenderOptions,
    on_progress: F,
) -> Result<(), RenderError>
where
    F: Fn(&RenderProgress) + Send + 'static,
{
    let resolved = display::resolve_edit(doc, project_dir)?;
    if resolved.is_empty() {
        return Err(RenderError::EmptyEdit);
    }

    let work_dir = output.parent().unwrap_or(Path::new(".")).join(format!(
        ".ar-edit-render-{}",
        output.file_stem().and_then(|s| s.to_str()).unwrap_or("out")
    ));
    std::fs::create_dir_all(&work_dir)?;

    let result = render_segments_and_concat_with_progress(
        &resolved,
        doc,
        project_dir,
        &work_dir,
        output,
        overlay_mode,
        options,
        &on_progress,
    );

    let _ = std::fs::remove_dir_all(&work_dir);

    result?;

    // Embed subtitles if requested (REQ-036)
    if options.subtitles {
        embed_subtitles_into_output(&resolved, project_dir, output)?;
    }

    Ok(())
}

/// Internal: extract segments with progress callbacks, write filelist.txt, and concatenate.
#[allow(clippy::too_many_arguments)]
fn render_segments_and_concat_with_progress<F>(
    resolved: &[ResolvedShot],
    _doc: &EditDocument,
    project_dir: &Path,
    work_dir: &Path,
    output: &Path,
    overlay_mode: OverlayMode,
    options: &RenderOptions,
    on_progress: &F,
) -> Result<(), RenderError>
where
    F: Fn(&RenderProgress),
{
    let encode_params = resolve_encode_params(resolved, project_dir, options)?;

    let shot_durations_ms: Vec<u64> = resolved.iter().map(|s| s.duration_ms).collect();
    let start_time = Instant::now();

    let mut segment_paths = Vec::with_capacity(resolved.len());
    let mut timeline_offset_ms: u64 = 0;

    for (i, shot) in resolved.iter().enumerate() {
        let (source_path, _) = playback::resolve_source_path(&shot.source, project_dir)?;
        let segment_path = work_dir.join(format!("segment_{i:04}.mp4"));

        // Emit initial progress for this shot
        let initial = compute_progress(
            i,
            resolved.len(),
            &shot_durations_ms,
            None,
            start_time,
            &shot.id,
        );
        on_progress(&initial);

        let overlay_filter = overlay::build_drawtext_filter(
            overlay_mode,
            &OverlayInfo {
                shot_id: shot.id.clone(),
                source_id: shot.source.clone(),
                snippet: shot.text_preview.clone().or(shot.scene_preview.clone()),
                timecode_offset_sec: timeline_offset_ms as f64 / 1000.0,
            },
        );

        let source_info = encode_params
            .source_info
            .iter()
            .find(|s| s.id == shot.source);

        let needs_video_reencode = match source_info {
            Some(info) => {
                let codec_mismatch = encode_params.target_codec.is_some()
                    && normalize_codec(&info.video_codec)
                        != normalize_codec(encode_params.target_codec.as_deref().unwrap());
                let res_mismatch = encode_params.target_resolution.is_some()
                    && info.resolution != encode_params.target_resolution.unwrap();
                codec_mismatch || res_mismatch
            }
            None => options.video_codec.is_some() || options.resolution.is_some(),
        };

        let has_overlay = overlay_filter.is_some();

        if has_overlay || needs_video_reencode {
            let mut filters: Vec<String> = Vec::new();

            if let Some((w, h)) = encode_params.target_resolution {
                let needs_scale = match source_info {
                    Some(info) => info.resolution != (w, h),
                    None => true,
                };
                if needs_scale {
                    filters.push(format!("scale={w}:{h}"));
                }
            }

            if let Some(ref vf) = overlay_filter {
                filters.push(vf.clone());
            }

            let combined_filter = if filters.is_empty() {
                None
            } else {
                Some(filters.join(","))
            };

            let encoder = encode_params
                .target_codec
                .as_deref()
                .map(ffmpeg_video_encoder)
                .unwrap_or("libx264");

            extract_segment_with_progress(
                &source_path,
                shot.start_ms,
                shot.end_ms,
                &segment_path,
                combined_filter.as_deref(),
                Some(encoder),
                i,
                resolved.len(),
                &shot_durations_ms,
                start_time,
                &shot.id,
                on_progress,
            )?;
        } else {
            extract_segment(&source_path, shot.start_ms, shot.end_ms, &segment_path)?;
        }

        segment_paths.push(segment_path);
        timeline_offset_ms += shot.duration_ms;
    }

    // Emit a final progress for concat phase
    let concat_progress = RenderProgress {
        progress: 0.99,
        current_shot: resolved.last().map(|s| s.id.clone()).unwrap_or_default(),
        eta_seconds: Some(0),
        shot_index: resolved.len(),
        shot_count: resolved.len(),
    };
    on_progress(&concat_progress);

    let concat_list_path = work_dir.join("filelist.txt");
    write_concat_list(&segment_paths, &concat_list_path)?;
    concat_segments(&concat_list_path, output)?;

    // Emit 100% done
    let done = RenderProgress {
        progress: 1.0,
        current_shot: resolved.last().map(|s| s.id.clone()).unwrap_or_default(),
        eta_seconds: Some(0),
        shot_index: resolved.len(),
        shot_count: resolved.len(),
    };
    on_progress(&done);

    Ok(())
}

/// Internal: extract segments, write filelist.txt, and concatenate.
fn render_segments_and_concat(
    resolved: &[ResolvedShot],
    _doc: &EditDocument,
    project_dir: &Path,
    work_dir: &Path,
    output: &Path,
    overlay_mode: OverlayMode,
    options: &RenderOptions,
) -> Result<(), RenderError> {
    // Resolve the target encoding parameters from options and source metadata
    let encode_params = resolve_encode_params(resolved, project_dir, options)?;

    let mut segment_paths = Vec::with_capacity(resolved.len());
    let mut timeline_offset_ms: u64 = 0;

    for (i, shot) in resolved.iter().enumerate() {
        let (source_path, _) = playback::resolve_source_path(&shot.source, project_dir)?;
        let segment_path = work_dir.join(format!("segment_{i:04}.mp4"));

        let overlay_filter = overlay::build_drawtext_filter(
            overlay_mode,
            &OverlayInfo {
                shot_id: shot.id.clone(),
                source_id: shot.source.clone(),
                snippet: shot.text_preview.clone().or(shot.scene_preview.clone()),
                timecode_offset_sec: timeline_offset_ms as f64 / 1000.0,
            },
        );

        // Determine per-segment encoding strategy
        let source_info = encode_params
            .source_info
            .iter()
            .find(|s| s.id == shot.source);

        let needs_video_reencode = match source_info {
            Some(info) => {
                let codec_mismatch = encode_params.target_codec.is_some()
                    && normalize_codec(&info.video_codec)
                        != normalize_codec(encode_params.target_codec.as_deref().unwrap());
                let res_mismatch = encode_params.target_resolution.is_some()
                    && info.resolution != encode_params.target_resolution.unwrap();
                codec_mismatch || res_mismatch
            }
            // No manifest info available — only re-encode if overlay or explicit options
            None => options.video_codec.is_some() || options.resolution.is_some(),
        };

        let has_overlay = overlay_filter.is_some();

        if has_overlay || needs_video_reencode {
            // Build combined video filter chain
            let mut filters: Vec<String> = Vec::new();

            // Scale filter first (before overlay)
            if let Some((w, h)) = encode_params.target_resolution {
                let needs_scale = match source_info {
                    Some(info) => info.resolution != (w, h),
                    None => true,
                };
                if needs_scale {
                    filters.push(format!("scale={w}:{h}"));
                }
            }

            // Overlay filter
            if let Some(ref vf) = overlay_filter {
                filters.push(vf.clone());
            }

            let combined_filter = if filters.is_empty() {
                None
            } else {
                Some(filters.join(","))
            };

            let encoder = encode_params
                .target_codec
                .as_deref()
                .map(ffmpeg_video_encoder)
                .unwrap_or("libx264");

            extract_segment_encoded(
                &source_path,
                shot.start_ms,
                shot.end_ms,
                &segment_path,
                combined_filter.as_deref(),
                encoder,
            )?;
        } else {
            // Stream copy — codecs and resolution match, no overlay
            extract_segment(&source_path, shot.start_ms, shot.end_ms, &segment_path)?;
        }

        segment_paths.push(segment_path);
        timeline_offset_ms += shot.duration_ms;
    }

    // Write concat demuxer filelist
    let concat_list_path = work_dir.join("filelist.txt");
    write_concat_list(&segment_paths, &concat_list_path)?;

    // Concatenate segments into final output
    concat_segments(&concat_list_path, output)?;

    Ok(())
}

/// Build a human-readable summary of what will be rendered, without executing ffmpeg.
///
/// Returns resolved shots so callers can display progress information.
pub fn resolve_preview(
    doc: &EditDocument,
    project_dir: &Path,
) -> Result<Vec<ResolvedShot>, RenderError> {
    let resolved = display::resolve_edit(doc, project_dir)?;
    if resolved.is_empty() {
        return Err(RenderError::EmptyEdit);
    }
    Ok(resolved)
}

/// Parse a resolution string like "1920x1080" into `(width, height)`.
pub fn parse_resolution(s: &str) -> Result<(u32, u32), RenderError> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        return Err(RenderError::InvalidResolution(s.to_string()));
    }
    let w: u32 = parts[0]
        .parse()
        .map_err(|_| RenderError::InvalidResolution(s.to_string()))?;
    let h: u32 = parts[1]
        .parse()
        .map_err(|_| RenderError::InvalidResolution(s.to_string()))?;
    if w == 0 || h == 0 {
        return Err(RenderError::InvalidResolution(s.to_string()));
    }
    Ok((w, h))
}

// ---------------------------------------------------------------------------
// Codec helpers
// ---------------------------------------------------------------------------

/// Minimal source info needed for per-segment encode decisions.
#[derive(Debug, Clone)]
struct SourceInfo {
    id: String,
    video_codec: String,
    resolution: (u32, u32),
}

/// Resolved encoding parameters for the entire render.
#[derive(Debug)]
struct EncodeParams {
    /// The target video codec name (normalised, e.g. "h264").
    /// `None` means no explicit target — use stream copy when possible.
    target_codec: Option<String>,
    /// The target resolution. `None` means no scaling requested and sources
    /// all share the same resolution.
    target_resolution: Option<(u32, u32)>,
    /// Per-source codec/resolution info from manifest.
    source_info: Vec<SourceInfo>,
}

/// Determine encoding parameters by inspecting the manifest and render options.
fn resolve_encode_params(
    resolved: &[ResolvedShot],
    project_dir: &Path,
    options: &RenderOptions,
) -> Result<EncodeParams, RenderError> {
    // Collect unique source IDs referenced by this edit
    let source_ids: HashSet<&str> = resolved.iter().map(|s| s.source.as_str()).collect();

    // Try to load manifest for source metadata
    let manifest = project::read_manifest(project_dir).ok();

    let source_info: Vec<SourceInfo> = match &manifest {
        Some(m) => m
            .sources
            .iter()
            .filter(|s| source_ids.contains(s.id.as_str()))
            .map(|s| SourceInfo {
                id: s.id.clone(),
                video_codec: s.video_codec.clone(),
                resolution: s.resolution,
            })
            .collect(),
        None => Vec::new(),
    };

    // Determine target codec
    let target_codec = if let Some(ref c) = options.video_codec {
        // Explicit codec requested
        Some(normalize_codec(c).to_string())
    } else if !source_info.is_empty() {
        // Check if all sources share the same codec — if so, no re-encode needed
        let first = normalize_codec(&source_info[0].video_codec);
        let all_same = source_info
            .iter()
            .all(|s| normalize_codec(&s.video_codec) == first);
        if all_same {
            None // all match, stream copy
        } else {
            // Mixed codecs — default to h264
            Some("h264".to_string())
        }
    } else {
        None
    };

    // Determine target resolution
    let target_resolution = if let Some(res) = options.resolution {
        Some(res)
    } else if !source_info.is_empty() {
        // Find highest resolution across sources
        let max_res = source_info
            .iter()
            .max_by_key(|s| (s.resolution.0 as u64) * (s.resolution.1 as u64))
            .map(|s| s.resolution)
            .unwrap();

        // Only set target if sources differ in resolution
        let all_same = source_info.iter().all(|s| s.resolution == max_res);
        if all_same {
            None // no scaling needed
        } else {
            Some(max_res)
        }
    } else {
        None
    };

    Ok(EncodeParams {
        target_codec,
        target_resolution,
        source_info,
    })
}

/// Normalise codec names for comparison.
///
/// Maps common aliases to a canonical form so that e.g. "h264", "avc", and
/// "libx264" are all treated as the same codec.
pub fn normalize_codec(codec: &str) -> &str {
    match codec {
        "avc" | "libx264" | "h264" => "h264",
        "hevc" | "libx265" | "h265" => "h265",
        "libvpx-vp9" | "vp9" => "vp9",
        "libaom-av1" | "libsvtav1" | "av1" => "av1",
        other => other,
    }
}

/// Map a normalised codec name to the ffmpeg encoder name.
pub fn ffmpeg_video_encoder(codec: &str) -> &str {
    match normalize_codec(codec) {
        "h264" => "libx264",
        "h265" => "libx265",
        "vp9" => "libvpx-vp9",
        "av1" => "libsvtav1",
        _ => codec,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a segment from a source video with frame-accurate seeking.
///
/// Re-encodes video to ensure frame-accurate cuts and uniform output format
/// (pixel format, frame rate, audio sample rate) so concat with stream copy
/// is safe.
fn extract_segment(
    source: &Path,
    start_ms: u64,
    end_ms: u64,
    output: &Path,
) -> Result<(), RenderError> {
    let start_secs = start_ms as f64 / 1000.0;
    let duration_secs = end_ms.saturating_sub(start_ms) as f64 / 1000.0;

    let result = Command::new("ffmpeg")
        .args(["-y", "-ss", &format!("{start_secs:.3}"), "-i"])
        .arg(source)
        .args([
            "-t",
            &format!("{duration_secs:.3}"),
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-c:v",
            "libx264",
            "-crf",
            "18",
            "-preset",
            "fast",
            "-pix_fmt",
            "yuv420p",
            "-r",
            "30",
            "-c:a",
            "aac",
            "-ar",
            "48000",
            "-ac",
            "2",
        ])
        .arg(output)
        .output()
        .map_err(|e| RenderError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(RenderError::FfmpegFailed(format!(
            "segment extraction failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Extract a segment with explicit video encoder and optional filter.
///
/// Used when re-encoding is required due to codec mismatch, resolution change,
/// or overlay filters. Audio is always copied unchanged.
fn extract_segment_encoded(
    source: &Path,
    start_ms: u64,
    end_ms: u64,
    output: &Path,
    video_filter: Option<&str>,
    video_encoder: &str,
) -> Result<(), RenderError> {
    let start_secs = start_ms as f64 / 1000.0;
    let duration_secs = end_ms.saturating_sub(start_ms) as f64 / 1000.0;

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-ss", &format!("{start_secs:.3}"), "-i"])
        .arg(source)
        .args([
            "-t",
            &format!("{duration_secs:.3}"),
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
        ]);

    if let Some(vf) = video_filter {
        cmd.args(["-vf", vf]);
    }

    cmd.args(["-c:v", video_encoder, "-pix_fmt", "yuv420p", "-r", "30"])
        .args(["-c:a", "aac", "-ar", "48000", "-ac", "2"])
        .arg(output);

    let result = cmd
        .output()
        .map_err(|e| RenderError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(RenderError::FfmpegFailed(format!(
            "segment encoding failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Write a concat demuxer file listing segment paths.
///
/// Each line follows the format `file '<absolute-path>'`. Paths are made
/// absolute because ffmpeg's concat demuxer resolves relative `file` entries
/// against the directory containing the list file, not the process working
/// directory. When the render output is given as a relative path the work dir
/// (and thus the segment paths) are relative, so writing them verbatim would
/// make ffmpeg look for `<workdir>/<workdir>/segment.mp4` and fail.
fn write_concat_list(segment_paths: &[PathBuf], output: &Path) -> Result<(), RenderError> {
    let mut f = std::fs::File::create(output)?;
    for path in segment_paths {
        // The segments exist by now, so canonicalize resolves cleanly; fall
        // back to the original path only if it somehow doesn't.
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        writeln!(f, "file '{}'", abs.display())?;
    }
    Ok(())
}

/// Extract a segment while streaming ffmpeg stderr for progress updates.
///
/// When `video_encoder` is `Some`, re-encodes; when `None`, uses stream copy.
/// Reads ffmpeg stderr line-by-line and calls `on_progress` with updated status.
#[allow(clippy::too_many_arguments)]
fn extract_segment_with_progress<F>(
    source: &Path,
    start_ms: u64,
    end_ms: u64,
    output: &Path,
    video_filter: Option<&str>,
    video_encoder: Option<&str>,
    shot_idx: usize,
    shot_count: usize,
    shot_durations_ms: &[u64],
    start_time: Instant,
    shot_id: &str,
    on_progress: &F,
) -> Result<(), RenderError>
where
    F: Fn(&RenderProgress),
{
    let start_secs = start_ms as f64 / 1000.0;
    let duration_secs = end_ms.saturating_sub(start_ms) as f64 / 1000.0;

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-progress",
        "pipe:2",
        "-ss",
        &format!("{start_secs:.3}"),
        "-i",
    ])
    .arg(source)
    .args([
        "-t",
        &format!("{duration_secs:.3}"),
        "-map",
        "0:v:0",
        "-map",
        "0:a:0",
    ]);

    if let Some(vf) = video_filter {
        cmd.args(["-vf", vf]);
    }

    if let Some(encoder) = video_encoder {
        cmd.args(["-c:v", encoder, "-pix_fmt", "yuv420p", "-r", "30"]);
        cmd.args(["-c:a", "aac", "-ar", "48000", "-ac", "2"]);
    } else {
        cmd.args([
            "-c:v", "libx264", "-crf", "18", "-preset", "fast", "-pix_fmt", "yuv420p", "-r", "30",
        ]);
        cmd.args(["-c:a", "aac", "-ar", "48000", "-ac", "2"]);
    }

    cmd.arg(output);
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| RenderError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    // Read stderr for progress
    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if let Some(ffp) = parse_ffmpeg_progress(&line) {
                let rp = compute_progress(
                    shot_idx,
                    shot_count,
                    shot_durations_ms,
                    Some(&ffp),
                    start_time,
                    shot_id,
                );
                on_progress(&rp);
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| RenderError::FfmpegFailed(format!("ffmpeg wait failed: {e}")))?;

    if !status.success() {
        return Err(RenderError::FfmpegFailed(format!(
            "segment extraction failed (exit code: {})",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )));
    }

    Ok(())
}

/// Concatenate segments using the ffmpeg concat demuxer.
///
/// Uses stream copy since all segments are pre-encoded to uniform format
/// (same codec, pixel format, frame rate, and audio sample rate).
fn concat_segments(concat_list: &Path, output: &Path) -> Result<(), RenderError> {
    let result = Command::new("ffmpeg")
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(concat_list)
        .args(["-c", "copy"])
        .arg(output)
        .output()
        .map_err(|e| RenderError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(RenderError::FfmpegFailed(format!(
            "concat failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Generate SRT subtitles and embed them into the rendered video (REQ-036).
///
/// Writes a temporary `.srt` file next to the output, embeds it via ffmpeg,
/// then cleans up the temporary file.
fn embed_subtitles_into_output(
    resolved: &[ResolvedShot],
    project_dir: &Path,
    output: &Path,
) -> Result<(), RenderError> {
    let srt_content = subtitles::generate_srt(resolved, project_dir)?;
    if srt_content.is_empty() {
        return Ok(());
    }

    let srt_path = output.with_extension("srt");
    std::fs::write(&srt_path, &srt_content)?;

    let result = subtitles::embed_subtitles(output, &srt_path);

    // Clean up the temp SRT file regardless of success
    let _ = std::fs::remove_file(&srt_path);

    result?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -- parse_resolution -----------------------------------------------------

    #[test]
    fn parse_resolution_valid() {
        assert_eq!(parse_resolution("1920x1080").unwrap(), (1920, 1080));
        assert_eq!(parse_resolution("3840x2160").unwrap(), (3840, 2160));
        assert_eq!(parse_resolution("1280x720").unwrap(), (1280, 720));
    }

    #[test]
    fn parse_resolution_invalid_format() {
        assert!(parse_resolution("1920:1080").is_err());
        assert!(parse_resolution("1920").is_err());
        assert!(parse_resolution("widexhigh").is_err());
        assert!(parse_resolution("").is_err());
        assert!(parse_resolution("0x1080").is_err());
        assert!(parse_resolution("1920x0").is_err());
    }

    // -- normalize_codec ------------------------------------------------------

    #[test]
    fn normalize_codec_aliases() {
        assert_eq!(normalize_codec("h264"), "h264");
        assert_eq!(normalize_codec("avc"), "h264");
        assert_eq!(normalize_codec("libx264"), "h264");
        assert_eq!(normalize_codec("h265"), "h265");
        assert_eq!(normalize_codec("hevc"), "h265");
        assert_eq!(normalize_codec("libx265"), "h265");
        assert_eq!(normalize_codec("vp9"), "vp9");
        assert_eq!(normalize_codec("libvpx-vp9"), "vp9");
        assert_eq!(normalize_codec("av1"), "av1");
        assert_eq!(normalize_codec("libaom-av1"), "av1");
        assert_eq!(normalize_codec("libsvtav1"), "av1");
    }

    #[test]
    fn normalize_codec_passthrough() {
        assert_eq!(normalize_codec("prores"), "prores");
        assert_eq!(normalize_codec("mjpeg"), "mjpeg");
    }

    // -- ffmpeg_video_encoder -------------------------------------------------

    #[test]
    fn ffmpeg_video_encoder_mapping() {
        assert_eq!(ffmpeg_video_encoder("h264"), "libx264");
        assert_eq!(ffmpeg_video_encoder("avc"), "libx264");
        assert_eq!(ffmpeg_video_encoder("h265"), "libx265");
        assert_eq!(ffmpeg_video_encoder("hevc"), "libx265");
        assert_eq!(ffmpeg_video_encoder("vp9"), "libvpx-vp9");
        assert_eq!(ffmpeg_video_encoder("av1"), "libsvtav1");
    }

    #[test]
    fn ffmpeg_video_encoder_passthrough() {
        assert_eq!(ffmpeg_video_encoder("prores"), "prores");
    }

    // -- resolve_encode_params ------------------------------------------------

    #[test]
    fn encode_params_no_manifest_no_options() {
        let resolved = vec![fake_resolved_shot("shot-001", "src-001")];
        let tmp = TempDir::new().unwrap();
        let options = RenderOptions::default();
        let params = resolve_encode_params(&resolved, tmp.path(), &options).unwrap();
        assert!(params.target_codec.is_none());
        assert!(params.target_resolution.is_none());
        assert!(params.source_info.is_empty());
    }

    #[test]
    fn encode_params_explicit_codec() {
        let resolved = vec![fake_resolved_shot("shot-001", "src-001")];
        let tmp = TempDir::new().unwrap();
        let options = RenderOptions {
            video_codec: Some("h265".into()),
            ..Default::default()
        };
        let params = resolve_encode_params(&resolved, tmp.path(), &options).unwrap();
        assert_eq!(params.target_codec.as_deref(), Some("h265"));
    }

    #[test]
    fn encode_params_explicit_resolution() {
        let resolved = vec![fake_resolved_shot("shot-001", "src-001")];
        let tmp = TempDir::new().unwrap();
        let options = RenderOptions {
            resolution: Some((1280, 720)),
            ..Default::default()
        };
        let params = resolve_encode_params(&resolved, tmp.path(), &options).unwrap();
        assert_eq!(params.target_resolution, Some((1280, 720)));
    }

    // -- render_options_default -----------------------------------------------

    #[test]
    fn render_options_default() {
        let opts = RenderOptions::default();
        assert!(opts.video_codec.is_none());
        assert!(opts.resolution.is_none());
    }

    // -- write_concat_list ----------------------------------------------------

    #[test]
    fn write_concat_list_creates_file() {
        let tmp = TempDir::new().unwrap();
        let segments = vec![
            PathBuf::from("/tmp/segment_0000.mp4"),
            PathBuf::from("/tmp/segment_0001.mp4"),
            PathBuf::from("/tmp/segment_0002.mp4"),
        ];
        let list_path = tmp.path().join("concat.txt");
        write_concat_list(&segments, &list_path).unwrap();

        let content = std::fs::read_to_string(&list_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "file '/tmp/segment_0000.mp4'");
        assert_eq!(lines[1], "file '/tmp/segment_0001.mp4'");
        assert_eq!(lines[2], "file '/tmp/segment_0002.mp4'");
    }

    #[test]
    fn write_concat_list_empty() {
        let tmp = TempDir::new().unwrap();
        let list_path = tmp.path().join("concat.txt");
        write_concat_list(&[], &list_path).unwrap();

        let content = std::fs::read_to_string(&list_path).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn write_concat_list_single_segment() {
        let tmp = TempDir::new().unwrap();
        let segments = vec![PathBuf::from("/video/clip.mp4")];
        let list_path = tmp.path().join("concat.txt");
        write_concat_list(&segments, &list_path).unwrap();

        let content = std::fs::read_to_string(&list_path).unwrap();
        assert_eq!(content.trim(), "file '/video/clip.mp4'");
    }

    // -- resolve_preview (empty edit) -----------------------------------------

    #[test]
    fn resolve_preview_empty_edit_returns_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("index")).unwrap();

        let doc = EditDocument::create("test");
        let result = resolve_preview(&doc, tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no shots"));
    }

    // -- extract_segment_encoded (overlay filter) ------------------------------

    #[test]
    fn extract_segment_encoded_with_overlay_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path().join("out.mp4");
        let result = extract_segment_encoded(
            &PathBuf::from("/nonexistent/video.mp4"),
            0,
            5000,
            &output,
            Some("drawtext=text='test':fontsize=16:fontcolor=white:x=10:y=10"),
            "libx264",
        );
        assert!(result.is_err());
    }

    // -- error variants -------------------------------------------------------

    #[test]
    fn empty_edit_error_message() {
        let err = RenderError::EmptyEdit;
        assert_eq!(err.to_string(), "edit has no shots");
    }

    #[test]
    fn ffmpeg_failed_error_message() {
        let err = RenderError::FfmpegFailed("exit code 1".into());
        assert!(err.to_string().contains("exit code 1"));
    }

    #[test]
    fn invalid_resolution_error_message() {
        let err = RenderError::InvalidResolution("bad".into());
        assert!(err.to_string().contains("bad"));
        assert!(err.to_string().contains("WxH"));
    }

    // -- render_to_file -------------------------------------------------------

    #[test]
    fn render_to_file_empty_edit_returns_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("index")).unwrap();

        let doc = EditDocument::create("test");
        let output = tmp.path().join("output.mp4");
        let result = render_to_file(
            &doc,
            tmp.path(),
            &output,
            OverlayMode::Clean,
            &RenderOptions::default(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no shots"));
    }

    // -- write_concat_list ordering -------------------------------------------

    #[test]
    fn write_concat_list_preserves_segment_order() {
        let tmp = TempDir::new().unwrap();
        let segments = vec![
            PathBuf::from("/tmp/segment_0002.mp4"),
            PathBuf::from("/tmp/segment_0000.mp4"),
            PathBuf::from("/tmp/segment_0001.mp4"),
        ];
        let list_path = tmp.path().join("filelist.txt");
        write_concat_list(&segments, &list_path).unwrap();

        let content = std::fs::read_to_string(&list_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        // Order must match the input order, not sorted
        assert_eq!(lines[0], "file '/tmp/segment_0002.mp4'");
        assert_eq!(lines[1], "file '/tmp/segment_0000.mp4'");
        assert_eq!(lines[2], "file '/tmp/segment_0001.mp4'");
    }

    #[test]
    fn write_concat_list_handles_paths_with_spaces() {
        let tmp = TempDir::new().unwrap();
        let segments = vec![PathBuf::from("/my videos/segment 001.mp4")];
        let list_path = tmp.path().join("filelist.txt");
        write_concat_list(&segments, &list_path).unwrap();

        let content = std::fs::read_to_string(&list_path).unwrap();
        assert_eq!(content.trim(), "file '/my videos/segment 001.mp4'");
    }

    // Serializes the few tests that mutate the process-wide current directory.
    static CWD_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Regression: rendering to a *relative* output path produced a relative
    /// work dir, so the concat filelist held relative segment paths. ffmpeg's
    /// concat demuxer resolves those against the list file's own directory,
    /// looking for `<workdir>/<workdir>/segment.mp4` and failing with
    /// "Error opening input files". write_concat_list must emit absolute paths
    /// for segments that exist, regardless of the cwd-relative input.
    #[test]
    fn write_concat_list_makes_existing_relative_segments_absolute() {
        let _guard = CWD_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path().join(".ar-edit-render-out");
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::write(work_dir.join("segment_0000.mp4"), b"x").unwrap();
        let list_path = work_dir.join("filelist.txt");

        // Reproduce the buggy scenario: cwd is the project dir and the segment
        // is referenced by a relative path (as it would be for `--output out.mp4`).
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let segments = vec![PathBuf::from(".ar-edit-render-out/segment_0000.mp4")];
        let result = write_concat_list(&segments, &list_path);
        // Restore cwd before any assertion can unwind the test.
        std::env::set_current_dir(&original_cwd).unwrap();
        result.unwrap();

        let content = std::fs::read_to_string(&list_path).unwrap();
        let line = content.trim();
        let path_str = line
            .strip_prefix("file '")
            .and_then(|s| s.strip_suffix('\''))
            .expect("filelist line should be wrapped in file '...'");
        let written = std::path::Path::new(path_str);
        assert!(
            written.is_absolute(),
            "concat entry must be absolute, got: {path_str}"
        );
        let expected = std::fs::canonicalize(work_dir.join("segment_0000.mp4")).unwrap();
        assert_eq!(written, expected);
    }

    // -- extract_segment (error path) -----------------------------------------

    #[test]
    fn extract_segment_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path().join("out.mp4");
        let result = extract_segment(&PathBuf::from("/nonexistent/video.mp4"), 0, 5000, &output);
        assert!(result.is_err());
    }

    // -- extract_segment_encoded (error path) ---------------------------------

    #[test]
    fn extract_segment_encoded_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path().join("out.mp4");
        let result = extract_segment_encoded(
            &PathBuf::from("/nonexistent/video.mp4"),
            0,
            5000,
            &output,
            None,
            "libx264",
        );
        assert!(result.is_err());
    }

    #[test]
    fn extract_segment_encoded_with_filter_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path().join("out.mp4");
        let result = extract_segment_encoded(
            &PathBuf::from("/nonexistent/video.mp4"),
            0,
            5000,
            &output,
            Some("scale=1280:720"),
            "libx265",
        );
        assert!(result.is_err());
    }

    // -- test helpers ---------------------------------------------------------

    fn fake_resolved_shot(id: &str, source: &str) -> ResolvedShot {
        ResolvedShot {
            id: id.to_string(),
            source: source.to_string(),
            range: crate::models::ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
            start_ms: 0,
            end_ms: 5000,
            duration_ms: 5000,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
            author: String::new(),
        }
    }
}
