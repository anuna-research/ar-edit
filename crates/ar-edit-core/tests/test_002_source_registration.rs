//! TEST-002: Source registration + metadata
//!
//! Verifies that `project::add()` registers video sources with correct
//! metadata, assigns sequential IDs, symlinks into sources/, and updates
//! the manifest.  Tests requiring ffmpeg/ffprobe are skipped when those
//! tools are unavailable.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ar_edit_core::project;
use tempfile::TempDir;

/// Returns true if both ffmpeg and ffprobe are available on PATH.
fn has_fftools() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        && Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// Generate a minimal 1-second test video (320x240, h264+aac).
fn create_test_video(path: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=black:s=320x240:d=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
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

// ---- Tests that don't require ffmpeg/ffprobe --------------------------------

#[test]
fn add_rejects_missing_file() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let err = project::add(&project_dir, &[PathBuf::from("/nonexistent/video.mp4")]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("not found"),
        "expected FileNotFound error, got: {msg}"
    );
}

#[test]
fn add_requires_initialised_project() {
    let tmp = TempDir::new().unwrap();
    // Not calling init — directory exists but no manifest.json
    let project_dir = tmp.path().join("not-a-project");
    fs::create_dir(&project_dir).unwrap();

    let err = project::add(&project_dir, &[PathBuf::from("video.mp4")]).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("manifest.json"),
        "expected NotAProject error, got: {msg}"
    );
}

// ---- Tests that require ffmpeg + ffprobe ------------------------------------

#[test]
fn add_registers_single_source_with_metadata() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video = tmp.path().join("interview.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let sources = project::add(&project_dir, &[video]).unwrap();
    assert_eq!(sources.len(), 1);

    let src = &sources[0];
    assert_eq!(src.id, "src-001");
    assert_eq!(src.original_filename, "interview.mp4");
    assert_eq!(src.resolution, (320, 240));
    assert!(!src.video_codec.is_empty());
    assert!(!src.audio_codec.is_empty());
    assert!(src.duration_ms > 0);
    assert!(src.frame_rate > 0.0);
    assert!(src.audio_channels > 0);
    assert!(src.audio_sample_rate > 0);
    assert!(!src.transcribed);
    assert!(!src.indexed);
}

#[test]
fn add_creates_symlink_in_sources_dir() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video = tmp.path().join("clip.mp4");
    assert!(create_test_video(&video), "failed to create test video");

    let sources = project::add(&project_dir, &[video]).unwrap();
    let linked_path = project_dir.join(&sources[0].path);

    assert!(linked_path.exists(), "symlinked source file should exist");
    #[cfg(unix)]
    assert!(
        linked_path
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink(),
        "source should be a symlink on unix"
    );
}

#[test]
fn add_assigns_sequential_source_ids() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video1 = tmp.path().join("a.mp4");
    let video2 = tmp.path().join("b.mp4");
    assert!(create_test_video(&video1));
    assert!(create_test_video(&video2));

    let first = project::add(&project_dir, &[video1]).unwrap();
    assert_eq!(first[0].id, "src-001");

    let second = project::add(&project_dir, &[video2]).unwrap();
    assert_eq!(second[0].id, "src-002");
}

#[test]
fn add_updates_manifest_on_disk() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video = tmp.path().join("clip.mp4");
    assert!(create_test_video(&video));

    project::add(&project_dir, &[video]).unwrap();

    let manifest = project::read_manifest(&project_dir).unwrap();
    assert_eq!(manifest.sources.len(), 1);
    assert_eq!(manifest.sources[0].id, "src-001");
    assert_eq!(manifest.next_source_id, 2);
}

#[test]
fn add_multiple_files_at_once() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-project");
    project::init(&project_dir).unwrap();

    let video1 = tmp.path().join("a.mp4");
    let video2 = tmp.path().join("b.mp4");
    assert!(create_test_video(&video1));
    assert!(create_test_video(&video2));

    let sources = project::add(&project_dir, &[video1, video2]).unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].id, "src-001");
    assert_eq!(sources[1].id, "src-002");

    let manifest = project::read_manifest(&project_dir).unwrap();
    assert_eq!(manifest.sources.len(), 2);
    assert_eq!(manifest.next_source_id, 3);
}
