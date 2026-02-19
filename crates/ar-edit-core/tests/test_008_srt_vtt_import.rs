//! TEST-007: SRT/VTT import
//!
//! Verifies that `transcript::import_transcript()` correctly parses SRT and
//! VTT subtitle files into Transcript structs with interpolated word
//! timestamps. No external tool dependencies.

use std::fs;

use ar_edit_core::models::Transcript;
use ar_edit_core::transcript::{import_transcript, parse_srt, parse_vtt};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// SRT import
// ---------------------------------------------------------------------------

#[test]
fn srt_import_produces_correct_segment_count() {
    let srt = "\
1
00:00:00,000 --> 00:00:05,230
Welcome to the interview

2
00:00:05,230 --> 00:00:11,800
This is the second segment

3
00:00:11,800 --> 00:00:18,000
And the third segment ends here";

    let t = parse_srt(srt, "src-001").unwrap();
    assert_eq!(t.segments.len(), 3);
}

#[test]
fn srt_import_word_count_matches_input() {
    let srt = "\
1
00:00:00,000 --> 00:00:05,230
Welcome to the interview

2
00:00:05,230 --> 00:00:11,800
This is the second segment";

    let t = parse_srt(srt, "src-001").unwrap();
    // "Welcome to the interview" = 4 words + "This is the second segment" = 5 words = 9
    assert_eq!(t.word_count, 9);
}

#[test]
fn srt_import_preserves_segment_timestamps() {
    let srt = "\
1
00:00:00,000 --> 00:00:05,230
Hello world";

    let t = parse_srt(srt, "src-001").unwrap();
    assert_eq!(t.segments[0].start_ms, 0);
    assert_eq!(t.segments[0].end_ms, 5230);
}

#[test]
fn srt_import_interpolates_word_timestamps() {
    let srt = "\
1
00:00:00,000 --> 00:00:04,000
one two three four";

    let t = parse_srt(srt, "src-001").unwrap();
    let words = &t.segments[0].words;
    assert_eq!(words.len(), 4);

    // Evenly distributed: 0-1000, 1000-2000, 2000-3000, 3000-4000
    assert_eq!(words[0].start_ms, 0);
    assert_eq!(words[0].end_ms, 1000);
    assert_eq!(words[1].start_ms, 1000);
    assert_eq!(words[1].end_ms, 2000);
    assert_eq!(words[2].start_ms, 2000);
    assert_eq!(words[2].end_ms, 3000);
    assert_eq!(words[3].start_ms, 3000);
    assert_eq!(words[3].end_ms, 4000);
}

#[test]
fn srt_import_sets_confidence_to_one() {
    let srt = "\
1
00:00:00,000 --> 00:00:03,000
Hello world";

    let t = parse_srt(srt, "src-001").unwrap();
    for word in &t.segments[0].words {
        assert_eq!(word.confidence, 1.0);
    }
}

