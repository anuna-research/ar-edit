use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{SourceIndex, Transcript};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum FeedbackError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse JSON from {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
}

// ---------------------------------------------------------------------------
// Playback feedback (CON-006, REQ-037)
// ---------------------------------------------------------------------------

/// Structured feedback output after playback exits or is interrupted.
///
/// Designed for an LLM agent to receive as context for editing instructions.
/// Output via `--json` on the play command after the player subprocess exits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackFeedback {
    /// Last known playback position in milliseconds.
    pub last_position_ms: u64,
    /// Shot ID at the playback position (null for source-only playback).
    pub shot_id: Option<String>,
    /// Source ID of the video being played.
    pub source_id: String,
    /// Word index at (or nearest before) the playback position.
    pub word_index: Option<u32>,
    /// Scene index at the playback position.
    pub scene_index: Option<u32>,
}

/// Build playback feedback by resolving position against transcript/index data.
///
/// Loads transcript from `<project_dir>/transcripts/<source_id>.transcript.json`
/// and index from `<project_dir>/index/<source_id>.index.json` if available.
/// Missing files are not errors — the corresponding field is set to `None`.
pub fn build_feedback(
    last_position_ms: u64,
    shot_id: Option<&str>,
    source_id: &str,
    project_dir: &Path,
) -> PlaybackFeedback {
    let transcript = load_transcript(project_dir, source_id);
    let index = load_index(project_dir, source_id);

    let word_index = transcript
        .as_ref()
        .and_then(|t| find_word_at_position(t, last_position_ms));

    let scene_index = index
        .as_ref()
        .and_then(|idx| find_scene_at_position(idx, last_position_ms));

    PlaybackFeedback {
        last_position_ms,
        shot_id: shot_id.map(|s| s.to_string()),
        source_id: source_id.to_string(),
        word_index,
        scene_index,
    }
}

// ---------------------------------------------------------------------------
// Full edit feedback
// ---------------------------------------------------------------------------

