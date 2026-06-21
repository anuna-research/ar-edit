use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::models::{EditDocument, Manifest, ShotRange, SourceIndex, Transcript};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationError {
    pub shot_id: String,
    pub error: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate a single source + range combination against the project manifest
/// and on-disk transcripts / scene indexes.
///
/// Returns a list of human-readable error strings (empty if valid).
/// Does **not** check range ordering or zero duration — those are enforced
/// by [`crate::edit::validate_range`].
pub fn validate_shot_source(
    source_id: &str,
    range: &ShotRange,
    manifest: &Manifest,
    project_dir: &Path,
) -> Vec<String> {
    let mut errors = Vec::new();

    let source = match manifest.sources.iter().find(|s| s.id == source_id) {
        Some(s) => s,
        None => {
            errors.push(format!("source '{}' not found in project", source_id));
            return errors;
        }
    };

    match range {
        ShotRange::Words { from, to } => {
            if !source.transcribed {
                errors.push(format!(
                    "source '{}' has no transcript; cannot use word range",
                    source.id
                ));
            } else if let Some(t) = load_transcript(project_dir, &source.id) {
                if *from >= t.word_count {
                    errors.push(format!(
                        "word index {} exceeds word_count {} for {}",
                        from, t.word_count, source.id
                    ));
                }
                if *to >= t.word_count {
                    errors.push(format!(
                        "word index {} exceeds word_count {} for {}",
                        to, t.word_count, source.id
                    ));
                }
            }
        }
        ShotRange::Scenes { from, to } => {
            if !source.indexed {
                errors.push(format!(
                    "source '{}' has no scene index; cannot use scene range",
                    source.id
                ));
            } else if let Some(idx) = load_index(project_dir, &source.id) {
                if *from >= idx.scene_count {
                    errors.push(format!(
                        "scene index {} exceeds scene_count {} for {}",
                        from, idx.scene_count, source.id
                    ));
                }
                if *to >= idx.scene_count {
                    errors.push(format!(
                        "scene index {} exceeds scene_count {} for {}",
                        to, idx.scene_count, source.id
                    ));
                }
            }
        }
        ShotRange::Time { from_ms, to_ms } => {
            if *from_ms > source.duration_ms {
                errors.push(format!(
                    "time {}ms exceeds duration {}ms for {}",
                    from_ms, source.duration_ms, source.id
                ));
            }
            if *to_ms > source.duration_ms {
                errors.push(format!(
                    "time {}ms exceeds duration {}ms for {}",
                    to_ms, source.duration_ms, source.id
                ));
            }
        }
    }

    errors
}

/// Validate every shot in `doc.snapshot` against the project manifest and
/// on-disk transcripts / scene indexes.
///
/// Checks implemented (CON-005):
///
/// 1. Source exists in manifest
/// 2. Transcript exists (for word ranges)
/// 3. Index exists (for scene ranges)
/// 4. Word index in bounds
/// 5. Scene index in bounds
/// 6. Time in bounds
/// 7. Range order (from <= to)
/// 8. Non-zero duration (from < to)
pub fn validate(doc: &EditDocument, manifest: &Manifest, project_dir: &Path) -> ValidationResult {
    let mut errors = Vec::new();

    // Cache loaded transcripts and indexes to avoid re-reading per shot.
    let mut transcripts: HashMap<String, Option<Transcript>> = HashMap::new();
    let mut indexes: HashMap<String, Option<SourceIndex>> = HashMap::new();

    for shot in &doc.snapshot.shots {
        // -- Check 1: source exists in manifest -----------------------------------
        let source = match manifest.sources.iter().find(|s| s.id == shot.source) {
            Some(s) => s,
            None => {
                errors.push(ValidationError {
                    shot_id: shot.id.clone(),
                    error: format!("source '{}' not found in project", shot.source),
                });
                continue; // Cannot do further checks without source metadata
            }
        };

        // -- Checks 7 & 8: range order and non-zero duration ---------------------
        match &shot.range {
            ShotRange::Words { from, to } => {
                if from > to {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!("from ({}) must be <= to ({}) in {}", from, to, shot.id),
                    });
                } else if from == to {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!("{} has zero duration", shot.id),
                    });
                }
            }
            ShotRange::Scenes { from, to } => {
                if from > to {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!("from ({}) must be <= to ({}) in {}", from, to, shot.id),
                    });
                } else if from == to {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!("{} has zero duration", shot.id),
                    });
                }
            }
            ShotRange::Time { from_ms, to_ms } => {
                if from_ms > to_ms {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!(
                            "from ({}) must be <= to ({}) in {}",
                            from_ms, to_ms, shot.id
                        ),
                    });
                } else if from_ms == to_ms {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!("{} has zero duration", shot.id),
                    });
                }
            }
        }

        // -- Range-specific checks ------------------------------------------------
        match &shot.range {
            ShotRange::Words { from, to } => {
                // Check 2: transcript exists
                if !source.transcribed {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!(
                            "source '{}' has no transcript; cannot use word range",
                            source.id
                        ),
                    });
                } else {
                    // Check 4: word indices in bounds
                    let transcript = transcripts
                        .entry(source.id.clone())
                        .or_insert_with(|| load_transcript(project_dir, &source.id));

                    if let Some(t) = transcript {
                        if *from >= t.word_count {
                            errors.push(ValidationError {
                                shot_id: shot.id.clone(),
                                error: format!(
                                    "word index {} exceeds word_count {} for {}",
                                    from, t.word_count, source.id
                                ),
                            });
                        }
                        if *to >= t.word_count {
                            errors.push(ValidationError {
                                shot_id: shot.id.clone(),
                                error: format!(
                                    "word index {} exceeds word_count {} for {}",
                                    to, t.word_count, source.id
                                ),
                            });
                        }
                    }
                }
            }
            ShotRange::Scenes { from, to } => {
                // Check 3: index exists
                if !source.indexed {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!(
                            "source '{}' has no scene index; cannot use scene range",
                            source.id
                        ),
                    });
                } else {
                    // Check 5: scene indices in bounds
                    let index = indexes
                        .entry(source.id.clone())
                        .or_insert_with(|| load_index(project_dir, &source.id));

                    if let Some(idx) = index {
                        if *from >= idx.scene_count {
                            errors.push(ValidationError {
                                shot_id: shot.id.clone(),
                                error: format!(
                                    "scene index {} exceeds scene_count {} for {}",
                                    from, idx.scene_count, source.id
                                ),
                            });
                        }
                        if *to >= idx.scene_count {
                            errors.push(ValidationError {
                                shot_id: shot.id.clone(),
                                error: format!(
                                    "scene index {} exceeds scene_count {} for {}",
                                    to, idx.scene_count, source.id
                                ),
                            });
                        }
                    }
                }
            }
            ShotRange::Time { from_ms, to_ms } => {
                // Check 6: time in bounds
                if *from_ms > source.duration_ms {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!(
                            "time {}ms exceeds duration {}ms for {}",
                            from_ms, source.duration_ms, source.id
                        ),
                    });
                }
                if *to_ms > source.duration_ms {
                    errors.push(ValidationError {
                        shot_id: shot.id.clone(),
                        error: format!(
                            "time {}ms exceeds duration {}ms for {}",
                            to_ms, source.duration_ms, source.id
                        ),
                    });
                }
            }
        }
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
    }
}

