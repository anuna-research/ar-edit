//! TEST-004: Transcription produces valid JSON
//! TEST-005: Transcript word indices are globally sequential
//!
//! Uses pre-generated transcript fixtures (no whisper.cpp dependency).
//! Verifies JSON structure, word timing invariants, and sequential indexing.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ar_edit_core::models::Transcript;

/// Workspace root for locating fixture files.
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

fn load_fixture(name: &str) -> Transcript {
    let path = workspace_root().join(format!("tests/fixtures/transcripts/{name}"));
    let content = fs::read_to_string(&path).unwrap();
    serde_json::from_str(&content).unwrap()
}

// ---------------------------------------------------------------------------
// TEST-004: Transcription produces valid JSON
// ---------------------------------------------------------------------------

#[test]
fn fixture_src_001_is_valid_json() {
    let t = load_fixture("src-001.transcript.json");
    assert_eq!(t.source_id, "src-001");
    assert!(t.word_count > 0, "word_count must be > 0");
}

#[test]
fn fixture_src_002_is_valid_json() {
    let t = load_fixture("src-002.transcript.json");
    assert_eq!(t.source_id, "src-002");
    assert!(t.word_count > 0, "word_count must be > 0");
}

#[test]
fn fixture_src_003_is_valid_json() {
    let t = load_fixture("src-003.transcript.json");
    assert_eq!(t.source_id, "src-003");
    assert!(t.word_count > 0, "word_count must be > 0");
}

#[test]
fn all_words_have_valid_timing() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        for seg in &t.segments {
            for word in &seg.words {
                assert!(
                    word.start_ms < word.end_ms,
                    "{name}: word {} '{}' has start_ms ({}) >= end_ms ({})",
                    word.index,
                    word.text,
                    word.start_ms,
                    word.end_ms
                );
            }
        }
    }
}

#[test]
fn word_count_matches_actual_words() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let actual: u32 = t.segments.iter().map(|s| s.words.len() as u32).sum();
        assert_eq!(
            t.word_count, actual,
            "{name}: word_count ({}) != actual words ({})",
            t.word_count, actual
        );
    }
}

#[test]
fn all_segments_have_valid_timing() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        for seg in &t.segments {
            assert!(
                seg.start_ms < seg.end_ms,
                "{name}: segment {} has start_ms ({}) >= end_ms ({})",
                seg.index,
                seg.start_ms,
                seg.end_ms
            );
        }
    }
}

#[test]
fn duration_ms_covers_all_segments() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let max_end = t.segments.iter().map(|s| s.end_ms).max().unwrap_or(0);
        assert!(
            t.duration_ms >= max_end,
            "{name}: duration_ms ({}) < max segment end ({})",
            t.duration_ms,
            max_end
        );
    }
}

#[test]
fn transcript_roundtrips_through_serde() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let json = serde_json::to_string(&t).unwrap();
        let back: Transcript = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back, "{name}: serde round-trip failed");
    }
}

#[test]
fn confidence_in_valid_range() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        for seg in &t.segments {
            for word in &seg.words {
                assert!(
                    (0.0..=1.0).contains(&word.confidence),
                    "{name}: word {} '{}' confidence {} out of range [0, 1]",
                    word.index,
                    word.text,
                    word.confidence
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TEST-005: Transcript word indices are globally sequential
// ---------------------------------------------------------------------------

#[test]
fn word_indices_start_at_zero() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let first_word = t.segments.iter().flat_map(|s| &s.words).next();
        if let Some(w) = first_word {
            assert_eq!(
                w.index, 0,
                "{name}: first word index should be 0, got {}",
                w.index
            );
        }
    }
}

#[test]
fn word_indices_end_at_word_count_minus_one() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let last_word = t.segments.iter().flat_map(|s| &s.words).last();
        if let Some(w) = last_word {
            assert_eq!(
                w.index,
                t.word_count - 1,
                "{name}: last word index ({}) != word_count - 1 ({})",
                w.index,
                t.word_count - 1
            );
        }
    }
}

