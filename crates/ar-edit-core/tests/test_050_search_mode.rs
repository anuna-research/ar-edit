//! TEST-050: Search mode
//!
//! Verifies the unified search pipeline used by the TUI's search mode:
//! transcript search, scene search, metadata (marker) search, type filtering,
//! source filtering, and result structure for display.

use std::fs;
use std::path::Path;

use ar_edit_core::models::{
    Defaults, Manifest, Marker, Scene, ShotRange, Source, SourceIndex, SourceMarkers,
    SourceMetadata, Transcript, TranscriptSegment, Word,
};
use ar_edit_core::search::{self, ResultType, TypeFilter};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_source(id: &str, transcribed: bool, indexed: bool) -> Source {
    Source {
        id: id.into(),
        path: format!("sources/{id}.mp4").into(),
        original_filename: format!("{id}.mp4"),
        duration_ms: 120000,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (1920, 1080),
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed,
        indexed,
    }
}

fn make_manifest(sources: Vec<Source>) -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        name: "test".into(),
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

fn make_transcript(source_id: &str, words: &[(&str, u64, u64)]) -> Transcript {
    let mut idx: u32 = 0;
    let w: Vec<Word> = words
        .iter()
        .map(|(text, start, end)| {
            let w = Word {
                index: idx,
                text: text.to_string(),
                start_ms: *start,
                end_ms: *end,
                confidence: 0.95,
            };
            idx += 1;
            w
        })
        .collect();

    let text = w
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let start_ms = words.first().map_or(0, |w| w.1);
    let end_ms = words.last().map_or(0, |w| w.2);

    Transcript {
        source_id: source_id.into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: end_ms,
        segments: vec![TranscriptSegment {
            index: 0,
            start_ms,
            end_ms,
            text,
            words: w,
        }],
        word_count: idx,
    }
}

fn make_index(source_id: &str, scenes: Vec<Scene>) -> SourceIndex {
    let count = scenes.len() as u32;
    SourceIndex {
        source_id: source_id.into(),
        indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
        metadata: SourceMetadata {
            duration_ms: 120000,
            resolution: (1920, 1080),
            codec: "h264".into(),
            file_size_bytes: 52428800,
        },
        thumbnails: vec![],
        scene_count: count,
        scenes,
    }
}

fn make_scene(index: u32, start_ms: u64, end_ms: u64, desc: Option<&str>) -> Scene {
    Scene {
        index,
        start_ms,
        end_ms,
        thumbnail: format!("t{index}.jpg").into(),
        description: desc.map(|s| s.to_string()),
    }
}

fn setup_project(
    dir: &Path,
    manifest: &Manifest,
    transcripts: &[Transcript],
    indexes: &[SourceIndex],
    markers: &[SourceMarkers],
) {
    fs::create_dir_all(dir.join("transcripts")).unwrap();
    fs::create_dir_all(dir.join("index")).unwrap();
    fs::create_dir_all(dir.join("annotations")).unwrap();

    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .unwrap();

    for t in transcripts {
        fs::write(
            dir.join(format!("transcripts/{}.transcript.json", t.source_id)),
            serde_json::to_string_pretty(t).unwrap(),
        )
        .unwrap();
    }

    for idx in indexes {
        fs::write(
            dir.join(format!("index/{}.index.json", idx.source_id)),
            serde_json::to_string_pretty(idx).unwrap(),
        )
        .unwrap();
    }

    for m in markers {
        fs::write(
            dir.join(format!("annotations/{}.markers.json", m.source_id)),
            serde_json::to_string_pretty(m).unwrap(),
        )
        .unwrap();
    }
}

// ---------------------------------------------------------------------------
// Tests: Basic search
// ---------------------------------------------------------------------------

/// Search finds a word in a transcript.
#[test]
fn search_finds_transcript_word() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, false)]);
    let transcript = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("climate", 200, 600),
            ("debate", 600, 1000),
        ],
    );
    setup_project(tmp.path(), &manifest, &[transcript], &[], &[]);

    let results = search::search(tmp.path(), "climate", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Transcript);
    assert_eq!(results[0].source_id, "src-001");
    assert_eq!(results[0].matched_text, "climate");
}