// ---------------------------------------------------------------------------
// File loaders
// ---------------------------------------------------------------------------

fn load_transcript(project_dir: &Path, source_id: &str) -> Option<Transcript> {
    let path = project_dir
        .join("transcripts")
        .join(format!("{}.transcript.json", source_id));
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn load_index(project_dir: &Path, source_id: &str) -> Option<SourceIndex> {
    let path = project_dir
        .join("index")
        .join(format!("{}.index.json", source_id));
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
    use chrono::Utc;
    use tempfile::TempDir;

    // -- Helpers --------------------------------------------------------------

    fn make_manifest(sources: Vec<Source>) -> Manifest {
        Manifest {
            version: "1.0.0".into(),
            name: "test-project".into(),
            created: Utc::now(),
            sources,
            next_source_id: 4,
            defaults: Defaults {
                whisper_model: "base".into(),
                thumbnail_interval_sec: 10,
                render_codec: "h264".into(),
                render_container: "mp4".into(),
            },
        }
    }

    fn make_source(id: &str, duration_ms: u64, transcribed: bool, indexed: bool) -> Source {
        Source {
            id: id.into(),
            path: format!("sources/{}.mp4", id).into(),
            original_filename: format!("{}.mp4", id),
            duration_ms,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            resolution: (1920, 1080),
            frame_rate: 29.97,
            audio_channels: 2,
            audio_sample_rate: 48000,
            added: Utc::now(),
            transcribed,
            indexed,
        }
    }

    fn make_edit_doc(shots: Vec<Shot>) -> EditDocument {
        EditDocument {
            name: "test-edit".into(),
            created: Utc::now(),
            next_shot_id: (shots.len() as u32) + 1,
            head: if shots.is_empty() { -1 } else { 0 },
            ops: vec![],
            snapshot: EditSnapshot { shots },
        }
    }

    fn make_transcript(source_id: &str, word_count: u32) -> Transcript {
        let mut words = Vec::new();
        for i in 0..word_count {
            words.push(Word {
                index: i,
                text: format!("word{}", i),
                start_ms: (i as u64) * 100,
                end_ms: (i as u64) * 100 + 80,
                confidence: 0.95,
            });
        }
        Transcript {
            source_id: source_id.into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms: (word_count as u64) * 100,
            segments: vec![TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: (word_count as u64) * 100,
                text: "test segment".into(),
                words,
            }],
            word_count,
        }
    }

    fn make_source_index(source_id: &str, scene_count: u32) -> SourceIndex {
        let scenes: Vec<Scene> = (0..scene_count)
            .map(|i| Scene {
                index: i,
                start_ms: (i as u64) * 10000,
                end_ms: ((i + 1) as u64) * 10000,
                thumbnail: format!("thumbnails/{}_scene{}.jpg", source_id, i).into(),
                description: None,
            })
            .collect();
        SourceIndex {
            source_id: source_id.into(),
            indexed_at: Utc::now(),
            metadata: SourceMetadata {
                duration_ms: (scene_count as u64) * 10000,
                resolution: (1920, 1080),
                codec: "h264".into(),
                file_size_bytes: 52428800,
            },
            thumbnails: vec![],
            scene_count,
            scenes,
        }
    }

    fn write_transcript(dir: &Path, transcript: &Transcript) {
        let path = dir
            .join("transcripts")
            .join(format!("{}.transcript.json", transcript.source_id));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(transcript).unwrap()).unwrap();
    }

    fn write_index(dir: &Path, index: &SourceIndex) {
        let path = dir
            .join("index")
            .join(format!("{}.index.json", index.source_id));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(index).unwrap()).unwrap();
    }

    // -- Valid edit document ---------------------------------------------------

    #[test]
    fn valid_edit_no_errors() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, true, true);
        let manifest = make_manifest(vec![src]);
        let transcript = make_transcript("src-001", 487);
        let index = make_source_index("src-001", 4);
        write_transcript(tmp.path(), &transcript);
        write_index(tmp.path(), &index);

        let doc = make_edit_doc(vec![
            Shot {
                author: String::new(),
                id: "shot-001".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 52 },
                notes: vec![],
            },
            Shot {
                author: String::new(),
                id: "shot-002".into(),
                source: "src-001".into(),
                range: ShotRange::Scenes { from: 0, to: 2 },
                notes: vec![],
            },
            Shot {
                author: String::new(),
                id: "shot-003".into(),
                source: "src-001".into(),
                range: ShotRange::Time {
                    from_ms: 15000,
                    to_ms: 22000,
                },
                notes: vec![],
            },
        ]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn empty_edit_is_valid() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![]);
        let doc = make_edit_doc(vec![]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    // -- Check 1: source exists -----------------------------------------------

    #[test]
    fn source_not_found() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![]);
        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-999".into(),
            range: ShotRange::Words { from: 0, to: 10 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            result.errors[0].error,
            "source 'src-999' not found in project"
        );
    }

    // -- Check 2: transcript exists -------------------------------------------

    #[test]
    fn word_range_on_non_transcribed_source() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, false);
        let manifest = make_manifest(vec![src]);
        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 52 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"source 'src-001' has no transcript; cannot use word range"));
    }

    // -- Check 3: index exists ------------------------------------------------

    #[test]
    fn scene_range_on_non_indexed_source() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, false);
        let manifest = make_manifest(vec![src]);
        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Scenes { from: 0, to: 2 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"source 'src-001' has no scene index; cannot use scene range"));
    }

    // -- Check 4: word index in bounds ----------------------------------------

    #[test]
    fn word_index_exceeds_word_count() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, true, false);
        let manifest = make_manifest(vec![src]);
        let transcript = make_transcript("src-001", 487);
        write_transcript(tmp.path(), &transcript);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 500 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"word index 500 exceeds word_count 487 for src-001"));
    }

    #[test]
    fn word_index_at_word_count_boundary() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, true, false);
        let manifest = make_manifest(vec![src]);
        let transcript = make_transcript("src-001", 375);
        write_transcript(tmp.path(), &transcript);

        // Index 375 with word_count=375 → out of bounds (valid: 0-374)
        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 375 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"word index 375 exceeds word_count 375 for src-001"));
    }

    #[test]
    fn word_index_from_also_checked() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, true, false);
        let manifest = make_manifest(vec![src]);
        let transcript = make_transcript("src-001", 100);
        write_transcript(tmp.path(), &transcript);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 200, to: 300 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        // Both from and to exceed word_count
        let word_errors: Vec<&ValidationError> = result
            .errors
            .iter()
            .filter(|e| e.error.contains("word index"))
            .collect();
        assert_eq!(word_errors.len(), 2);
    }

    // -- Check 5: scene index in bounds ---------------------------------------

    #[test]
    fn scene_index_exceeds_scene_count() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, true);
        let manifest = make_manifest(vec![src]);
        let index = make_source_index("src-001", 4);
        write_index(tmp.path(), &index);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Scenes { from: 0, to: 10 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"scene index 10 exceeds scene_count 4 for src-001"));
    }

    // -- Check 6: time in bounds ----------------------------------------------

    #[test]
    fn time_exceeds_duration() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, false);
        let manifest = make_manifest(vec![src]);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 200000,
            },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"time 200000ms exceeds duration 124500ms for src-001"));
    }

    #[test]
    fn time_at_duration_is_valid() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, false);
        let manifest = make_manifest(vec![src]);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 10000,
                to_ms: 124500,
            },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(result.valid);
    }

    // -- Check 7: range order -------------------------------------------------

    #[test]
    fn words_from_greater_than_to() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, true, false);
        let manifest = make_manifest(vec![src]);
        let transcript = make_transcript("src-001", 487);
        write_transcript(tmp.path(), &transcript);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 52, to: 10 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"from (52) must be <= to (10) in shot-001"));
    }

    #[test]
    fn scenes_from_greater_than_to() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, true);
        let manifest = make_manifest(vec![src]);
        let index = make_source_index("src-001", 4);
        write_index(tmp.path(), &index);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Scenes { from: 3, to: 1 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"from (3) must be <= to (1) in shot-001"));
    }

    #[test]
    fn time_from_greater_than_to() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, false);
        let manifest = make_manifest(vec![src]);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 22000,
                to_ms: 15000,
            },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"from (22000) must be <= to (15000) in shot-001"));
    }

    // -- Check 8: non-zero duration -------------------------------------------

    #[test]
    fn words_zero_duration() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, true, false);
        let manifest = make_manifest(vec![src]);
        let transcript = make_transcript("src-001", 487);
        write_transcript(tmp.path(), &transcript);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 5, to: 5 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"shot-001 has zero duration"));
    }

    #[test]
    fn scenes_zero_duration() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, true);
        let manifest = make_manifest(vec![src]);
        let index = make_source_index("src-001", 4);
        write_index(tmp.path(), &index);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Scenes { from: 2, to: 2 },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"shot-001 has zero duration"));
    }

    #[test]
    fn time_zero_duration() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 124500, false, false);
        let manifest = make_manifest(vec![src]);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 5000,
                to_ms: 5000,
            },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let errors: Vec<&str> = result.errors.iter().map(|e| e.error.as_str()).collect();
        assert!(errors.contains(&"shot-001 has zero duration"));
    }

    // -- Multiple errors per shot ---------------------------------------------

    #[test]
    fn multiple_errors_collected() {
        let tmp = TempDir::new().unwrap();
        let src1 = make_source("src-001", 124500, true, false);
        let manifest = make_manifest(vec![src1]);
        let transcript = make_transcript("src-001", 100);
        write_transcript(tmp.path(), &transcript);

        let doc = make_edit_doc(vec![
            Shot {
                author: String::new(),
                id: "shot-001".into(),
                source: "src-999".into(), // source not found
                range: ShotRange::Words { from: 0, to: 10 },
                notes: vec![],
            },
            Shot {
                author: String::new(),
                id: "shot-002".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 500 }, // word out of bounds
                notes: vec![],
            },
        ]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        assert!(result.errors.len() >= 2);

        let shot_ids: Vec<&str> = result.errors.iter().map(|e| e.shot_id.as_str()).collect();
        assert!(shot_ids.contains(&"shot-001"));
        assert!(shot_ids.contains(&"shot-002"));
    }

    // -- Serialization --------------------------------------------------------

    #[test]
    fn validation_result_json_valid() {
        let result = ValidationResult {
            valid: true,
            errors: vec![],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json, serde_json::json!({ "valid": true, "errors": [] }));
    }

    #[test]
    fn validation_result_json_invalid() {
        let result = ValidationResult {
            valid: false,
            errors: vec![ValidationError {
                shot_id: "shot-003".into(),
                error: "word index 380 exceeds word_count 375 for src-001".into(),
            }],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["valid"], false);
        assert_eq!(json["errors"][0]["shot_id"], "shot-003");
        assert_eq!(
            json["errors"][0]["error"],
            "word index 380 exceeds word_count 375 for src-001"
        );
    }

    // -- Multi-source edit document -------------------------------------------

    #[test]
    fn multi_source_valid() {
        let tmp = TempDir::new().unwrap();
        let src1 = make_source("src-001", 124500, true, true);
        let src2 = make_source("src-002", 60000, false, true);
        let manifest = make_manifest(vec![src1, src2]);

        let transcript = make_transcript("src-001", 487);
        let index1 = make_source_index("src-001", 4);
        let index2 = make_source_index("src-002", 3);
        write_transcript(tmp.path(), &transcript);
        write_index(tmp.path(), &index1);
        write_index(tmp.path(), &index2);

        let doc = make_edit_doc(vec![
            Shot {
                author: String::new(),
                id: "shot-001".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 52 },
                notes: vec![],
            },
            Shot {
                author: String::new(),
                id: "shot-002".into(),
                source: "src-002".into(),
                range: ShotRange::Scenes { from: 0, to: 2 },
                notes: vec![],
            },
            Shot {
                author: String::new(),
                id: "shot-003".into(),
                source: "src-002".into(),
                range: ShotRange::Time {
                    from_ms: 15000,
                    to_ms: 22000,
                },
                notes: vec![],
            },
        ]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(result.valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn time_from_exceeds_duration() {
        let tmp = TempDir::new().unwrap();
        let src = make_source("src-001", 50000, false, false);
        let manifest = make_manifest(vec![src]);

        let doc = make_edit_doc(vec![Shot {
            author: String::new(),
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 60000,
                to_ms: 70000,
            },
            notes: vec![],
        }]);

        let result = validate(&doc, &manifest, tmp.path());
        assert!(!result.valid);
        let time_errors: Vec<&ValidationError> = result
            .errors
            .iter()
            .filter(|e| e.error.contains("exceeds duration"))
            .collect();
        assert_eq!(time_errors.len(), 2); // both from and to exceed
    }
}