#[test]
fn word_indices_are_sequential_no_gaps() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let indices: Vec<u32> = t
            .segments
            .iter()
            .flat_map(|s| &s.words)
            .map(|w| w.index)
            .collect();

        for (i, &idx) in indices.iter().enumerate() {
            assert_eq!(
                idx, i as u32,
                "{name}: expected word index {i} but got {idx} (gap detected)"
            );
        }
    }
}

#[test]
fn word_indices_have_no_duplicates() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        let indices: Vec<u32> = t
            .segments
            .iter()
            .flat_map(|s| &s.words)
            .map(|w| w.index)
            .collect();

        let unique: HashSet<u32> = indices.iter().copied().collect();
        assert_eq!(
            indices.len(),
            unique.len(),
            "{name}: duplicate word indices detected"
        );
    }
}

#[test]
fn word_indices_sequential_across_segments() {
    // Verify that indices continue sequentially from one segment to the next
    let t = load_fixture("src-001.transcript.json");
    assert!(
        t.segments.len() >= 2,
        "fixture needs at least 2 segments for cross-segment test"
    );

    let last_of_seg0 = t.segments[0].words.last().unwrap().index;
    let first_of_seg1 = t.segments[1].words.first().unwrap().index;
    assert_eq!(
        first_of_seg1,
        last_of_seg0 + 1,
        "word indices not sequential across segment boundary: seg0 ends at {}, seg1 starts at {}",
        last_of_seg0,
        first_of_seg1
    );
}

#[test]
fn segment_indices_are_sequential() {
    for name in &[
        "src-001.transcript.json",
        "src-002.transcript.json",
        "src-003.transcript.json",
    ] {
        let t = load_fixture(name);
        for (i, seg) in t.segments.iter().enumerate() {
            assert_eq!(
                seg.index, i as u32,
                "{name}: segment index mismatch at position {i}: got {}",
                seg.index
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Whisper JSON parsing produces valid output (TEST-004 from raw JSON)
// ---------------------------------------------------------------------------

#[test]
fn parse_whisper_json_produces_valid_transcript() {
    let json = r#"{
        "result": { "language": "en" },
        "transcription": [
            {
                "offsets": { "from": 0, "to": 5000 },
                "text": " Hello world test",
                "tokens": [
                    { "text": " Hello", "offsets": { "from": 0, "to": 1500 }, "p": 0.95 },
                    { "text": " world", "offsets": { "from": 1500, "to": 3000 }, "p": 0.90 },
                    { "text": " test", "offsets": { "from": 3000, "to": 5000 }, "p": 0.88 }
                ]
            },
            {
                "offsets": { "from": 5000, "to": 10000 },
                "text": " Second segment here",
                "tokens": [
                    { "text": " Second", "offsets": { "from": 5000, "to": 6500 }, "p": 0.93 },
                    { "text": " segment", "offsets": { "from": 6500, "to": 8000 }, "p": 0.91 },
                    { "text": " here", "offsets": { "from": 8000, "to": 10000 }, "p": 0.89 }
                ]
            }
        ]
    }"#;

    let t = ar_edit_core::transcript::parse_whisper_json(json, "src-test", "base").unwrap();

    // Valid JSON output checks (TEST-004)
    assert_eq!(t.source_id, "src-test");
    assert!(t.word_count > 0);
    for seg in &t.segments {
        for word in &seg.words {
            assert!(word.start_ms < word.end_ms);
        }
    }

    // Sequential indices (TEST-005)
    let indices: Vec<u32> = t
        .segments
        .iter()
        .flat_map(|s| &s.words)
        .map(|w| w.index)
        .collect();
    assert_eq!(indices[0], 0);
    assert_eq!(*indices.last().unwrap(), t.word_count - 1);
    for (i, &idx) in indices.iter().enumerate() {
        assert_eq!(idx, i as u32);
    }
}
