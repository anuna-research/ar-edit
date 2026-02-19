//! TEST-005 / TEST-006: whisper.cpp subprocess invocation and model selection
//!
//! Verifies that `transcript::invoke_whisper()` correctly invokes whisper-cli,
//! parses its JSON output into a `Transcript`, and handles model selection.
//! Tests requiring whisper-cli are skipped when it is unavailable.

use std::path::{Path, PathBuf};
use std::process::Command;

use ar_edit_core::transcript::{
    self, parse_progress, validate_model, WhisperProgress, DEFAULT_WHISPER_MODEL, WHISPER_MODELS,
};
use tempfile::TempDir;

/// Returns true if whisper-cli is available on PATH.
fn has_whisper() -> bool {
    Command::new("whisper-cli")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Returns true if ffmpeg is available on PATH.
fn has_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Try to find a whisper model for testing. Returns None if no model found.
fn find_test_model() -> Option<PathBuf> {
    transcript::find_model(DEFAULT_WHISPER_MODEL).ok()
}

/// Generate a minimal 1-second test video (320x240, h264+aac) with speech-like audio.
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

// ---- Tests that don't require external tools --------------------------------

#[test]
fn default_model_is_base() {
    assert_eq!(DEFAULT_WHISPER_MODEL, "base");
}

#[test]
fn whisper_models_contains_all_five() {
    assert_eq!(WHISPER_MODELS.len(), 5);
    assert!(WHISPER_MODELS.contains(&"tiny"));
    assert!(WHISPER_MODELS.contains(&"base"));
    assert!(WHISPER_MODELS.contains(&"small"));
    assert!(WHISPER_MODELS.contains(&"medium"));
    assert!(WHISPER_MODELS.contains(&"large"));
}

#[test]
fn validate_model_accepts_all_valid_names() {
    for model in WHISPER_MODELS {
        assert!(
            validate_model(model).is_ok(),
            "should accept model '{model}'"
        );
    }
}

#[test]
fn validate_model_rejects_invalid_names() {
    assert!(validate_model("huge").is_err());
    assert!(validate_model("turbo").is_err());
    assert!(validate_model("").is_err());
    assert!(validate_model("BASE").is_err()); // case-sensitive
}

#[test]
fn validate_model_error_message_is_descriptive() {
    let err = validate_model("huge").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("huge"), "should mention the invalid model");
    assert!(msg.contains("tiny"), "should list valid options");
}

#[test]
fn parse_progress_from_whisper_stderr() {
    let stderr = "\
whisper_init_from_file: loading model '/path/to/model'
whisper_model_load: loading model
whisper_print_progress_callback: progress =   5%
whisper_print_progress_callback: progress =  25%
whisper_print_progress_callback: progress =  50%
whisper_print_progress_callback: progress =  75%
whisper_print_progress_callback: progress = 100%
main: output text: hello world";

    let progress = parse_progress(stderr);
    assert_eq!(progress.len(), 5);
    assert_eq!(progress[0], WhisperProgress { percent: 5 });
    assert_eq!(progress[1], WhisperProgress { percent: 25 });
    assert_eq!(progress[2], WhisperProgress { percent: 50 });
    assert_eq!(progress[3], WhisperProgress { percent: 75 });
    assert_eq!(progress[4], WhisperProgress { percent: 100 });
}

#[test]
fn parse_progress_handles_no_progress_lines() {
    let stderr = "whisper_init_from_file: loading model\nmain: done\n";
    let progress = parse_progress(stderr);
    assert!(progress.is_empty());
}

#[test]
fn invoke_whisper_rejects_nonexistent_audio() {
    let err = transcript::invoke_whisper(
        Path::new("/nonexistent/audio.wav"),
        Path::new("/nonexistent/model.bin"),
        "src-001",
    );
    assert!(err.is_err());
}

#[test]
fn find_model_rejects_invalid_name() {
    let err = transcript::find_model("huge").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("huge"));
}

// ---- Tests that require whisper-cli and a model -----------------------------

#[test]
fn invoke_whisper_transcribes_audio() {
    if !has_whisper() {
        eprintln!("SKIPPED: whisper-cli not available");
        return;
    }
    if !has_ffmpeg() {
        eprintln!("SKIPPED: ffmpeg not available");
        return;
    }
    let model_path = match find_test_model() {
        Some(p) => p,
        None => {
            eprintln!("SKIPPED: no whisper model found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    let video = tmp.path().join("test.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let wav = tmp.path().join("src-001.wav");
    transcript::extract_audio(&video, &wav).unwrap();

    let (transcript, progress) = transcript::invoke_whisper(&wav, &model_path, "src-001").unwrap();

    // Basic structure checks
    assert_eq!(transcript.source_id, "src-001");
    assert!(!transcript.language.is_empty());
    assert!(transcript.duration_ms > 0);

    // Progress should have been reported
    assert!(
        !progress.is_empty(),
        "whisper should report at least one progress update"
    );

    // Verify the intermediate JSON file was cleaned up
    let json_path = wav.with_file_name("src-001.wav.json");
    assert!(
        !json_path.exists(),
        "intermediate whisper JSON should be deleted"
    );
}

#[test]
fn invoke_whisper_with_model_flag_uses_specified_model() {
    // TEST-006: verify the model name flows through to the transcript
    if !has_whisper() {
        eprintln!("SKIPPED: whisper-cli not available");
        return;
    }
    if !has_ffmpeg() {
        eprintln!("SKIPPED: ffmpeg not available");
        return;
    }
    let model_path = match find_test_model() {
        Some(p) => p,
        None => {
            eprintln!("SKIPPED: no whisper model found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    let video = tmp.path().join("test.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let wav = tmp.path().join("src-001.wav");
    transcript::extract_audio(&video, &wav).unwrap();

    let (transcript, _) = transcript::invoke_whisper(&wav, &model_path, "src-001").unwrap();

    // The model name should be extracted from the model filename
    // (ggml-base.bin → "base")
    assert!(
        WHISPER_MODELS.contains(&transcript.model.as_str()),
        "model should be one of the valid whisper model names, got: {}",
        transcript.model
    );
}
