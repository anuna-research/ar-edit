//! TEST-040: Cross-index search
//!
//! Verifies that `search::search()` finds results across transcripts, scene
//! descriptions, and markers simultaneously. Tests source filtering, type
//! filtering, case-insensitive matching, and multi-source scenarios using
//! pre-generated index fixtures.

use ar_edit_core::models::*;
use ar_edit_core::search::{search, ResultType, TypeFilter};
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_index(name: &str) -> SourceIndex {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/indexes")
        .join(name);
    let data = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&data).unwrap()
}

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

fn make_source(id: &str, transcribed: bool, indexed: bool) -> Source {
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
        indexed,
    }
}

fn make_transcript(source_id: &str, words_data: &[(&str, u64, u64)]) -> Transcript {
    let mut global_idx: u32 = 0;
    let mut duration_ms: u64 = 0;

    let words: Vec<Word> = words_data
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

    let text = words
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
            start_ms: words_data.first().map_or(0, |w| w.1),
            end_ms: words_data.last().map_or(0, |w| w.2),
            text,
            words,
        }],
    }
}

fn setup_project(
    dir: &Path,
    manifest: &Manifest,
    transcripts: &[Transcript],
    indexes: &[SourceIndex],
    markers: &[SourceMarkers],
) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("annotations")).unwrap();

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

    for idx in indexes {
        std::fs::write(
            dir.join(format!("index/{}.index.json", idx.source_id)),
            serde_json::to_string_pretty(idx).unwrap(),
        )
        .unwrap();
    }

    for m in markers {
        std::fs::write(
            dir.join(format!("annotations/{}.markers.json", m.source_id)),
            serde_json::to_string_pretty(m).unwrap(),
        )
        .unwrap();
    }
}

// -- Cross-type search using fixtures -----------------------------------------

#[test]
fn search_finds_scene_from_fixture_index() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    let results = search(tmp.path(), "office", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, ResultType::Scene);
    assert_eq!(results[0].source_id, "src-001");
    assert!(results[0].matched_text.contains("office"));
}

#[test]
fn search_finds_transcript_and_scene_for_same_query() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("interview", 200, 600),
            ("begins", 600, 1000),
        ],
    );
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[transcript], &[idx], &[]);

    let results = search(tmp.path(), "interview", None, None).unwrap();
    assert!(results.len() >= 2);

    let types: Vec<&ResultType> = results.iter().map(|r| &r.result_type).collect();
    assert!(types.contains(&&ResultType::Transcript));
    assert!(types.contains(&&ResultType::Scene));
}

#[test]
fn search_finds_all_three_types() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript(
        "src-001",
        &[
            ("The", 0, 200),
            ("interview", 200, 600),
            ("scene", 600, 1000),
        ],
    );
    let idx = fixture_index("src-001.index.json");
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
            label: "select".into(),
            note: Some("Best interview take".into()),
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }],
    };
    setup_project(tmp.path(), &manifest, &[transcript], &[idx], &[markers]);

    let results = search(tmp.path(), "interview", None, None).unwrap();
    let types: Vec<&ResultType> = results.iter().map(|r| &r.result_type).collect();
    assert!(types.contains(&&ResultType::Transcript));
    assert!(types.contains(&&ResultType::Scene));
    assert!(types.contains(&&ResultType::Metadata));
}

// -- Multi-source search with fixtures ----------------------------------------

#[test]
fn search_across_multiple_indexed_sources() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", false, true),
        make_source("src-002", false, true),
    ]);
    let idx1 = fixture_index("src-001.index.json");
    let idx2 = fixture_index("src-002.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx1, idx2], &[]);

    // "interview" appears in src-001 scene descriptions
    let results = search(tmp.path(), "interview", None, None).unwrap();
    let sources: Vec<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    assert!(sources.contains(&"src-001"));

    // "desk" appears in src-002 scene descriptions
    let results = search(tmp.path(), "desk", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_id, "src-002");
}

// -- Source filter -------------------------------------------------------------

#[test]
fn source_filter_limits_to_one_source() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", false, true),
        make_source("src-002", false, true),
    ]);
    let idx1 = fixture_index("src-001.index.json");
    let idx2 = fixture_index("src-002.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx1, idx2], &[]);

    // Both sources have scene descriptions, but filter to src-002 only
    let results = search(tmp.path(), ".*", Some("src-002"), None).unwrap();
    for r in &results {
        assert_eq!(r.source_id, "src-002");
    }
}

// -- Type filter --------------------------------------------------------------

#[test]
fn type_filter_scene_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript("src-001", &[("office", 0, 400)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[transcript], &[idx], &[]);

    let results = search(tmp.path(), "office", None, Some(&TypeFilter::Scene)).unwrap();
    for r in &results {
        assert_eq!(r.result_type, ResultType::Scene);
    }
}

#[test]
fn type_filter_transcript_only() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", true, true)]);
    let transcript = make_transcript("src-001", &[("office", 0, 400)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[transcript], &[idx], &[]);

    let results = search(tmp.path(), "office", None, Some(&TypeFilter::Transcript)).unwrap();
    for r in &results {
        assert_eq!(r.result_type, ResultType::Transcript);
    }
}

// -- Case-insensitive matching on scene descriptions --------------------------

#[test]
fn case_insensitive_scene_search() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    // "interior" in lowercase should match "Interior office, wide shot"
    let results = search(tmp.path(), "interior", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].matched_text.contains("Interior"));
}

// -- Scenes without descriptions are skipped ----------------------------------

#[test]
fn scenes_without_descriptions_not_searched() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-003", false, true)]);
    let idx = fixture_index("src-003-no-descriptions.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    let results = search(tmp.path(), "anything", None, None).unwrap();
    assert!(results.is_empty());
}

// -- Regex pattern matching ---------------------------------------------------

#[test]
fn regex_pattern_matches_scene_descriptions() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    // Match scenes containing "shot" (appears in "wide shot", "establishing shot")
    let results = search(tmp.path(), "shot", None, None).unwrap();
    assert!(results.len() >= 2);
}

// -- Scene result context format ----------------------------------------------

#[test]
fn scene_result_context_includes_scene_index() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    let results = search(tmp.path(), "office", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert!(
        results[0].context.contains("scene 0:"),
        "context should contain scene index: {}",
        results[0].context
    );
}

// -- Scene result timestamps match scene boundaries ---------------------------

#[test]
fn scene_result_timestamps_match_scene_boundaries() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", false, true)]);
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    let results = search(tmp.path(), "Close-up", None, None).unwrap();
    assert_eq!(results.len(), 1);
    // "Close-up interview" is scene 1: 18000..45000
    assert_eq!(results[0].start_ms, 18000);
    assert_eq!(results[0].end_ms, 45000);
}

// -- No results from unindexed source -----------------------------------------

#[test]
fn unindexed_source_returns_no_scene_results() {
    let tmp = TempDir::new().unwrap();
    // Source is NOT indexed
    let manifest = make_manifest(vec![make_source("src-001", false, false)]);
    // Write the index file anyway — it should be ignored because source.indexed is false
    let idx = fixture_index("src-001.index.json");
    setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

    let results = search(tmp.path(), "office", None, Some(&TypeFilter::Scene)).unwrap();
    assert!(results.is_empty(), "should not search unindexed source");
}
