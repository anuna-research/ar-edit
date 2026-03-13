//! TEST-024: Source playback at timestamp (REQ-024, CON-006)
//!
//! Given a source video with a transcript and index,
//! when a specific word, scene, or timecode is targeted,
//! then the player is launched at the resolved timestamp.
//!
//! This integration test verifies the full pipeline:
//!   source + transcript/index → resolve word/scene/timecode → build PlayRequest.

use ar_edit_core::feedback;
use ar_edit_core::models::*;
use ar_edit_core::playback::{parse_timecode, PlayRequest};
use ar_edit_core::resolve;

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 124500,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 5230,
                text: "Welcome to the interview".into(),
                words: vec![
                    Word {
                        index: 0,
                        text: "Welcome".into(),
                        start_ms: 0,
                        end_ms: 420,
                        confidence: 0.95,
                    },
                    Word {
                        index: 1,
                        text: "to".into(),
                        start_ms: 420,
                        end_ms: 540,
                        confidence: 0.97,
                    },
                    Word {
                        index: 2,
                        text: "the".into(),
                        start_ms: 540,
                        end_ms: 650,
                        confidence: 0.98,
                    },
                    Word {
                        index: 3,
                        text: "interview".into(),
                        start_ms: 650,
                        end_ms: 1200,
                        confidence: 0.96,
                    },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 5230,
                end_ms: 12400,
                text: "Today we discuss climate".into(),
                words: vec![
                    Word {
                        index: 4,
                        text: "Today".into(),
                        start_ms: 5230,
                        end_ms: 5600,
                        confidence: 0.94,
                    },
                    Word {
                        index: 5,
                        text: "we".into(),
                        start_ms: 5600,
                        end_ms: 5750,
                        confidence: 0.99,
                    },
                    Word {
                        index: 6,
                        text: "discuss".into(),
                        start_ms: 5750,
                        end_ms: 6200,
                        confidence: 0.93,
                    },
                    Word {
                        index: 7,
                        text: "climate".into(),
                        start_ms: 6200,
                        end_ms: 6800,
                        confidence: 0.91,
                    },
                ],
            },
        ],
        word_count: 8,
    }
}

fn make_source_index() -> SourceIndex {
    SourceIndex {
        source_id: "src-001".into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms: 124500,
            resolution: (1920, 1080),
            codec: "h264".into(),
            file_size_bytes: 52428800,
        },
        thumbnails: vec![],
        scene_count: 3,
        scenes: vec![
            Scene {
                index: 0,
                start_ms: 0,
                end_ms: 18000,
                thumbnail: "thumbnails/src-001_00m00s.jpg".into(),
                description: Some("Interior office, wide shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 18000,
                end_ms: 45000,
                thumbnail: "thumbnails/src-001_00m18s.jpg".into(),
                description: None,
            },
            Scene {
                index: 2,
                start_ms: 45000,
                end_ms: 90000,
                thumbnail: "thumbnails/src-001_00m45s.jpg".into(),
                description: Some("Close-up interview".into()),
            },
        ],
    }
}

fn make_manifest() -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        name: "test-project".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
        sources: vec![Source {
            id: "src-001".into(),
            path: "sources/src-001.mp4".into(),
            original_filename: "interview.mp4".into(),
            duration_ms: 124500,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            resolution: (1920, 1080),
            frame_rate: 29.97,
            audio_channels: 2,
            audio_sample_rate: 48000,
            added: "2026-02-19T12:01:00Z".parse().unwrap(),
            transcribed: true,
            indexed: true,
        }],
        next_source_id: 2,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    }
}