#[test]
fn srt_import_sequential_word_indices() {
    let srt = "\
1
00:00:00,000 --> 00:00:05,000
Hello beautiful world

2
00:00:05,000 --> 00:00:10,000
How are you today";

    let t = parse_srt(srt, "src-001").unwrap();
    let indices: Vec<u32> = t
        .segments
        .iter()
        .flat_map(|s| &s.words)
        .map(|w| w.index)
        .collect();
    assert_eq!(indices, vec![0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn srt_import_model_is_imported() {
    let srt = "\
1
00:00:00,000 --> 00:00:03,000
Hello";

    let t = parse_srt(srt, "src-001").unwrap();
    assert_eq!(t.model, "imported");
}

#[test]
fn srt_import_language_is_undetermined() {
    let srt = "\
1
00:00:00,000 --> 00:00:03,000
Hello";

    let t = parse_srt(srt, "src-001").unwrap();
    assert_eq!(t.language, "und");
}

#[test]
fn srt_import_duration_from_last_segment() {
    let srt = "\
1
00:00:00,000 --> 00:00:05,000
First

2
00:00:05,000 --> 00:00:12,500
Second";

    let t = parse_srt(srt, "src-001").unwrap();
    assert_eq!(t.duration_ms, 12500);
}

// ---------------------------------------------------------------------------
// VTT import
// ---------------------------------------------------------------------------

#[test]
fn vtt_import_produces_correct_segment_count() {
    let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.230
Welcome to the interview

00:00:05.230 --> 00:00:11.800
This is the second segment";

    let t = parse_vtt(vtt, "src-001").unwrap();
    assert_eq!(t.segments.len(), 2);
}

#[test]
fn vtt_import_word_count_matches_input() {
    let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.230
Welcome to the interview

00:00:05.230 --> 00:00:11.800
This is the second segment";

    let t = parse_vtt(vtt, "src-001").unwrap();
    assert_eq!(t.word_count, 9);
}

#[test]
fn vtt_import_preserves_segment_timestamps() {
    let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.230
Hello world";

    let t = parse_vtt(vtt, "src-001").unwrap();
    assert_eq!(t.segments[0].start_ms, 0);
    assert_eq!(t.segments[0].end_ms, 5230);
}

#[test]
fn vtt_import_interpolates_word_timestamps() {
    let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:04.000
one two three four";

    let t = parse_vtt(vtt, "src-001").unwrap();
    let words = &t.segments[0].words;
    assert_eq!(words.len(), 4);

    assert_eq!(words[0].start_ms, 0);
    assert_eq!(words[0].end_ms, 1000);
    assert_eq!(words[3].start_ms, 3000);
    assert_eq!(words[3].end_ms, 4000);
}

#[test]
fn vtt_import_sequential_word_indices_across_segments() {
    let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.000
Hello world

00:00:05.000 --> 00:00:10.000
How are you";

    let t = parse_vtt(vtt, "src-001").unwrap();
    let indices: Vec<u32> = t
        .segments
        .iter()
        .flat_map(|s| &s.words)
        .map(|w| w.index)
        .collect();
    assert_eq!(indices, vec![0, 1, 2, 3, 4]);
}

#[test]
fn vtt_import_rejects_missing_header() {
    let vtt = "00:00:00.000 --> 00:00:05.000\nHello";
    assert!(parse_vtt(vtt, "src-001").is_err());
}

#[test]
fn vtt_import_handles_short_timestamps() {
    let vtt = "\
WEBVTT

00:05.000 --> 00:10.000
Short format";

    let t = parse_vtt(vtt, "src-001").unwrap();
    assert_eq!(t.segments[0].start_ms, 5000);
    assert_eq!(t.segments[0].end_ms, 10000);
}

#[test]
fn vtt_import_skips_note_blocks() {
    let vtt = "\
WEBVTT

NOTE This is a comment

00:00:00.000 --> 00:00:05.000
Actual content";

    let t = parse_vtt(vtt, "src-001").unwrap();
    assert_eq!(t.segments.len(), 1);
    assert_eq!(t.segments[0].text, "Actual content");
    assert_eq!(t.segments[0].index, 0);
}

// ---------------------------------------------------------------------------
// import_transcript (file-based detection)
// ---------------------------------------------------------------------------

#[test]
fn import_transcript_detects_srt_by_extension() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.srt");
    fs::write(
        &path,
        "1\n00:00:00,000 --> 00:00:03,000\nHello world\n",
    )
    .unwrap();

    let t = import_transcript(&path, "src-001").unwrap();
    assert_eq!(t.model, "imported");
    assert_eq!(t.word_count, 2);
}

#[test]
fn import_transcript_detects_vtt_by_extension() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.vtt");
    fs::write(
        &path,
        "WEBVTT\n\n00:00:00.000 --> 00:00:03.000\nHello world\n",
    )
    .unwrap();

    let t = import_transcript(&path, "src-001").unwrap();
    assert_eq!(t.model, "imported");
    assert_eq!(t.word_count, 2);
}

#[test]
fn import_transcript_detects_json_by_extension() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.json");
    fs::write(
        &path,
        r#"{"result":{"language":"en"},"transcription":[{"offsets":{"from":0,"to":3000},"text":" Hello world","tokens":[{"text":" Hello","offsets":{"from":0,"to":1500},"p":0.9},{"text":" world","offsets":{"from":1500,"to":3000},"p":0.85}]}]}"#,
    )
    .unwrap();

    let t = import_transcript(&path, "src-001").unwrap();
    assert_eq!(t.language, "en");
    assert_eq!(t.word_count, 2);
}

#[test]
fn import_transcript_rejects_unsupported_format() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.ass");
    fs::write(&path, "some content").unwrap();

    let err = import_transcript(&path, "src-001").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("unsupported"));
}

#[test]
fn srt_roundtrips_through_serde() {
    let srt = "\
1
00:00:00,000 --> 00:00:05,230
Welcome to the interview

2
00:00:05,230 --> 00:00:11,800
This is the second segment";

    let t = parse_srt(srt, "src-001").unwrap();
    let json = serde_json::to_string(&t).unwrap();
    let back: Transcript = serde_json::from_str(&json).unwrap();
    assert_eq!(t, back);
}

#[test]
fn vtt_roundtrips_through_serde() {
    let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.230
Welcome to the interview

00:00:05.230 --> 00:00:11.800
This is the second segment";

    let t = parse_vtt(vtt, "src-001").unwrap();
    let json = serde_json::to_string(&t).unwrap();
    let back: Transcript = serde_json::from_str(&json).unwrap();
    assert_eq!(t, back);
}
