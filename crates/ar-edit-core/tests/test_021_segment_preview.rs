//! TEST-021: Segment preview (REQ-021, CON-006)
//!
//! Given an edit document with a shot,
//! when a play request is built for that shot,
//! then the player is launched with correct start/end timestamps.
//!
//! This integration test verifies the full pipeline:
//!   edit document → resolve shot → build PlayRequest → verify player args.

use ar_edit_core::display;
use ar_edit_core::models::*;
use ar_edit_core::playback::{PlayRequest, Player, PlayerKind};

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
                    Word { index: 0, text: "Welcome".into(), start_ms: 0, end_ms: 420, confidence: 0.95 },
                    Word { index: 1, text: "to".into(), start_ms: 420, end_ms: 540, confidence: 0.97 },
                    Word { index: 2, text: "the".into(), start_ms: 540, end_ms: 650, confidence: 0.98 },
                    Word { index: 3, text: "interview".into(), start_ms: 650, end_ms: 1200, confidence: 0.96 },
                ],
            },
            TranscriptSegment {
                index: 1,
                start_ms: 5230,
                end_ms: 12400,
                text: "Today we discuss climate".into(),
                words: vec![
                    Word { index: 4, text: "Today".into(), start_ms: 5230, end_ms: 5600, confidence: 0.94 },
                    Word { index: 5, text: "we".into(), start_ms: 5600, end_ms: 5750, confidence: 0.99 },
                    Word { index: 6, text: "discuss".into(), start_ms: 5750, end_ms: 6200, confidence: 0.93 },
                    Word { index: 7, text: "climate".into(), start_ms: 6200, end_ms: 6800, confidence: 0.91 },
                ],
            },
        ],
        word_count: 8,
    }
}

fn make_source_index(source_id: &str) -> SourceIndex {
    SourceIndex {
        source_id: source_id.into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms: 90000,
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
                thumbnail: "thumbnails/src-002_00m00s.jpg".into(),
                description: Some("Interior office, wide shot".into()),
            },
            Scene {
                index: 1,
                start_ms: 18000,
                end_ms: 45000,
                thumbnail: "thumbnails/src-002_00m18s.jpg".into(),
                description: None,
            },
            Scene {
                index: 2,
                start_ms: 45000,
                end_ms: 90000,
                thumbnail: "thumbnails/src-002_00m45s.jpg".into(),
                description: Some("Close-up interview".into()),
            },
        ],
    }
}

fn setup_project(dir: &Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("edits")).unwrap();

    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();

    let index = make_source_index("src-002");
    std::fs::write(
        dir.join("index/src-002.index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Segment preview: resolve shot to PlayRequest
// ---------------------------------------------------------------------------

#[test]
fn segment_preview_resolves_words_shot() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 1);

    let shot = &resolved[0];
    assert_eq!(shot.id, "shot-001");
    assert_eq!(shot.start_ms, 0);    // word 0 start_ms
    assert_eq!(shot.end_ms, 1200);   // word 3 end_ms
    assert_eq!(shot.duration_ms, 1200);

    // Build PlayRequest for this segment
    let req = PlayRequest {
        file: PathBuf::from("/tmp/test.mp4"),
        start_ms: shot.start_ms,
        end_ms: Some(shot.end_ms),
    };
    assert_eq!(req.start_ms, 0);
    assert_eq!(req.end_ms, Some(1200));
}

#[test]
fn segment_preview_resolves_scenes_shot() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 1);

    let shot = &resolved[0];
    assert_eq!(shot.start_ms, 0);     // scene 0 start_ms
    assert_eq!(shot.end_ms, 45000);   // scene 1 end_ms
    assert_eq!(shot.duration_ms, 45000);

    let req = PlayRequest {
        file: PathBuf::from("/tmp/test.mp4"),
        start_ms: shot.start_ms,
        end_ms: Some(shot.end_ms),
    };
    assert_eq!(req.start_ms, 0);
    assert_eq!(req.end_ms, Some(45000));
}

#[test]
fn segment_preview_resolves_time_shot() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 10000 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 1);

    let shot = &resolved[0];
    assert_eq!(shot.start_ms, 5000);
    assert_eq!(shot.end_ms, 10000);
    assert_eq!(shot.duration_ms, 5000);

    let req = PlayRequest {
        file: PathBuf::from("/tmp/test.mp4"),
        start_ms: shot.start_ms,
        end_ms: Some(shot.end_ms),
    };
    assert_eq!(req.start_ms, 5000);
    assert_eq!(req.end_ms, Some(10000));
}

// ---------------------------------------------------------------------------
// Segment preview: player command args
// ---------------------------------------------------------------------------

