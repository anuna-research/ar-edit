//! TEST-047: Playback integration
//!
//! Verifies the playback pipeline: resolving source paths from manifest,
//! constructing play requests with correct file/timecode parameters, and
//! player detection behavior.

use std::fs;
use std::path::{Path, PathBuf};

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::{Defaults, EditDocument, Manifest, ShotRange, Source};
use ar_edit_core::playback::{self, PlayRequest, PlayerKind};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_source(id: &str) -> Source {
    Source {
        id: id.into(),
        path: PathBuf::from(format!("sources/{id}.mp4")),
        original_filename: format!("{id}.mp4"),
        duration_ms: 60_000,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (1920, 1080),
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed: false,
        indexed: false,
    }
}

fn setup_project_with_sources(dir: &Path, source_ids: &[&str]) {
    fs::create_dir_all(dir.join("sources")).unwrap();
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("edits")).unwrap();
    fs::create_dir_all(dir.join("thumbnails")).unwrap();
    fs::create_dir_all(dir.join("annotations")).unwrap();

    let sources: Vec<Source> = source_ids.iter().map(|id| make_source(id)).collect();

    // Create dummy source files so resolve_source_path can find them
    for source in &sources {
        let file_path = dir.join(&source.path);
        fs::write(&file_path, b"fake video data").unwrap();
    }

    let manifest = Manifest {
        version: "1.0.0".into(),
        name: "test".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
        sources,
        next_source_id: source_ids.len() as u32 + 1,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    };

    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Tests: Player detection
// ---------------------------------------------------------------------------

/// Player detection returns a valid result or PlayerNotFound.
#[test]
fn detect_player_returns_result() {
    match playback::detect_player() {
        Ok(player) => {
            assert!(!player.name.is_empty());
            assert!(player.path.exists());
        }
        Err(e) => {
            assert!(format!("{e}").contains("no video player"));
        }
    }
}

/// PlayerKind variants are distinct.
#[test]
fn player_kinds_are_distinct() {
    assert_ne!(PlayerKind::Vlc, PlayerKind::Ffplay);
    assert_eq!(PlayerKind::Vlc, PlayerKind::Vlc);
    assert_eq!(PlayerKind::Ffplay, PlayerKind::Ffplay);
}

// ---------------------------------------------------------------------------
// Tests: Source path resolution
// ---------------------------------------------------------------------------

/// resolve_source_path finds the correct file for a registered source.
#[test]
fn resolve_source_path_finds_file() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_sources(tmp.path(), &["src-001", "src-002"]);

    let (path, source) = playback::resolve_source_path("src-001", tmp.path()).unwrap();
    assert_eq!(source.id, "src-001");
    assert!(path.exists());
    assert!(path.to_string_lossy().contains("src-001"));
}

/// resolve_source_path returns the correct source metadata.
#[test]
fn resolve_source_path_returns_metadata() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_sources(tmp.path(), &["src-001"]);

    let (_, source) = playback::resolve_source_path("src-001", tmp.path()).unwrap();
    assert_eq!(source.duration_ms, 60_000);
    assert_eq!(source.resolution, (1920, 1080));
}

/// resolve_source_path fails for unknown source ID.
#[test]
fn resolve_source_path_unknown_source() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_sources(tmp.path(), &["src-001"]);

    let err = playback::resolve_source_path("src-999", tmp.path()).unwrap_err();
    assert!(format!("{err}").contains("not found"));
}

// ---------------------------------------------------------------------------
// Tests: PlayRequest construction from resolved shots
// ---------------------------------------------------------------------------

/// Play request for a time-range shot has correct start/end.
#[test]
fn play_request_from_time_shot() {
    let shot = ResolvedShot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Time {
            from_ms: 5000,
            to_ms: 15000,
        },
        start_ms: 5000,
        end_ms: 15000,
        duration_ms: 10000,
        text_preview: None,
        scene_preview: None,
        notes: vec![],
    };

    // Simulate what do_play does in the TUI:
    let start_ms = shot.start_ms;
    let end_ms = if shot.duration_ms > 0 {
        Some(shot.end_ms)
    } else {
        None
    };

    assert_eq!(start_ms, 5000);
    assert_eq!(end_ms, Some(15000));
}

/// Play request for a shot with zero duration omits end time.
#[test]
fn play_request_zero_duration_no_end() {
    let shot = ResolvedShot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 0 },
        start_ms: 0,
        end_ms: 0,
        duration_ms: 0,
        text_preview: None,
        scene_preview: None,
        notes: vec![],
    };

    let end_ms = if shot.duration_ms > 0 {
        Some(shot.end_ms)
    } else {
        None
    };

    assert_eq!(end_ms, None);
}

/// PlayRequest is clonable and preserves all fields.
#[test]
fn play_request_clone_preserves_fields() {
    let req = PlayRequest {
        file: PathBuf::from("/tmp/video.mp4"),
        start_ms: 5500,
        end_ms: Some(12000),
    };
    let cloned = req.clone();
    assert_eq!(cloned.file, PathBuf::from("/tmp/video.mp4"));
    assert_eq!(cloned.start_ms, 5500);
    assert_eq!(cloned.end_ms, Some(12000));
}

// ---------------------------------------------------------------------------
// Tests: Timecode parsing (used for --at flag)
// ---------------------------------------------------------------------------

#[test]
fn parse_timecode_seconds() {
    assert_eq!(playback::parse_timecode("90").unwrap(), 90_000);
}

#[test]
fn parse_timecode_fractional() {
    assert_eq!(playback::parse_timecode("5.5").unwrap(), 5_500);
}

#[test]
fn parse_timecode_mm_ss() {
    assert_eq!(playback::parse_timecode("02:30").unwrap(), 150_000);
}

#[test]
fn parse_timecode_hh_mm_ss() {
    assert_eq!(playback::parse_timecode("01:00:00").unwrap(), 3_600_000);
}

#[test]
fn parse_timecode_zero() {
    assert_eq!(playback::parse_timecode("0").unwrap(), 0);
}

#[test]
fn parse_timecode_invalid() {
    assert!(playback::parse_timecode("abc").is_err());
}

#[test]
fn parse_timecode_too_many_parts() {
    assert!(playback::parse_timecode("1:2:3:4").is_err());
}

// ---------------------------------------------------------------------------
// Tests: Full playback flow simulation
// ---------------------------------------------------------------------------

/// Simulate the full TUI playback flow: resolve shot → build request → verify.
#[test]
fn full_playback_flow_simulation() {
    let tmp = TempDir::new().unwrap();
    setup_project_with_sources(tmp.path(), &["src-001"]);

    // 1. Load edit with a time-range shot
    let mut doc = EditDocument::create("test");
    doc.add_shot(
        "src-001",
        ShotRange::Time {
            from_ms: 3000,
            to_ms: 8000,
        },
    )
    .unwrap();

    // 2. Resolve the shot (time ranges don't need transcript)
    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let shot = &resolved[0];
    assert_eq!(shot.source, "src-001");
    assert_eq!(shot.start_ms, 3000);
    assert_eq!(shot.end_ms, 8000);

    // 3. Resolve source path
    let (file, _) = playback::resolve_source_path(&shot.source, tmp.path()).unwrap();
    assert!(file.exists());

    // 4. Build play request
    let req = PlayRequest {
        file,
        start_ms: shot.start_ms,
        end_ms: Some(shot.end_ms),
    };
    assert_eq!(req.start_ms, 3000);
    assert_eq!(req.end_ms, Some(8000));
}