/// Build feedback for full edit playback, resolving the position against
/// the timeline of resolved shots to find which shot was being viewed.
///
/// `shot_timings` is a list of `(shot_id, source_id, start_ms, end_ms)` in
/// timeline order (cumulative offsets into the preview).
pub fn build_edit_feedback(
    last_position_ms: u64,
    shot_timings: &[(String, String, u64, u64)],
    project_dir: &Path,
) -> PlaybackFeedback {
    // Find which shot contains last_position_ms in the timeline
    let hit = shot_timings
        .iter()
        .find(|(_, _, start, end)| last_position_ms >= *start && last_position_ms < *end);

    // Fall back to the last shot if position is past all shots
    let hit = hit.or_else(|| shot_timings.last());

    match hit {
        Some((shot_id, source_id, start, _end)) => {
            // Compute position within the source: offset from shot start in timeline
            let offset_in_shot = last_position_ms.saturating_sub(*start);

            // For source-relative position we'd need the shot's source start_ms,
            // but for the feedback we report the timeline position directly.
            let _ = offset_in_shot;

            build_feedback(last_position_ms, Some(shot_id), source_id, project_dir)
        }
        None => {
            // No shots — return feedback with no shot context
            PlaybackFeedback {
                last_position_ms,
                shot_id: None,
                source_id: String::new(),
                word_index: None,
                scene_index: None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Position lookup helpers
// ---------------------------------------------------------------------------

/// Find the word index at or nearest before the given position in milliseconds.
fn find_word_at_position(transcript: &Transcript, position_ms: u64) -> Option<u32> {
    let mut best: Option<(u32, u64)> = None; // (index, start_ms)

    for segment in &transcript.segments {
        for word in &segment.words {
            // Exact hit: position falls within word boundaries
            if position_ms >= word.start_ms && position_ms <= word.end_ms {
                return Some(word.index);
            }
            // Track the nearest word that starts at or before the position
            if word.start_ms <= position_ms {
                match best {
                    Some((_, best_start)) if word.start_ms > best_start => {
                        best = Some((word.index, word.start_ms));
                    }
                    None => {
                        best = Some((word.index, word.start_ms));
                    }
                    _ => {}
                }
            }
        }
    }

    best.map(|(idx, _)| idx)
}

/// Find the scene index at the given position in milliseconds.
fn find_scene_at_position(index: &SourceIndex, position_ms: u64) -> Option<u32> {
    for scene in &index.scenes {
        if position_ms >= scene.start_ms && position_ms < scene.end_ms {
            return Some(scene.index);
        }
    }

    // If position is exactly at the end of the last scene, return that scene
    if let Some(last) = index.scenes.last() {
        if position_ms == last.end_ms {
            return Some(last.index);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// File loading
// ---------------------------------------------------------------------------

fn load_transcript(project_dir: &Path, source_id: &str) -> Option<Transcript> {
    let path = project_dir
        .join("transcripts")
        .join(format!("{source_id}.transcript.json"));
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn load_index(project_dir: &Path, source_id: &str) -> Option<SourceIndex> {
    let path = project_dir
        .join("index")
        .join(format!("{source_id}.index.json"));
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use tempfile::TempDir;

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

    fn setup_project(dir: &Path) {
        std::fs::create_dir_all(dir.join("transcripts")).unwrap();
        std::fs::create_dir_all(dir.join("index")).unwrap();

        let transcript = make_transcript();
        std::fs::write(
            dir.join("transcripts/src-001.transcript.json"),
            serde_json::to_string(&transcript).unwrap(),
        )
        .unwrap();

        let index = make_source_index();
        std::fs::write(
            dir.join("index/src-001.index.json"),
            serde_json::to_string(&index).unwrap(),
        )
        .unwrap();
    }

    // -- find_word_at_position ------------------------------------------------

    #[test]
    fn word_exact_hit() {
        let t = make_transcript();
        assert_eq!(find_word_at_position(&t, 500), Some(1)); // within "to" (420..540)
    }

    #[test]
    fn word_at_start_boundary() {
        let t = make_transcript();
        assert_eq!(find_word_at_position(&t, 0), Some(0)); // at start of "Welcome"
    }

    #[test]
    fn word_at_end_boundary() {
        let t = make_transcript();
        // 420 is both end of "Welcome" (0..420) and start of "to" (420..540);
        // "Welcome" is found first during iteration.
        assert_eq!(find_word_at_position(&t, 420), Some(0));
    }

    #[test]
    fn word_in_gap_between_words() {
        let t = make_transcript();
        // Position between segments (1200..5230) — nearest word before is index 3
        assert_eq!(find_word_at_position(&t, 3000), Some(3));
    }

    #[test]
    fn word_before_all_words() {
        // Position 0 is the start of word 0, so it should match
        let t = make_transcript();
        assert_eq!(find_word_at_position(&t, 0), Some(0));
    }

    #[test]
    fn word_after_all_words() {
        let t = make_transcript();
        assert_eq!(find_word_at_position(&t, 100000), Some(7)); // last word
    }

    #[test]
    fn word_cross_segment() {
        let t = make_transcript();
        // 5600 is both end of "Today" (5230..5600) and start of "we" (5600..5750);
        // "Today" is found first during iteration.
        assert_eq!(find_word_at_position(&t, 5600), Some(4));
    }

    // -- find_scene_at_position -----------------------------------------------

    #[test]
    fn scene_exact_hit() {
        let idx = make_source_index();
        assert_eq!(find_scene_at_position(&idx, 5000), Some(0)); // within scene 0 (0..18000)
    }

    #[test]
    fn scene_at_boundary() {
        let idx = make_source_index();
        assert_eq!(find_scene_at_position(&idx, 18000), Some(1)); // start of scene 1
    }

    #[test]
    fn scene_at_end() {
        let idx = make_source_index();
        assert_eq!(find_scene_at_position(&idx, 90000), Some(2)); // end of last scene
    }

    #[test]
    fn scene_beyond_end() {
        let idx = make_source_index();
        assert_eq!(find_scene_at_position(&idx, 100000), None);
    }

    #[test]
    fn scene_at_zero() {
        let idx = make_source_index();
        assert_eq!(find_scene_at_position(&idx, 0), Some(0));
    }

    // -- build_feedback -------------------------------------------------------

    #[test]
    fn feedback_with_transcript_and_index() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let fb = build_feedback(600, Some("shot-001"), "src-001", tmp.path());
        assert_eq!(fb.last_position_ms, 600);
        assert_eq!(fb.shot_id.as_deref(), Some("shot-001"));
        assert_eq!(fb.source_id, "src-001");
        assert_eq!(fb.word_index, Some(2)); // "the" at 540..650
        assert_eq!(fb.scene_index, Some(0)); // scene 0 at 0..18000
    }

    #[test]
    fn feedback_without_shot_id() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let fb = build_feedback(5300, None, "src-001", tmp.path());
        assert_eq!(fb.shot_id, None);
        assert_eq!(fb.word_index, Some(4)); // "Today" at 5230..5600
        assert_eq!(fb.scene_index, Some(0));
    }

    #[test]
    fn feedback_missing_files() {
        let tmp = TempDir::new().unwrap();
        // No transcript or index files
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("index")).unwrap();

        let fb = build_feedback(5000, Some("shot-001"), "src-999", tmp.path());
        assert_eq!(fb.last_position_ms, 5000);
        assert_eq!(fb.word_index, None);
        assert_eq!(fb.scene_index, None);
    }

    #[test]
    fn feedback_serialization() {
        let fb = PlaybackFeedback {
            last_position_ms: 34500,
            shot_id: Some("shot-003".into()),
            source_id: "src-002".into(),
            word_index: Some(85),
            scene_index: Some(2),
        };

        let json = serde_json::to_value(&fb).unwrap();
        assert_eq!(json["last_position_ms"], 34500);
        assert_eq!(json["shot_id"], "shot-003");
        assert_eq!(json["source_id"], "src-002");
        assert_eq!(json["word_index"], 85);
        assert_eq!(json["scene_index"], 2);

        let back: PlaybackFeedback = serde_json::from_value(json).unwrap();
        assert_eq!(back.last_position_ms, 34500);
    }

    #[test]
    fn feedback_serialization_null_fields() {
        let fb = PlaybackFeedback {
            last_position_ms: 5000,
            shot_id: None,
            source_id: "src-001".into(),
            word_index: None,
            scene_index: None,
        };

        let json = serde_json::to_value(&fb).unwrap();
        assert!(json["shot_id"].is_null());
        assert!(json["word_index"].is_null());
        assert!(json["scene_index"].is_null());
    }

    // -- build_edit_feedback --------------------------------------------------

    #[test]
    fn edit_feedback_finds_correct_shot() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let timings = vec![
            ("shot-001".into(), "src-001".into(), 0u64, 5000u64),
            ("shot-002".into(), "src-001".into(), 5000, 12000),
        ];

        let fb = build_edit_feedback(6000, &timings, tmp.path());
        assert_eq!(fb.shot_id.as_deref(), Some("shot-002"));
        assert_eq!(fb.source_id, "src-001");
        assert_eq!(fb.last_position_ms, 6000);
    }

    #[test]
    fn edit_feedback_falls_back_to_last_shot() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let timings = vec![("shot-001".into(), "src-001".into(), 0u64, 5000u64)];

        let fb = build_edit_feedback(50000, &timings, tmp.path());
        assert_eq!(fb.shot_id.as_deref(), Some("shot-001"));
    }

    #[test]
    fn edit_feedback_empty_timings() {
        let tmp = TempDir::new().unwrap();

        let fb = build_edit_feedback(5000, &[], tmp.path());
        assert_eq!(fb.shot_id, None);
        assert!(fb.source_id.is_empty());
    }
}
