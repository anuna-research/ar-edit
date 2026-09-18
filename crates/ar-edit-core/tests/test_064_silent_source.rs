//! TEST-003b: Silent sources are accepted and render with generated silence
//! (REQ-003).
//!
//! Screen recordings and browser screencasts commonly have no audio stream.
//! Verifies that `project::add()` registers such files with zeroed audio
//! metadata, that still images are still rejected, and that rendering a
//! silent source produces an output with a uniform aac audio track. Tests
//! requiring ffmpeg/ffprobe are skipped when those tools are unavailable.

use std::path::Path;
use std::process::Command;

use ar_edit_core::models::*;
use ar_edit_core::overlay::OverlayMode;
use ar_edit_core::project;
use ar_edit_core::render::{self, RenderOptions};
use tempfile::TempDir;

/// Returns true if both ffmpeg and ffprobe are available on PATH.
fn has_fftools() -> bool {
    ["ffprobe", "ffmpeg"].iter().all(|tool| {
        Command::new(tool)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Generate a 2-second video with no audio stream (320x240, h264).
fn create_silent_video(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=black:s=320x240:d=2",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Generate a single-frame PNG via ffmpeg.
fn create_png(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=red:s=8x8:d=1",
            "-frames:v",
            "1",
            "-y",
        ])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Codec types (`video`, `audio`, ...) of the streams in a media file.
fn stream_types(path: &Path) -> Vec<String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe should run");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// ---- Tests that don't require ffmpeg/ffprobe --------------------------------

#[test]
fn has_audio_reflects_channel_count() {
    let mut src = Source {
        id: "src-001".into(),
        path: "sources/src-001.mp4".into(),
        original_filename: "clip.mp4".into(),
        duration_ms: 1000,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (320, 240),
        frame_rate: 30.0,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: chrono::Utc::now(),
        transcribed: false,
        indexed: false,
    };
    assert!(src.has_audio());

    src.audio_codec.clear();
    src.audio_channels = 0;
    src.audio_sample_rate = 0;
    assert!(!src.has_audio());
}

// ---- Tests that require ffmpeg + ffprobe ------------------------------------

#[test]
fn add_accepts_silent_video_with_zeroed_audio_metadata() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video = tmp.path().join("screencast.mp4");
    assert!(create_silent_video(&video), "failed to create silent video");

    let sources = project::add(&project_dir, &[video]).unwrap();
    let src = &sources[0];
    assert_eq!(src.id, "src-001");
    assert!(!src.video_codec.is_empty());
    assert!(src.duration_ms > 0);
    assert!(src.audio_codec.is_empty());
    assert_eq!(src.audio_channels, 0);
    assert_eq!(src.audio_sample_rate, 0);
    assert!(!src.has_audio());

    // Round-trips through the manifest unchanged.
    let manifest = project::read_manifest(&project_dir).unwrap();
    assert!(!manifest.sources[0].has_audio());
}

#[test]
fn add_still_rejects_still_images() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let png = tmp.path().join("frame.png");
    assert!(create_png(&png), "failed to create png");

    let err = project::add(&project_dir, &[png]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("still image"),
        "expected still-image rejection, got: {msg}"
    );
}

#[test]
fn render_silent_source_produces_audio_track() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video = tmp.path().join("screencast.mp4");
    assert!(create_silent_video(&video), "failed to create silent video");
    project::add(&project_dir, &[video]).unwrap();

    let mut doc = EditDocument::create("demo");
    doc.add_shot(
        "src-001",
        ShotRange::Time {
            from_ms: 250,
            to_ms: 1250,
        },
    )
    .unwrap();

    // Once via the stream-copy path and once forcing a re-encode, so both
    // extractors are exercised.
    for (name, options) in [
        ("copy.mp4", RenderOptions::default()),
        (
            "reencode.mp4",
            RenderOptions {
                video_codec: Some("h264".into()),
                resolution: Some((160, 120)),
                ..RenderOptions::default()
            },
        ),
    ] {
        let output = tmp.path().join(name);
        render::render_to_file(&doc, &project_dir, &output, OverlayMode::Clean, &options)
            .unwrap_or_else(|e| panic!("render {name} failed: {e}"));

        let types = stream_types(&output);
        assert!(
            types.iter().any(|t| t == "video"),
            "{name}: expected a video stream, got {types:?}"
        );
        assert!(
            types.iter().any(|t| t == "audio"),
            "{name}: expected a generated audio stream, got {types:?}"
        );
    }
}
