//! TEST-009: Transcript reading
//! TEST-010: Transcript search
//!
//! Verifies that `transcript_ops::read()` returns the full transcript and
//! that `transcript_ops::search()` finds matches across multiple sources
//! with correct word indices and context. Uses in-memory project setup
//! (no whisper.cpp or ffmpeg dependency).

use std::path::Path;

use ar_edit_core::models::*;
use ar_edit_core::transcript_ops;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_manifest(sources: Vec<Source>) -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        name: "test-project".into(),
        created: "2026-02-19T12:00:00Z".parse().unwrap(),
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

fn make_source(id: &str, transcribed: bool) -> Source {
    Source {
        id: id.into(),
        path: format!("sources/{id}.mp4").into(),
        original_filename: format!("{id}.mp4"),
        duration_ms: 124500,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (1920, 1080),
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed,
        indexed: false,
    }
}

fn make_transcript(source_id: &str, words: &[(&str, u64, u64)]) -> Transcript {
    let mut global_idx: u32 = 0;
    let mut duration_ms: u64 = 0;

    let word_list: Vec<Word> = words
        .iter()
        .map(|(text, start, end)| {
            let w = Word {
                index: global_idx,
                text: text.to_string(),
                start_ms: *start,
                end_ms: *end,
                confidence: 0.95,
            };
            global_idx += 1;
            if *end > duration_ms {
                duration_ms = *end;
            }
            w
        })
        .collect();

    let text = word_list
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    Transcript {
        source_id: source_id.into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms,
        word_count: global_idx,
        segments: vec![TranscriptSegment {
            index: 0,
            start_ms: words.first().map_or(0, |w| w.1),
            end_ms: words.last().map_or(0, |w| w.2),
            text,
            words: word_list,
        }],
    }
}

fn setup_project(dir: &Path, manifest: &Manifest, transcripts: &[Transcript]) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();

    for t in transcripts {
        std::fs::write(
            dir.join(format!("transcripts/{}.transcript.json", t.source_id)),
            serde_json::to_string_pretty(t).unwrap(),
        )
        .unwrap();
    }
}

// ---------------------------------------------------------------------------
// TEST-009: Transcript reading
// ---------------------------------------------------------------------------

#[test]
fn read_returns_full_transcript() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript("src-001", &[("Hello", 0, 500), ("world", 500, 1000)]);
    setup_project(tmp.path(), &manifest, &[t.clone()]);

    let result = transcript_ops::read(tmp.path(), "src-001").unwrap();
    assert_eq!(result, t);
}

#[test]
fn read_returns_correct_source_id() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript("src-001", &[("Hello", 0, 500)]);
    setup_project(tmp.path(), &manifest, &[t]);

    let result = transcript_ops::read(tmp.path(), "src-001").unwrap();
    assert_eq!(result.source_id, "src-001");
}

#[test]
fn read_matches_stored_json() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[("Hello", 0, 500), ("world", 500, 1000), ("test", 1000, 1500)],
    );
    setup_project(tmp.path(), &manifest, &[t.clone()]);

    let result = transcript_ops::read(tmp.path(), "src-001").unwrap();

    // Verify all fields match
    assert_eq!(result.source_id, t.source_id);
    assert_eq!(result.model, t.model);
    assert_eq!(result.language, t.language);
    assert_eq!(result.duration_ms, t.duration_ms);
    assert_eq!(result.word_count, t.word_count);
    assert_eq!(result.segments.len(), t.segments.len());
    assert_eq!(result.segments[0].words.len(), t.segments[0].words.len());
}

#[test]
fn read_rejects_unknown_source() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    setup_project(tmp.path(), &manifest, &[]);

    let err = transcript_ops::read(tmp.path(), "src-999").unwrap_err();
    assert!(format!("{err}").contains("not found"));
}

#[test]
fn read_rejects_untranscribed_source() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false)]);
    setup_project(tmp.path(), &manifest, &[]);

    let err = transcript_ops::read(tmp.path(), "src-001").unwrap_err();
    assert!(format!("{err}").contains("no transcript"));
}

#[test]
fn read_preserves_word_details() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript("src-001", &[("Hello", 100, 500), ("world", 500, 1000)]);
    setup_project(tmp.path(), &manifest, &[t]);

    let result = transcript_ops::read(tmp.path(), "src-001").unwrap();
    let word = &result.segments[0].words[0];
    assert_eq!(word.text, "Hello");
    assert_eq!(word.start_ms, 100);
    assert_eq!(word.end_ms, 500);
    assert_eq!(word.index, 0);
}

// ---------------------------------------------------------------------------
// TEST-010: Transcript search
// ---------------------------------------------------------------------------

#[test]
fn search_finds_climate_in_two_sources() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
        make_source("src-003", true),
    ]);

    let t1 = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("climate", 200, 600),
            ("is", 600, 800),
            ("changing", 800, 1200),
        ],
    );
    let t2 = make_transcript(
        "src-002",
        &[
            ("No", 0, 200),
            ("match", 200, 500),
            ("here", 500, 800),
        ],
    );
    let t3 = make_transcript(
        "src-003",
        &[
            ("Our", 0, 200),
            ("climate", 200, 600),
            ("future", 600, 1000),
        ],
    );

    setup_project(tmp.path(), &manifest, &[t1, t2, t3]);

    let results = transcript_ops::search(tmp.path(), "climate", None).unwrap();
    assert_eq!(results.len(), 2);

    let source_ids: Vec<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    assert!(source_ids.contains(&"src-001"));
    assert!(source_ids.contains(&"src-003"));
    assert!(!source_ids.contains(&"src-002"));
}