fn setup_project(dir: &Path) {
    std::fs::create_dir_all(dir.join("sources")).unwrap();
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();

    // Write manifest
    let manifest = make_manifest();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Write transcript
    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();

    // Write index
    let index = make_source_index();
    std::fs::write(
        dir.join("index/src-001.index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Source playback: --at-word resolves to correct timestamp
// ---------------------------------------------------------------------------

#[test]
fn source_playback_at_word_resolves_timestamp() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // Resolve word 4 ("Today") → starts at 5230ms
    let transcript = make_transcript();
    let range = ShotRange::Words { from: 4, to: 4 };
    let (start_ms, _end_ms) = resolve::resolve_range(&range, Some(&transcript), None).unwrap();
    assert_eq!(start_ms, 5230);

    // Build PlayRequest for source playback (no end_ms)
    let req = PlayRequest {
        file: PathBuf::from("/tmp/interview.mp4"),
        start_ms,
        end_ms: None,
        ipc_socket: None, source_id: None,
    };
    assert_eq!(req.start_ms, 5230);
    assert!(req.end_ms.is_none(), "source playback has no end time");
}

#[test]
fn source_playback_at_word_zero() {
    let transcript = make_transcript();
    let range = ShotRange::Words { from: 0, to: 0 };
    let (start_ms, end_ms) = resolve::resolve_range(&range, Some(&transcript), None).unwrap();
    assert_eq!(start_ms, 0);
    assert_eq!(end_ms, 420);
}

#[test]
fn source_playback_at_word_cross_segment() {
    let transcript = make_transcript();
    // Word 3 ("interview") is in segment 0, word 4 ("Today") is in segment 1
    let range = ShotRange::Words { from: 3, to: 4 };
    let (start_ms, end_ms) = resolve::resolve_range(&range, Some(&transcript), None).unwrap();
    assert_eq!(start_ms, 650); // word 3 start
    assert_eq!(end_ms, 5600); // word 4 end
}

// ---------------------------------------------------------------------------
// Source playback: --at-scene resolves to correct timestamp
// ---------------------------------------------------------------------------

#[test]
fn source_playback_at_scene_resolves_timestamp() {
    let index = make_source_index();
    let range = ShotRange::Scenes { from: 1, to: 1 };
    let (start_ms, end_ms) = resolve::resolve_range(&range, None, Some(&index)).unwrap();
    assert_eq!(start_ms, 18000);
    assert_eq!(end_ms, 45000);

    // Build PlayRequest (start at scene 1's beginning)
    let req = PlayRequest {
        file: PathBuf::from("/tmp/interview.mp4"),
        start_ms,
        end_ms: None,
        ipc_socket: None, source_id: None,
    };
    assert_eq!(req.start_ms, 18000);
    assert!(req.end_ms.is_none());
}

#[test]
fn source_playback_at_scene_first_scene() {
    let index = make_source_index();
    let range = ShotRange::Scenes { from: 0, to: 0 };
    let (start_ms, _) = resolve::resolve_range(&range, None, Some(&index)).unwrap();
    assert_eq!(start_ms, 0);
}

#[test]
fn source_playback_at_scene_last_scene() {
    let index = make_source_index();
    let range = ShotRange::Scenes { from: 2, to: 2 };
    let (start_ms, end_ms) = resolve::resolve_range(&range, None, Some(&index)).unwrap();
    assert_eq!(start_ms, 45000);
    assert_eq!(end_ms, 90000);
}

// ---------------------------------------------------------------------------
// Source playback: --at timecode parsing
// ---------------------------------------------------------------------------

#[test]
fn source_playback_at_timecode_seconds() {
    let ms = parse_timecode("90").unwrap();
    assert_eq!(ms, 90000);

    let req = PlayRequest {
        file: PathBuf::from("/tmp/interview.mp4"),
        start_ms: ms,
        end_ms: None,
        ipc_socket: None, source_id: None,
    };
    assert_eq!(req.start_ms, 90000);
}

#[test]
fn source_playback_at_timecode_mm_ss() {
    let ms = parse_timecode("01:30").unwrap();
    assert_eq!(ms, 90000);
}

#[test]
fn source_playback_at_timecode_hh_mm_ss() {
    let ms = parse_timecode("00:01:30").unwrap();
    assert_eq!(ms, 90000);
}

#[test]
fn source_playback_at_timecode_fractional() {
    let ms = parse_timecode("01:30.500").unwrap();
    assert_eq!(ms, 90500);
}

#[test]
fn source_playback_at_timecode_zero() {
    assert_eq!(parse_timecode("0").unwrap(), 0);
    assert_eq!(parse_timecode("00:00").unwrap(), 0);
    assert_eq!(parse_timecode("00:00:00").unwrap(), 0);
}

#[test]
fn source_playback_invalid_timecode_errors() {
    assert!(parse_timecode("abc").is_err());
    assert!(parse_timecode("").is_err());
    assert!(parse_timecode("1:2:3:4").is_err());
}

// ---------------------------------------------------------------------------
// Source playback: player command args (no end time)
// ---------------------------------------------------------------------------

#[test]
fn source_playback_vlc_args_start_only() {
    let start_ms = 18000u64; // 18 seconds
    let start_secs = start_ms as f64 / 1000.0;

    let mut cmd = Command::new("/usr/bin/cvlc");
    cmd.arg("/tmp/interview.mp4");
    cmd.arg(format!("--start-time={start_secs:.3}"));
    // No --stop-time for source playback
    cmd.arg("vlc://quit");

    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_str().unwrap().to_string())
        .collect();

    assert_eq!(args[0], "/tmp/interview.mp4");
    assert_eq!(args[1], "--start-time=18.000");
    assert_eq!(args[2], "vlc://quit");
    // No --stop-time arg
    assert_eq!(args.len(), 3);
}

#[test]
fn source_playback_ffplay_args_start_only() {
    let start_ms = 45000u64; // 45 seconds
    let start_secs = start_ms as f64 / 1000.0;

    let mut cmd = Command::new("/usr/bin/ffplay");
    cmd.arg("-ss").arg(format!("{start_secs:.3}"));
    // No -t duration for source playback
    cmd.arg("-autoexit");
    cmd.arg("/tmp/interview.mp4");

    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_str().unwrap().to_string())
        .collect();

    assert_eq!(args[0], "-ss");
    assert_eq!(args[1], "45.000");
    assert_eq!(args[2], "-autoexit");
    assert_eq!(args[3], "/tmp/interview.mp4");
    // No -t arg
    assert_eq!(args.len(), 4);
}

