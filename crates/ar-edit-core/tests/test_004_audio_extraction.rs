//! TEST-004: Audio extraction via ffmpeg
//!
//! Verifies that `transcript::extract_audio()` extracts mono 16 kHz PCM WAV
//! from video files.  Tests requiring ffmpeg are skipped when it is unavailable.

use std::path::Path;
use std::process::Command;

use ar_edit_core::transcript;
use tempfile::TempDir;

/// Returns true if ffmpeg is available on PATH.
fn has_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a minimal 1-second test video (320x240, h264+aac).
fn create_test_video(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-f", "lavfi",
            "-i", "color=black:s=320x240:d=1",
            "-f", "lavfi",
            "-i", "sine=frequency=440:duration=1",
            "-c:v", "libx264",
            "-c:a", "aac",
            "-shortest",
            "-y",
        ])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Generate a 1-second video with no audio stream.
fn create_video_no_audio(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-f", "lavfi",
            "-i", "color=black:s=320x240:d=1",
            "-c:v", "libx264",
            "-an",
            "-y",
        ])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---- Tests that don't require ffmpeg ----------------------------------------

#[test]
fn extract_audio_rejects_nonexistent_source() {
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.wav");
    let err = transcript::extract_audio(Path::new("/nonexistent/video.mp4"), &output);
    assert!(err.is_err());
}

// ---- Tests that require ffmpeg ----------------------------------------------

#[test]
fn extract_audio_produces_wav_file() {
    if !has_ffmpeg() {
        eprintln!("SKIPPED: ffmpeg not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let video = tmp.path().join("test.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let wav = tmp.path().join("output.wav");
    let result = transcript::extract_audio(&video, &wav).unwrap();

    assert_eq!(result, wav);
    assert!(wav.exists(), "WAV file should exist");
    assert!(
        std::fs::metadata(&wav).unwrap().len() > 0,
        "WAV file should not be empty"
    );
}

#[test]
fn extract_audio_wav_is_valid_pcm() {
    if !has_ffmpeg() {
        eprintln!("SKIPPED: ffmpeg not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let video = tmp.path().join("test.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let wav = tmp.path().join("output.wav");
    transcript::extract_audio(&video, &wav).unwrap();

    // Verify the WAV format using ffprobe
    let probe = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_streams"])
        .arg(&wav)
        .output()
        .expect("ffprobe should be available");

    let stdout = String::from_utf8_lossy(&probe.stdout);
    // Should be mono (1 channel), 16000 Hz, pcm_s16le
    assert!(stdout.contains("pcm_s16le"), "expected pcm_s16le codec");
    assert!(stdout.contains("16000"), "expected 16000 Hz sample rate");
    assert!(
        stdout.contains("\"channels\": 1") || stdout.contains("\"channels\":1"),
        "expected mono (1 channel)"
    );
}

#[test]
fn extract_audio_no_audio_stream_error() {
    if !has_ffmpeg() {
        eprintln!("SKIPPED: ffmpeg not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let video = tmp.path().join("no-audio.mp4");
    assert!(
        create_video_no_audio(&video),
        "failed to create video without audio"
    );

    let wav = tmp.path().join("output.wav");
    let err = transcript::extract_audio(&video, &wav).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("no audio stream") || msg.contains("ffmpeg"),
        "expected no-audio-stream error, got: {msg}"
    );
}

#[test]
fn extract_audio_overwrites_existing_output() {
    if !has_ffmpeg() {
        eprintln!("SKIPPED: ffmpeg not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let video = tmp.path().join("test.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let wav = tmp.path().join("output.wav");

    // Create a dummy file at the output path
    std::fs::write(&wav, b"dummy").unwrap();

    // extract_audio should overwrite it (uses -y flag)
    transcript::extract_audio(&video, &wav).unwrap();

    let size = std::fs::metadata(&wav).unwrap().len();
    assert!(size > 5, "WAV should be larger than the dummy content");
}
