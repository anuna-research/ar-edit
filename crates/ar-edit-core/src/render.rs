use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::display::{self, ResolvedShot};
use crate::models::EditDocument;
use crate::overlay::{self, OverlayInfo, OverlayMode};
use crate::playback;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("edit has no shots")]
    EmptyEdit,
    #[error(transparent)]
    Display(#[from] display::DisplayError),
    #[error(transparent)]
    Playback(#[from] playback::PlaybackError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

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
) -> Result<PathBuf, RenderError> {
    let resolved = display::resolve_edit(doc, project_dir)?;
    if resolved.is_empty() {
        return Err(RenderError::EmptyEdit);
    }

    let preview_dir = std::env::temp_dir().join(format!("ar-edit-preview-{}", doc.name));
    std::fs::create_dir_all(&preview_dir)?;

    // Extract each shot as a segment
    let mut segment_paths = Vec::with_capacity(resolved.len());
    let mut timeline_offset_ms: u64 = 0;

    for (i, shot) in resolved.iter().enumerate() {
        let (source_path, _) = playback::resolve_source_path(&shot.source, project_dir)?;
        let segment_path = preview_dir.join(format!("segment_{i:04}.mp4"));

        let filter = overlay::build_drawtext_filter(
            overlay_mode,
            &OverlayInfo {
                shot_id: shot.id.clone(),
                source_id: shot.source.clone(),
                snippet: shot.text_preview.clone().or(shot.scene_preview.clone()),
                timecode_offset_sec: timeline_offset_ms as f64 / 1000.0,
            },
        );

        match filter {
            Some(ref vf) => extract_segment_with_filter(
                &source_path,
                shot.start_ms,
                shot.end_ms,
                &segment_path,
                vf,
            )?,
            None => extract_segment(&source_path, shot.start_ms, shot.end_ms, &segment_path)?,
        }

        segment_paths.push(segment_path);
        timeline_offset_ms += shot.duration_ms;
    }

    // Write concat demuxer file
    let concat_list_path = preview_dir.join("concat.txt");
    write_concat_list(&segment_paths, &concat_list_path)?;

    // Concatenate segments
    let output_path = preview_dir.join("preview.mp4");
    concat_segments(&concat_list_path, &output_path)?;

    Ok(output_path)
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

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a segment from a source video using ffmpeg stream copy.
///
/// Uses `-ss` before `-i` for fast seeking, then `-t` for duration.
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
        .args(["-t", &format!("{duration_secs:.3}"), "-c", "copy"])
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

/// Extract a segment from a source video with a video filter applied.
///
/// Re-encodes the video (cannot use stream copy with filters) while copying
/// audio unchanged. Used for overlay modes that require drawtext filters.
fn extract_segment_with_filter(
    source: &Path,
    start_ms: u64,
    end_ms: u64,
    output: &Path,
    video_filter: &str,
) -> Result<(), RenderError> {
    let start_secs = start_ms as f64 / 1000.0;
    let duration_secs = end_ms.saturating_sub(start_ms) as f64 / 1000.0;

    let result = Command::new("ffmpeg")
        .args(["-y", "-ss", &format!("{start_secs:.3}"), "-i"])
        .arg(source)
        .args(["-t", &format!("{duration_secs:.3}")])
        .args(["-vf", video_filter])
        .args(["-c:a", "copy"])
        .arg(output)
        .output()
        .map_err(|e| RenderError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(RenderError::FfmpegFailed(format!(
            "segment extraction with overlay failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(())
}

/// Write a concat demuxer file listing segment paths.
///
/// Each line follows the format `file '<absolute-path>'`.
fn write_concat_list(segment_paths: &[PathBuf], output: &Path) -> Result<(), RenderError> {
    let mut f = std::fs::File::create(output)?;
    for path in segment_paths {
        writeln!(f, "file '{}'", path.display())?;
    }
    Ok(())
}

/// Concatenate segments using the ffmpeg concat demuxer.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    // -- extract_segment_with_filter ------------------------------------------

    #[test]
    fn extract_segment_with_filter_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let output = tmp.path().join("out.mp4");
        let result = extract_segment_with_filter(
            &PathBuf::from("/nonexistent/video.mp4"),
            0,
            5000,
            &output,
            "drawtext=text='test':fontsize=16:fontcolor=white:x=10:y=10",
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
}