// ---------------------------------------------------------------------------
// Source playback: resolve from project directory
// ---------------------------------------------------------------------------

#[test]
fn source_playback_resolve_word_from_dir() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let range = ShotRange::Words { from: 5, to: 5 };
    let (start_ms, end_ms) =
        resolve::resolve_range_from_dir(&range, "src-001", &tmp.path().join("transcripts"))
            .unwrap();

    assert_eq!(start_ms, 5600); // word 5 ("we") start
    assert_eq!(end_ms, 5750); // word 5 ("we") end
}

#[test]
fn source_playback_resolve_scene_from_dir() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let range = ShotRange::Scenes { from: 2, to: 2 };
    let (start_ms, end_ms) =
        resolve::resolve_range_from_dir(&range, "src-001", &tmp.path().join("index")).unwrap();

    assert_eq!(start_ms, 45000);
    assert_eq!(end_ms, 90000);
}

// ---------------------------------------------------------------------------
// Source playback: source path from manifest
// ---------------------------------------------------------------------------

#[test]
fn source_playback_resolve_source_path() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let (path, source) =
        ar_edit_core::playback::resolve_source_path("src-001", tmp.path()).unwrap();

    assert_eq!(source.id, "src-001");
    assert_eq!(source.original_filename, "interview.mp4");
    assert_eq!(path, tmp.path().join("sources/src-001.mp4"));
}

#[test]
fn source_playback_resolve_source_path_not_found() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let result = ar_edit_core::playback::resolve_source_path("src-999", tmp.path());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Source playback: feedback after playback (REQ-037)
// ---------------------------------------------------------------------------

