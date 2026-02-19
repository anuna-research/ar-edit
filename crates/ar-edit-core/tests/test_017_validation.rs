//! TEST-017: Validation catches all error types
//!
//! Verifies that `validate::validate()` catches all 8 error categories from
//! CON-005: missing source, missing transcript, missing index, word out of
//! bounds, scene out of bounds, time out of bounds, reversed range, zero
//! duration.

use ar_edit_core::models::*;
use ar_edit_core::validate::validate;
use chrono::Utc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        path: format!("sources/{id}.mp4").into(),
        original_filename: format!("{id}.mp4"),
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
    let words: Vec<Word> = (0..word_count)
        .map(|i| Word {
            index: i,
            text: format!("word{i}"),
            start_ms: (i as u64) * 100,
            end_ms: (i as u64) * 100 + 80,
            confidence: 0.95,
        })
        .collect();
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
            thumbnail: format!("thumbnails/{source_id}_scene{i}.jpg").into(),
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

fn write_transcript(dir: &std::path::Path, transcript: &Transcript) {
    let path = dir
        .join("transcripts")
        .join(format!("{}.transcript.json", transcript.source_id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(transcript).unwrap()).unwrap();
}

fn write_index(dir: &std::path::Path, index: &SourceIndex) {
    let path = dir
        .join("index")
        .join(format!("{}.index.json", index.source_id));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(index).unwrap()).unwrap();
}

// -- Valid document -----------------------------------------------------------

#[test]
fn valid_edit_passes_all_checks() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, true, true);
    let manifest = make_manifest(vec![src]);
    write_transcript(tmp.path(), &make_transcript("src-001", 487));
    write_index(tmp.path(), &make_source_index("src-001", 4));

    let doc = make_edit_doc(vec![
        Shot { id: "shot-001".into(), source: "src-001".into(), range: ShotRange::Words { from: 0, to: 52 }, notes: vec![] },
        Shot { id: "shot-002".into(), source: "src-001".into(), range: ShotRange::Scenes { from: 0, to: 2 }, notes: vec![] },
        Shot { id: "shot-003".into(), source: "src-001".into(), range: ShotRange::Time { from_ms: 15000, to_ms: 22000 }, notes: vec![] },
    ]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(result.valid, "unexpected errors: {:?}", result.errors);
}

#[test]
fn empty_edit_is_valid() {
    let tmp = TempDir::new().unwrap();
    let result = validate(
        &make_edit_doc(vec![]),
        &make_manifest(vec![]),
        tmp.path(),
    );
    assert!(result.valid);
}

// -- Check 1: source not in manifest -----------------------------------------

#[test]
fn check1_source_not_found() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![]);
    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-999".into(),
        range: ShotRange::Words { from: 0, to: 10 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors[0].error.contains("not found in project"));
}

// -- Check 2: word range without transcript -----------------------------------

#[test]
fn check2_word_range_no_transcript() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, false);
    let manifest = make_manifest(vec![src]);
    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 52 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("no transcript")));
}

// -- Check 3: scene range without index ---------------------------------------

#[test]
fn check3_scene_range_no_index() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, false);
    let manifest = make_manifest(vec![src]);
    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Scenes { from: 0, to: 2 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("no scene index")));
}

// -- Check 4: word index out of bounds ----------------------------------------

#[test]
fn check4_word_index_exceeds_count() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, true, false);
    let manifest = make_manifest(vec![src]);
    write_transcript(tmp.path(), &make_transcript("src-001", 100));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 0, to: 500 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("word index 500")));
}

#[test]
fn check4_word_from_also_checked() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, true, false);
    let manifest = make_manifest(vec![src]);
    write_transcript(tmp.path(), &make_transcript("src-001", 100));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 200, to: 300 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    let word_errors: Vec<_> = result.errors.iter().filter(|e| e.error.contains("word index")).collect();
    assert_eq!(word_errors.len(), 2, "both from and to should be flagged");
}

// -- Check 5: scene index out of bounds ---------------------------------------

#[test]
fn check5_scene_index_exceeds_count() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, true);
    let manifest = make_manifest(vec![src]);
    write_index(tmp.path(), &make_source_index("src-001", 4));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Scenes { from: 0, to: 10 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("scene index 10")));
}

// -- Check 6: time exceeds duration -------------------------------------------

