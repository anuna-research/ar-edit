use std::path::Path;

use thiserror::Error;

use crate::models::{Scene, ShotRange, SourceIndex, Transcript, Word};

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("word index {0} not found in transcript")]
    WordNotFound(u32),
    #[error("scene index {0} not found in source index")]
    SceneNotFound(u32),
    #[error("transcript required to resolve words range")]
    TranscriptRequired,
    #[error("source index required to resolve scenes range")]
    IndexRequired,
    #[error("failed to read {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse JSON from {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
}

/// Resolve a [`ShotRange`] to absolute `(start_ms, end_ms)`.
///
/// - **Words**: requires `transcript`; returns `word[from].start_ms .. word[to].end_ms`.
/// - **Scenes**: requires `index`; returns `scene[from].start_ms .. scene[to].end_ms`.
/// - **Time**: direct passthrough — no auxiliary data needed.
pub fn resolve_range(
    range: &ShotRange,
    transcript: Option<&Transcript>,
    index: Option<&SourceIndex>,
) -> Result<(u64, u64), ResolveError> {
    match range {
        ShotRange::Time { from_ms, to_ms } => Ok((*from_ms, *to_ms)),
        ShotRange::Words { from, to } => {
            let transcript = transcript.ok_or(ResolveError::TranscriptRequired)?;
            let start_word = find_word(transcript, *from)?;
            let end_word = find_word(transcript, *to)?;
            Ok((start_word.start_ms, end_word.end_ms))
        }
        ShotRange::Scenes { from, to } => {
            let index = index.ok_or(ResolveError::IndexRequired)?;
            let start_scene = find_scene(index, *from)?;
            let end_scene = find_scene(index, *to)?;
            Ok((start_scene.start_ms, end_scene.end_ms))
        }
    }
}

/// Load transcript/index JSON from a project directory and resolve the range.
///
/// Expects transcript at `<dir>/<source_id>.transcript.json`
/// and index at `<dir>/<source_id>.index.json`.
pub fn resolve_range_from_dir(
    range: &ShotRange,
    source_id: &str,
    dir: &Path,
) -> Result<(u64, u64), ResolveError> {
    match range {
        ShotRange::Time { .. } => resolve_range(range, None, None),
        ShotRange::Words { .. } => {
            let path = dir.join(format!("{}.transcript.json", source_id));
            let data = std::fs::read_to_string(&path).map_err(|e| ResolveError::Io {
                path: path.clone(),
                source: e,
            })?;
            let transcript: Transcript =
                serde_json::from_str(&data).map_err(|e| ResolveError::Json {
                    path,
                    source: e,
                })?;
            resolve_range(range, Some(&transcript), None)
        }
        ShotRange::Scenes { .. } => {
            let path = dir.join(format!("{}.index.json", source_id));
            let data = std::fs::read_to_string(&path).map_err(|e| ResolveError::Io {
                path: path.clone(),
                source: e,
            })?;
            let index: SourceIndex =
                serde_json::from_str(&data).map_err(|e| ResolveError::Json {
                    path,
                    source: e,
                })?;
            resolve_range(range, None, Some(&index))
        }
    }
}

fn find_word(transcript: &Transcript, word_index: u32) -> Result<&Word, ResolveError> {
    for segment in &transcript.segments {
        for word in &segment.words {
            if word.index == word_index {
                return Ok(word);
            }
        }
    }
    Err(ResolveError::WordNotFound(word_index))
}