#[test]
fn source_playback_feedback_at_word_position() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // Simulate playback paused at 600ms
    let fb = feedback::build_feedback(600, None, "src-001", tmp.path());
    assert_eq!(fb.last_position_ms, 600);
    assert_eq!(fb.shot_id, None); // source playback, no shot
    assert_eq!(fb.source_id, "src-001");
    // word "the" spans 540..650, so word_index should be 2
    assert_eq!(fb.word_index, Some(2));
    // scene 0 spans 0..18000
    assert_eq!(fb.scene_index, Some(0));
}

#[test]
fn source_playback_feedback_serializes_to_json() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let fb = feedback::build_feedback(5300, None, "src-001", tmp.path());
    let json = serde_json::to_value(&fb).unwrap();

    // CON-006 output format
    assert_eq!(json["last_position_ms"], 5300);
    assert!(json["shot_id"].is_null());
    assert_eq!(json["source_id"], "src-001");
    assert!(json.get("word_index").is_some());
    assert!(json.get("scene_index").is_some());
}

// ---------------------------------------------------------------------------
// Source playback: full pipeline (word → timestamp → play request → feedback)
// ---------------------------------------------------------------------------

#[test]
fn source_playback_full_pipeline_at_word() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // 1. Resolve word 6 ("discuss") to timestamp
    let transcript = make_transcript();
    let range = ShotRange::Words { from: 6, to: 6 };
    let (start_ms, _end_ms) = resolve::resolve_range(&range, Some(&transcript), None).unwrap();
    assert_eq!(start_ms, 5750);

    // 2. Resolve source path
    let (source_path, source) =
        ar_edit_core::playback::resolve_source_path("src-001", tmp.path()).unwrap();
    assert_eq!(source.id, "src-001");
    assert!(source_path.to_str().unwrap().contains("src-001.mp4"));

    // 3. Build PlayRequest
    let req = PlayRequest {
        file: source_path,
        start_ms,
        end_ms: None,
        ipc_socket: None, source_id: None,
    };
    assert_eq!(req.start_ms, 5750);
    assert!(req.end_ms.is_none());

    // 4. Build feedback (simulated after playback)
    let fb = feedback::build_feedback(start_ms, None, "src-001", tmp.path());
    assert_eq!(fb.last_position_ms, 5750);
    // At 5750ms, word 5 ("we" 5600..5750) is found first (boundary inclusive),
    // so feedback reports word 5 rather than word 6 which also starts at 5750.
    assert_eq!(fb.word_index, Some(5));
    assert_eq!(fb.scene_index, Some(0)); // still in scene 0 (0..18000)
}

#[test]
fn source_playback_full_pipeline_at_scene() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // 1. Resolve scene 2 to timestamp
    let index = make_source_index();
    let range = ShotRange::Scenes { from: 2, to: 2 };
    let (start_ms, _end_ms) = resolve::resolve_range(&range, None, Some(&index)).unwrap();
    assert_eq!(start_ms, 45000);

    // 2. Build PlayRequest
    let req = PlayRequest {
        file: tmp.path().join("sources/src-001.mp4"),
        start_ms,
        end_ms: None,
        ipc_socket: None, source_id: None,
    };
    assert_eq!(req.start_ms, 45000);

    // 3. Build feedback
    let fb = feedback::build_feedback(start_ms, None, "src-001", tmp.path());
    assert_eq!(fb.last_position_ms, 45000);
    assert_eq!(fb.scene_index, Some(2));
}

#[test]
fn source_playback_full_pipeline_at_timecode() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // 1. Parse timecode "01:30" → 90000ms
    let start_ms = parse_timecode("01:30").unwrap();
    assert_eq!(start_ms, 90000);

    // 2. Build PlayRequest
    let req = PlayRequest {
        file: tmp.path().join("sources/src-001.mp4"),
        start_ms,
        end_ms: None,
        ipc_socket: None, source_id: None,
    };
    assert_eq!(req.start_ms, 90000);

    // 3. Build feedback
    let fb = feedback::build_feedback(start_ms, None, "src-001", tmp.path());
    assert_eq!(fb.last_position_ms, 90000);
    // 90000ms is past all scenes (last scene ends at 90000, exact boundary)
    assert_eq!(fb.scene_index, Some(2));
}