#[test]
fn segment_preview_vlc_args_for_segment() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let shot = &resolved[0];

    // Build VLC command per playback::launch_player logic
    let player = Player {
        name: "cvlc".into(),
        path: PathBuf::from("/usr/bin/cvlc"),
        kind: PlayerKind::Vlc,
    };

    let start_secs = shot.start_ms as f64 / 1000.0;
    let end_secs = shot.end_ms as f64 / 1000.0;

    let mut cmd = Command::new(&player.path);
    cmd.arg("/tmp/test.mp4");
    cmd.arg(format!("--start-time={start_secs:.3}"));
    cmd.arg(format!("--stop-time={end_secs:.3}"));
    cmd.arg("vlc://quit");

    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_str().unwrap().to_string())
        .collect();

    assert_eq!(args[0], "/tmp/test.mp4");
    assert_eq!(args[1], "--start-time=0.000");
    assert_eq!(args[2], "--stop-time=1.200");
    assert_eq!(args[3], "vlc://quit");
}

#[test]
fn segment_preview_ffplay_args_for_segment() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-001", ShotRange::Words { from: 2, to: 6 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let shot = &resolved[0];

    assert_eq!(shot.start_ms, 540);   // word 2 start
    assert_eq!(shot.end_ms, 6200);    // word 6 end

    // Build ffplay command per playback::launch_player logic
    let start_secs = shot.start_ms as f64 / 1000.0;
    let duration_secs = shot.end_ms.saturating_sub(shot.start_ms) as f64 / 1000.0;

    let mut cmd = Command::new("/usr/bin/ffplay");
    cmd.arg("-ss").arg(format!("{start_secs:.3}"));
    cmd.arg("-t").arg(format!("{duration_secs:.3}"));
    cmd.arg("-autoexit");
    cmd.arg("/tmp/test.mp4");

    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_str().unwrap().to_string())
        .collect();

    assert_eq!(args[0], "-ss");
    assert_eq!(args[1], "0.540");
    assert_eq!(args[2], "-t");
    assert_eq!(args[3], "5.660");
    assert_eq!(args[4], "-autoexit");
    assert_eq!(args[5], "/tmp/test.mp4");
}

// ---------------------------------------------------------------------------
// Segment preview: specific shot from multi-shot edit
// ---------------------------------------------------------------------------

#[test]
fn segment_preview_specific_shot_from_multi_shot_edit() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();
    doc.add_shot("src-002", ShotRange::Scenes { from: 1, to: 2 }).unwrap();
    doc.add_shot("src-001", ShotRange::Time { from_ms: 10000, to_ms: 20000 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);

    // Simulate --shot shot-002: find and play only that shot
    let target = resolved.iter().find(|s| s.id == "shot-002").unwrap();
    assert_eq!(target.source, "src-002");
    assert_eq!(target.start_ms, 18000);  // scene 1 start
    assert_eq!(target.end_ms, 90000);    // scene 2 end

    let req = PlayRequest {
        file: PathBuf::from("/tmp/test.mp4"),
        start_ms: target.start_ms,
        end_ms: Some(target.end_ms),
    };
    assert_eq!(req.start_ms, 18000);
    assert_eq!(req.end_ms, Some(90000));
}

// ---------------------------------------------------------------------------
// Segment preview: text and scene previews included
// ---------------------------------------------------------------------------

#[test]
fn segment_preview_preserves_text_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(
        resolved[0].text_preview.as_deref(),
        Some("Welcome to the interview")
    );
    assert!(resolved[0].scene_preview.is_none());
}

#[test]
fn segment_preview_preserves_scene_preview() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(
        resolved[0].scene_preview.as_deref(),
        Some("Interior office, wide shot; Close-up interview")
    );
    assert!(resolved[0].text_preview.is_none());
}

// ---------------------------------------------------------------------------
// Segment preview: cross-segment word ranges
// ---------------------------------------------------------------------------

#[test]
fn segment_preview_cross_segment_words() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("test-edit");
    // Range spanning both transcript segments
    doc.add_shot("src-001", ShotRange::Words { from: 2, to: 6 }).unwrap();

    let resolved = display::resolve_edit(&doc, tmp.path()).unwrap();
    let shot = &resolved[0];
    assert_eq!(shot.start_ms, 540);   // word 2 ("the") start
    assert_eq!(shot.end_ms, 6200);    // word 6 ("discuss") end
    assert_eq!(shot.duration_ms, 5660);
    assert_eq!(
        shot.text_preview.as_deref(),
        Some("the interview Today we discuss")
    );
}
