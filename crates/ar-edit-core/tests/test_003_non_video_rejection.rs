//! TEST-003: Non-video rejection
//!
//! Verifies that `project::add()` rejects files that are not valid video
//! containers with both a video and audio stream (REQ-003).
//! Tests requiring ffprobe are skipped when it is unavailable.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use ar_edit_core::project;
use tempfile::TempDir;

fn has_ffprobe() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn has_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn rejects_plain_text_file() {
    if !has_ffprobe() {
        eprintln!("SKIPPED: ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let txt_file = tmp.path().join("notes.txt");
    fs::write(&txt_file, "this is not a video").unwrap();

    let err = project::add(&project_dir, &[txt_file]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not a video") || msg.contains("ffprobe"),
        "expected rejection of text file, got: {msg}"
    );
}

#[test]
fn rejects_empty_file() {
    if !has_ffprobe() {
        eprintln!("SKIPPED: ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let empty = tmp.path().join("empty.mp4");
    fs::write(&empty, b"").unwrap();

    let err = project::add(&project_dir, &[empty]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not a video") || msg.contains("ffprobe"),
        "expected rejection of empty file, got: {msg}"
    );
}

#[test]
fn rejects_audio_only_file() {
    if !has_ffmpeg() || !has_ffprobe() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    // Create an audio-only file (no video stream)
    let audio_only = tmp.path().join("audio.wav");
    let created = Command::new("ffmpeg")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-y"])
        .arg(&audio_only)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !created {
        eprintln!("SKIPPED: could not create audio-only test file");
        return;
    }

    let err = project::add(&project_dir, &[audio_only]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not a video") || msg.contains("no video stream"),
        "expected rejection of audio-only file, got: {msg}"
    );
}

#[test]
fn rejects_image_file() {
    if !has_ffprobe() {
        eprintln!("SKIPPED: ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    // Create a minimal valid PNG (1x1 red pixel)
    let png = tmp.path().join("photo.png");
    #[rustfmt::skip]
    let png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
        0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
        0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, // IDAT chunk
        0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
        0x00, 0x00, 0x02, 0x00, 0x01, 0xe2, 0x21, 0xbc,
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, // IEND chunk
        0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    fs::write(&png, &png_bytes).unwrap();

    let err = project::add(&project_dir, &[png]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not a video") || msg.contains("no audio stream"),
        "expected rejection of image file, got: {msg}"
    );
}

#[test]
fn rejects_nonexistent_file() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let err = project::add(&project_dir, &[PathBuf::from("/no/such/file.mp4")]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not found"),
        "expected file-not-found error, got: {msg}"
    );
}

#[test]
fn rejects_json_file() {
    if !has_ffprobe() {
        eprintln!("SKIPPED: ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let json_file = tmp.path().join("data.json");
    fs::write(&json_file, r#"{"key": "value"}"#).unwrap();

    let err = project::add(&project_dir, &[json_file]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not a video") || msg.contains("ffprobe"),
        "expected rejection of JSON file, got: {msg}"
    );
}
