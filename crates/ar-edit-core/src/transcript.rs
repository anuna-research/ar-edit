use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum TranscriptError {
    #[error("ffmpeg not found: {0}")]
    FfmpegNotFound(std::io::Error),
    #[error("source file has no audio stream: {0}")]
    NoAudioStream(PathBuf),
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract WAV audio from a video file using ffmpeg.
///
/// Runs: `ffmpeg -i <source> -vn -acodec pcm_s16le -ar 16000 -ac 1 <output>`
///
/// The output is 16 kHz mono PCM suitable for whisper.cpp ingestion.
/// Returns the path to the generated WAV file.
pub fn extract_audio(source: &Path, output: &Path) -> Result<PathBuf, TranscriptError> {
    let result = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(source)
        .args(["-vn", "-acodec", "pcm_s16le", "-ar", "16000", "-ac", "1"])
        .arg(output)
        .output()
        .map_err(TranscriptError::FfmpegNotFound)?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);

        if stderr.contains("does not contain any stream")
            || stderr.contains("matches no streams")
            || stderr.contains("no audio stream")
        {
            return Err(TranscriptError::NoAudioStream(source.to_path_buf()));
        }

        return Err(TranscriptError::FfmpegFailed(format!(
            "ffmpeg audio extraction failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(output.to_path_buf())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_audio_rejects_nonexistent_source() {
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("out.wav");
        let err = extract_audio(Path::new("/nonexistent/video.mp4"), &output).unwrap_err();
        // ffmpeg will fail (either not found or failed on missing input)
        let msg = format!("{err}");
        assert!(
            msg.contains("ffmpeg") || msg.contains("No such file"),
            "expected ffmpeg-related error, got: {msg}"
        );
    }

    #[test]
    fn extract_audio_returns_output_path() {
        // Verify the function signature returns the output path on success.
        // This is a compile-time check; actual ffmpeg tests are in integration tests.
        let _: fn(&Path, &Path) -> Result<PathBuf, TranscriptError> = extract_audio;
    }
}
