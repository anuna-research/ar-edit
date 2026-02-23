//! TEST-006: Whisper model selection
//!
//! Verifies model validation, default model, and that model names flow
//! correctly through transcript parsing. Uses pre-generated fixtures
//! (no whisper.cpp dependency).

use std::fs;
use std::path::Path;

use ar_edit_core::models::Transcript;
use ar_edit_core::transcript::{
    parse_whisper_json, validate_model, DEFAULT_WHISPER_MODEL, WHISPER_MODELS,
};

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Model validation
// ---------------------------------------------------------------------------

#[test]
fn default_model_is_base() {
    assert_eq!(DEFAULT_WHISPER_MODEL, "base");
}

#[test]
fn all_valid_models_accepted() {
    for model in WHISPER_MODELS {
        assert!(
            validate_model(model).is_ok(),
            "validate_model should accept '{model}'"
        );
    }
}

#[test]
fn invalid_model_rejected_with_descriptive_error() {
    let err = validate_model("huge").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("huge"),
        "error should mention the invalid model"
    );
    assert!(
        msg.contains("tiny") && msg.contains("base") && msg.contains("small"),
        "error should list valid options"
    );
}

#[test]
fn model_validation_is_case_sensitive() {
    assert!(validate_model("Base").is_err());
    assert!(validate_model("BASE").is_err());
    assert!(validate_model("TINY").is_err());
}

// ---------------------------------------------------------------------------
// Model name flows through parsing
// ---------------------------------------------------------------------------

#[test]
fn parse_whisper_json_preserves_model_name() {
    let json = r#"{
        "result": { "language": "en" },
        "transcription": [
            {
                "offsets": { "from": 0, "to": 3000 },
                "text": " Hello",
                "tokens": [
                    { "text": " Hello", "offsets": { "from": 0, "to": 3000 }, "p": 0.9 }
                ]
            }
        ]
    }"#;

    for model in WHISPER_MODELS {
        let t = parse_whisper_json(json, "src-001", model).unwrap();
        assert_eq!(
            t.model, *model,
            "model field should be '{model}', got '{}'",
            t.model
        );
    }
}

#[test]
fn fixture_src_001_has_base_model() {
    let path = workspace_root().join("tests/fixtures/transcripts/src-001.transcript.json");
    let content = fs::read_to_string(&path).unwrap();
    let t: Transcript = serde_json::from_str(&content).unwrap();
    assert_eq!(t.model, "base");
}

#[test]
fn fixture_src_002_has_small_model() {
    let path = workspace_root().join("tests/fixtures/transcripts/src-002.transcript.json");
    let content = fs::read_to_string(&path).unwrap();
    let t: Transcript = serde_json::from_str(&content).unwrap();
    assert_eq!(t.model, "small");
}

#[test]
fn different_models_produce_distinct_transcripts() {
    let json = r#"{
        "result": { "language": "en" },
        "transcription": [
            {
                "offsets": { "from": 0, "to": 3000 },
                "text": " Hello",
                "tokens": [
                    { "text": " Hello", "offsets": { "from": 0, "to": 3000 }, "p": 0.9 }
                ]
            }
        ]
    }"#;

    let t_base = parse_whisper_json(json, "src-001", "base").unwrap();
    let t_small = parse_whisper_json(json, "src-001", "small").unwrap();

    assert_ne!(t_base.model, t_small.model);
    // But same content
    assert_eq!(t_base.word_count, t_small.word_count);
    assert_eq!(t_base.segments.len(), t_small.segments.len());
}

#[test]
fn find_model_rejects_invalid_names() {
    assert!(ar_edit_core::transcript::find_model("huge").is_err());
    assert!(ar_edit_core::transcript::find_model("").is_err());
    assert!(ar_edit_core::transcript::find_model("gpt-4").is_err());
}