/// Search finds a scene description.
#[test]
fn search_finds_scene_description() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let index = make_index(
        "src-001",
        vec![
            make_scene(0, 0, 30000, Some("Wide establishing shot")),
            make_scene(1, 30000, 60000, Some("Close-up interview")),
        ],
    );
    setup_project(tmp.path(), &manifest, &[], &[index], &[]);

    let results = search::search(tmp.path(), "interview", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Scene);
    assert_eq!(results[0].start_ms, 30000);
    assert_eq!(results[0].end_ms, 60000);
}

/// Search finds a marker label.
#[test]
fn search_finds_marker_label() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, false)]);
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
            label: "hero-shot".into(),
            note: Some("Best take".into()),
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }],
    };
    setup_project(tmp.path(), &manifest, &[], &[], &[markers]);

    let results = search::search(tmp.path(), "hero", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Metadata);
    assert_eq!(results[0].matched_text, "hero-shot");
}

/// Search finds a marker note.
#[test]
fn search_finds_marker_note() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, false)]);
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
            label: "select".into(),
            note: Some("Great performance here".into()),
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }],
    };
    setup_project(tmp.path(), &manifest, &[], &[], &[markers]);

    let results = search::search(tmp.path(), "performance", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Metadata);
    assert!(results[0].matched_text.contains("performance"));
}

// ---------------------------------------------------------------------------
// Tests: Unified search across all types
// ---------------------------------------------------------------------------

/// Search returns results from all three domains when query matches.
#[test]
fn search_unified_across_all_types() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript(
        "src-001",
        &[("The", 0, 200), ("office", 200, 600), ("scene", 600, 1000)],
    );
    let index = make_index(
        "src-001",
        vec![make_scene(0, 0, 30000, Some("Interior office wide"))],
    );
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
            label: "office-best".into(),
            note: None,
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }],
    };
    setup_project(tmp.path(), &manifest, &[transcript], &[index], &[markers]);

    let results = search::search(tmp.path(), "office", None, None).unwrap();
    assert_eq!(results.len(), 3);

    let types: Vec<&ResultType> = results.iter().map(|r| &r.result_type).collect();
    assert!(types.contains(&&ResultType::Transcript));
    assert!(types.contains(&&ResultType::Scene));
    assert!(types.contains(&&ResultType::Metadata));
}

// ---------------------------------------------------------------------------
// Tests: Type filtering (Tab key cycles filter)
// ---------------------------------------------------------------------------

/// Filter by transcript type only.
#[test]
fn filter_transcript_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript("src-001", &[("office", 0, 400)]);
    let index = make_index(
        "src-001",
        vec![make_scene(0, 0, 30000, Some("office shot"))],
    );
    setup_project(tmp.path(), &manifest, &[transcript], &[index], &[]);

    let results =
        search::search(tmp.path(), "office", None, Some(&TypeFilter::Transcript)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Transcript);
}

/// Filter by scene type only.
#[test]
fn filter_scene_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript("src-001", &[("office", 0, 400)]);
    let index = make_index(
        "src-001",
        vec![make_scene(0, 0, 30000, Some("office shot"))],
    );
    setup_project(tmp.path(), &manifest, &[transcript], &[index], &[]);

    let results = search::search(tmp.path(), "office", None, Some(&TypeFilter::Scene)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Scene);
}

/// Filter by metadata type only.
#[test]
fn filter_metadata_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript("src-001", &[("office", 0, 400)]);
    let index = make_index(
        "src-001",
        vec![make_scene(0, 0, 30000, Some("office shot"))],
    );
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
            label: "office-select".into(),
            note: None,
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }],
    };
    setup_project(tmp.path(), &manifest, &[transcript], &[index], &[markers]);

    let results = search::search(tmp.path(), "office", None, Some(&TypeFilter::Metadata)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Metadata);
}

// ---------------------------------------------------------------------------
// Tests: Search across multiple sources
// ---------------------------------------------------------------------------

/// Search returns results from multiple sources.
#[test]
fn search_across_multiple_sources() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", true, false),
        make_source("src-002", true, false),
    ]);
    let t1 = make_transcript("src-001", &[("interview", 0, 400)]);
    let t2 = make_transcript("src-002", &[("interview", 0, 400)]);
    setup_project(tmp.path(), &manifest, &[t1, t2], &[], &[]);

    let results = search::search(tmp.path(), "interview", None, None).unwrap();
    assert_eq!(results.len(), 2);

    let source_ids: Vec<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    assert!(source_ids.contains(&"src-001"));
    assert!(source_ids.contains(&"src-002"));
}