#[test]
fn search_returns_correct_word_indices() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("climate", 200, 600),
            ("policy", 600, 1000),
        ],
    );
    setup_project(tmp.path(), &manifest, &[t]);

    let results = transcript_ops::search(tmp.path(), "climate", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].from_word, 1);
    assert_eq!(results[0].to_word, 1);
}

#[test]
fn search_returns_correct_timestamps() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("climate", 200, 600),
            ("policy", 600, 1000),
        ],
    );
    setup_project(tmp.path(), &manifest, &[t]);

    let results = transcript_ops::search(tmp.path(), "climate", None).unwrap();
    assert_eq!(results[0].start_ms, 200);
    assert_eq!(results[0].end_ms, 600);
}

#[test]
fn search_returns_context() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("big", 200, 400),
            ("climate", 400, 800),
            ("policy", 800, 1200),
            ("debate", 1200, 1600),
        ],
    );
    setup_project(tmp.path(), &manifest, &[t]);

    let results = transcript_ops::search(tmp.path(), "climate", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].context_before, "The big");
    assert_eq!(results[0].context_after, "policy debate");
}

#[test]
fn search_is_case_insensitive() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[("Climate", 0, 400), ("change", 400, 800)],
    );
    setup_project(tmp.path(), &manifest, &[t]);

    let results = transcript_ops::search(tmp.path(), "climate", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].text, "Climate");
}

#[test]
fn search_no_matches_returns_empty() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[("Hello", 0, 400), ("world", 400, 800)],
    );
    setup_project(tmp.path(), &manifest, &[t]);

    let results = transcript_ops::search(tmp.path(), "nonexistent", None).unwrap();
    assert!(results.is_empty());
}

#[test]
fn search_multi_word_match() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript(
        "src-001",
        &[
            ("climate", 0, 400),
            ("policy", 400, 800),
            ("debate", 800, 1200),
        ],
    );
    setup_project(tmp.path(), &manifest, &[t]);

    let results = transcript_ops::search(tmp.path(), "climate policy", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].from_word, 0);
    assert_eq!(results[0].to_word, 1);
    assert_eq!(results[0].text, "climate policy");
}

#[test]
fn search_with_source_filter() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-003", true),
    ]);
    let t1 = make_transcript("src-001", &[("climate", 0, 400)]);
    let t3 = make_transcript("src-003", &[("climate", 0, 400)]);
    setup_project(tmp.path(), &manifest, &[t1, t3]);

    // Filter to only src-003
    let results = transcript_ops::search(tmp.path(), "climate", Some("src-003")).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_id, "src-003");
}

#[test]
fn search_invalid_regex_returns_error() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    setup_project(tmp.path(), &manifest, &[]);

    let err = transcript_ops::search(tmp.path(), "[invalid", None).unwrap_err();
    assert!(format!("{err}").contains("regex"));
}

// ---------------------------------------------------------------------------
// Fixture-based integration tests
// ---------------------------------------------------------------------------

#[test]
fn fixture_transcripts_are_searchable() {
    // Load fixtures into a temp project and verify search works
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
        make_source("src-003", true),
    ]);
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::write(
        tmp.path().join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Copy fixture files to temp project
    for id in &["src-001", "src-002", "src-003"] {
        let fixture = workspace_root.join(format!("tests/fixtures/transcripts/{id}.transcript.json"));
        let dest = tmp.path().join(format!("transcripts/{id}.transcript.json"));
        std::fs::copy(&fixture, &dest).unwrap();
    }

    // Search for "Climate" — should be in src-003 only (fixture content)
    let results = transcript_ops::search(tmp.path(), "Climate", None).unwrap();
    assert!(
        !results.is_empty(),
        "search for 'Climate' should find matches in fixture transcripts"
    );
    assert!(
        results.iter().any(|r| r.source_id == "src-003"),
        "src-003 fixture contains 'Climate'"
    );
}

#[test]
fn fixture_transcripts_are_listable() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
        make_source("src-003", true),
    ]);
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::write(
        tmp.path().join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    for id in &["src-001", "src-002", "src-003"] {
        let fixture = workspace_root.join(format!("tests/fixtures/transcripts/{id}.transcript.json"));
        let dest = tmp.path().join(format!("transcripts/{id}.transcript.json"));
        std::fs::copy(&fixture, &dest).unwrap();
    }

    let result = transcript_ops::list(tmp.path()).unwrap();
    assert_eq!(result.len(), 3);

    // Verify word counts match fixture data
    let r1 = result.iter().find(|r| r.source_id == "src-001").unwrap();
    let r2 = result.iter().find(|r| r.source_id == "src-002").unwrap();
    let r3 = result.iter().find(|r| r.source_id == "src-003").unwrap();
    assert_eq!(r1.word_count, 10);
    assert_eq!(r2.word_count, 13);
    assert_eq!(r3.word_count, 13);
}

#[test]
fn fixture_transcripts_are_readable() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::write(
        tmp.path().join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let fixture = workspace_root.join("tests/fixtures/transcripts/src-001.transcript.json");
    let dest = tmp.path().join("transcripts/src-001.transcript.json");
    std::fs::copy(&fixture, &dest).unwrap();

    let result = transcript_ops::read(tmp.path(), "src-001").unwrap();
    assert_eq!(result.source_id, "src-001");
    assert_eq!(result.word_count, 10);
    assert_eq!(result.segments.len(), 2);
}