#[test]
fn check6_time_to_exceeds_duration() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, false);
    let manifest = make_manifest(vec![src]);

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Time { from_ms: 0, to_ms: 200000 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("exceeds duration")));
}

#[test]
fn check6_time_from_exceeds_duration() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 50000, false, false);
    let manifest = make_manifest(vec![src]);

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Time { from_ms: 60000, to_ms: 70000 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    let time_errors: Vec<_> = result.errors.iter().filter(|e| e.error.contains("exceeds duration")).collect();
    assert_eq!(time_errors.len(), 2, "both from and to exceed");
}

#[test]
fn check6_time_at_duration_boundary_is_valid() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, false);
    let manifest = make_manifest(vec![src]);

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Time { from_ms: 10000, to_ms: 124500 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(result.valid);
}

// -- Check 7: reversed range (from > to) -------------------------------------

#[test]
fn check7_words_reversed_range() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, true, false);
    let manifest = make_manifest(vec![src]);
    write_transcript(tmp.path(), &make_transcript("src-001", 487));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 52, to: 10 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("must be <=")));
}

#[test]
fn check7_scenes_reversed_range() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, true);
    let manifest = make_manifest(vec![src]);
    write_index(tmp.path(), &make_source_index("src-001", 4));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Scenes { from: 3, to: 1 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("must be <=")));
}

#[test]
fn check7_time_reversed_range() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, false);
    let manifest = make_manifest(vec![src]);

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Time { from_ms: 22000, to_ms: 15000 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("must be <=")));
}

// -- Check 8: zero duration (from == to) --------------------------------------

#[test]
fn check8_words_zero_duration() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, true, false);
    let manifest = make_manifest(vec![src]);
    write_transcript(tmp.path(), &make_transcript("src-001", 487));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Words { from: 5, to: 5 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("zero duration")));
}

#[test]
fn check8_scenes_zero_duration() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, true);
    let manifest = make_manifest(vec![src]);
    write_index(tmp.path(), &make_source_index("src-001", 4));

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Scenes { from: 2, to: 2 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("zero duration")));
}

#[test]
fn check8_time_zero_duration() {
    let tmp = TempDir::new().unwrap();
    let src = make_source("src-001", 124500, false, false);
    let manifest = make_manifest(vec![src]);

    let doc = make_edit_doc(vec![Shot {
        id: "shot-001".into(),
        source: "src-001".into(),
        range: ShotRange::Time { from_ms: 5000, to_ms: 5000 },
        notes: vec![],
    }]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.error.contains("zero duration")));
}

// -- Multiple errors collected ------------------------------------------------

#[test]
fn multiple_errors_from_different_shots() {
    let tmp = TempDir::new().unwrap();
    let src1 = make_source("src-001", 124500, true, false);
    let manifest = make_manifest(vec![src1]);
    write_transcript(tmp.path(), &make_transcript("src-001", 100));

    let doc = make_edit_doc(vec![
        Shot {
            id: "shot-001".into(),
            source: "src-999".into(),
            range: ShotRange::Words { from: 0, to: 10 },
            notes: vec![],
        },
        Shot {
            id: "shot-002".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 500 },
            notes: vec![],
        },
        Shot {
            id: "shot-003".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 5, to: 5 },
            notes: vec![],
        },
    ]);

    let result = validate(&doc, &manifest, tmp.path());
    assert!(!result.valid);
    assert!(result.errors.len() >= 3);

    let shot_ids: Vec<&str> = result.errors.iter().map(|e| e.shot_id.as_str()).collect();
    assert!(shot_ids.contains(&"shot-001"));
    assert!(shot_ids.contains(&"shot-002"));
    assert!(shot_ids.contains(&"shot-003"));
}

// -- Serialization ------------------------------------------------------------

#[test]
fn validation_result_serializes_correctly() {
    use ar_edit_core::validate::{ValidationError, ValidationResult};

    let result = ValidationResult {
        valid: false,
        errors: vec![ValidationError {
            shot_id: "shot-003".into(),
            error: "word index 500 exceeds word_count 100 for src-001".into(),
        }],
    };

    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["valid"], false);
    assert_eq!(json["errors"][0]["shot_id"], "shot-003");
    assert!(json["errors"][0]["error"]
        .as_str()
        .unwrap()
        .contains("word index 500"));
}