/// Source filter restricts to a single source.
#[test]
fn search_with_source_filter() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", true, false),
        make_source("src-002", true, false),
    ]);
    let t1 = make_transcript("src-001", &[("interview", 0, 400)]);
    let t2 = make_transcript("src-002", &[("interview", 0, 400)]);
    setup_project(tmp.path(), &manifest, &[t1, t2], &[], &[]);

    let results = search::search(tmp.path(), "interview", Some("src-002"), None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_id, "src-002");
}

// ---------------------------------------------------------------------------
// Tests: Edge cases
// ---------------------------------------------------------------------------

/// Empty query is handled (search module uses regex, empty = match all).
#[test]
fn search_empty_project() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![]);
    setup_project(tmp.path(), &manifest, &[], &[], &[]);

    let results = search::search(tmp.path(), "anything", None, None).unwrap();
    assert!(results.is_empty());
}

/// No matches returns empty results.
#[test]
fn search_no_matches() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, false)]);
    let transcript = make_transcript("src-001", &[("hello", 0, 400), ("world", 400, 800)]);
    setup_project(tmp.path(), &manifest, &[transcript], &[], &[]);

    let results = search::search(tmp.path(), "nonexistent", None, None).unwrap();
    assert!(results.is_empty());
}

/// Search is case-insensitive.
#[test]
fn search_case_insensitive() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let index = make_index(
        "src-001",
        vec![make_scene(0, 0, 30000, Some("Interior OFFICE Scene"))],
    );
    setup_project(tmp.path(), &manifest, &[], &[index], &[]);

    let results = search::search(tmp.path(), "office", None, None).unwrap();
    assert_eq!(results.len(), 1);
}

/// Invalid regex returns an error.
#[test]
fn search_invalid_regex_error() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![]);
    setup_project(tmp.path(), &manifest, &[], &[], &[]);

    let err = search::search(tmp.path(), "[invalid", None, None).unwrap_err();
    assert!(format!("{err}").contains("regex"));
}

// ---------------------------------------------------------------------------
// Tests: Result structure for display
// ---------------------------------------------------------------------------

/// Each result has a source_id, timestamps, matched_text, and context.
#[test]
fn result_has_all_display_fields() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, false)]);
    let transcript = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("big", 200, 400),
            ("climate", 400, 800),
            ("debate", 800, 1200),
            ("begins", 1200, 1600),
        ],
    );
    setup_project(tmp.path(), &manifest, &[transcript], &[], &[]);

    let results = search::search(tmp.path(), "climate", None, None).unwrap();
    let r = &results[0];

    assert_eq!(r.source_id, "src-001");
    assert_eq!(r.start_ms, 400);
    assert_eq!(r.end_ms, 800);
    assert!(!r.matched_text.is_empty());
    assert!(!r.context.is_empty());
}

/// Transcript search context includes surrounding words.
#[test]
fn transcript_result_context_has_surrounding_words() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, false)]);
    let transcript = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("big", 200, 400),
            ("climate", 400, 800),
            ("policy", 800, 1200),
            ("debate", 1200, 1600),
        ],
    );
    setup_project(tmp.path(), &manifest, &[transcript], &[], &[]);

    let results = search::search(tmp.path(), "climate", None, None).unwrap();
    let context = &results[0].context;
    // Context should include words before and after
    assert!(context.contains("The big"));
    assert!(context.contains("policy debate"));
}

/// Scene result context includes scene index.
#[test]
fn scene_result_context_has_scene_index() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let index = make_index(
        "src-001",
        vec![
            make_scene(0, 0, 30000, Some("Wide shot")),
            make_scene(1, 30000, 60000, Some("Interview close-up")),
        ],
    );
    setup_project(tmp.path(), &manifest, &[], &[index], &[]);

    let results = search::search(tmp.path(), "interview", None, None).unwrap();
    assert!(results[0].context.contains("scene 1:"));
}

/// Marker result context includes marker ID and label.
#[test]
fn marker_result_context_has_id_and_label() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, false)]);
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
            label: "hero".into(),
            note: Some("Best take".into()),
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }],
    };
    setup_project(tmp.path(), &manifest, &[], &[], &[markers]);

    let results = search::search(tmp.path(), "hero", None, None).unwrap();
    let context = &results[0].context;
    assert!(context.contains("mark-001"));
    assert!(context.contains("hero"));
}