fn find_scene(index: &SourceIndex, scene_index: u32) -> Result<&Scene, ResolveError> {
    index
        .scenes
        .iter()
        .find(|s| s.index == scene_index)
        .ok_or(ResolveError::SceneNotFound(scene_index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

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

    // -- Time passthrough -----------------------------------------------------

    #[test]
    fn time_passthrough() {
        let range = ShotRange::Time { from_ms: 15000, to_ms: 22000 };
        let (start, end) = resolve_range(&range, None, None).unwrap();
        assert_eq!(start, 15000);
        assert_eq!(end, 22000);
    }

    // -- Words resolution -----------------------------------------------------

    #[test]
    fn words_same_segment() {
        let t = make_transcript();
        let range = ShotRange::Words { from: 0, to: 3 };
        let (start, end) = resolve_range(&range, Some(&t), None).unwrap();
        assert_eq!(start, 0);    // word 0 start
        assert_eq!(end, 1200);   // word 3 end
    }

    #[test]
    fn words_cross_segment() {
        let t = make_transcript();
        let range = ShotRange::Words { from: 2, to: 6 };
        let (start, end) = resolve_range(&range, Some(&t), None).unwrap();
        assert_eq!(start, 540);  // word 2 start
        assert_eq!(end, 6200);   // word 6 end
    }

    #[test]
    fn words_single_word() {
        let t = make_transcript();
        let range = ShotRange::Words { from: 4, to: 4 };
        let (start, end) = resolve_range(&range, Some(&t), None).unwrap();
        assert_eq!(start, 5230); // word 4 start
        assert_eq!(end, 5600);   // word 4 end
    }

    #[test]
    fn words_not_found() {
        let t = make_transcript();
        let range = ShotRange::Words { from: 0, to: 99 };
        let err = resolve_range(&range, Some(&t), None).unwrap_err();
        assert!(matches!(err, ResolveError::WordNotFound(99)));
    }

    #[test]
    fn words_no_transcript() {
        let range = ShotRange::Words { from: 0, to: 5 };
        let err = resolve_range(&range, None, None).unwrap_err();
        assert!(matches!(err, ResolveError::TranscriptRequired));
    }

    // -- Scenes resolution ----------------------------------------------------

    #[test]
    fn scenes_single_scene() {
        let idx = make_source_index();
        let range = ShotRange::Scenes { from: 1, to: 1 };
        let (start, end) = resolve_range(&range, None, Some(&idx)).unwrap();
        assert_eq!(start, 18000);
        assert_eq!(end, 45000);
    }

    #[test]
    fn scenes_multi_scene() {
        let idx = make_source_index();
        let range = ShotRange::Scenes { from: 0, to: 2 };
        let (start, end) = resolve_range(&range, None, Some(&idx)).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 90000);
    }

    #[test]
    fn scenes_not_found() {
        let idx = make_source_index();
        let range = ShotRange::Scenes { from: 0, to: 10 };
        let err = resolve_range(&range, None, Some(&idx)).unwrap_err();
        assert!(matches!(err, ResolveError::SceneNotFound(10)));
    }

    #[test]
    fn scenes_no_index() {
        let range = ShotRange::Scenes { from: 0, to: 2 };
        let err = resolve_range(&range, None, None).unwrap_err();
        assert!(matches!(err, ResolveError::IndexRequired));
    }

    // -- File-based resolution ------------------------------------------------

    #[test]
    fn from_dir_time_no_files_needed() {
        let range = ShotRange::Time { from_ms: 1000, to_ms: 2000 };
        // Should succeed even with a non-existent dir since Time needs no files.
        let (start, end) = resolve_range_from_dir(&range, "src-001", Path::new("/nonexistent")).unwrap();
        assert_eq!(start, 1000);
        assert_eq!(end, 2000);
    }

    #[test]
    fn from_dir_words_loads_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = make_transcript();
        let path = dir.path().join("src-001.transcript.json");
        std::fs::write(&path, serde_json::to_string(&transcript).unwrap()).unwrap();

        let range = ShotRange::Words { from: 0, to: 7 };
        let (start, end) = resolve_range_from_dir(&range, "src-001", dir.path()).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 6800);
    }

    #[test]
    fn from_dir_scenes_loads_index() {
        let dir = tempfile::tempdir().unwrap();
        let index = make_source_index();
        let path = dir.path().join("src-001.index.json");
        std::fs::write(&path, serde_json::to_string(&index).unwrap()).unwrap();

        let range = ShotRange::Scenes { from: 0, to: 1 };
        let (start, end) = resolve_range_from_dir(&range, "src-001", dir.path()).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 45000);
    }

    #[test]
    fn from_dir_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let range = ShotRange::Words { from: 0, to: 5 };
        let err = resolve_range_from_dir(&range, "src-001", dir.path()).unwrap_err();
        assert!(matches!(err, ResolveError::Io { .. }));
    }
}
