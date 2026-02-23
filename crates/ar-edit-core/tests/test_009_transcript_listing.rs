//! TEST-008: Transcript listing
//!
//! Verifies that `transcript_ops::list()` returns correct entries for
//! transcribed sources in a project. Uses in-memory project setup
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
// Tests
// ---------------------------------------------------------------------------

#[test]
fn list_returns_three_transcribed_sources() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
        make_source("src-003", true),
    ]);

    let t1 = make_transcript("src-001", &[("Hello", 0, 500), ("world", 500, 1000)]);
    let t2 = make_transcript(
        "src-002",
        &[("The", 0, 200), ("economy", 200, 600), ("grows", 600, 1000)],
    );
    let t3 = make_transcript(
        "src-003",
        &[
            ("Climate", 0, 400),
            ("change", 400, 800),
            ("matters", 800, 1200),
        ],
    );

    setup_project(tmp.path(), &manifest, &[t1, t2, t3]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    assert_eq!(result.len(), 3);
}

#[test]
fn list_returns_correct_source_ids() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
        make_source("src-003", true),
    ]);

    let t1 = make_transcript("src-001", &[("Hello", 0, 500)]);
    let t2 = make_transcript("src-002", &[("World", 0, 500)]);
    let t3 = make_transcript("src-003", &[("Test", 0, 500)]);

    setup_project(tmp.path(), &manifest, &[t1, t2, t3]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    let ids: Vec<&str> = result.iter().map(|r| r.source_id.as_str()).collect();
    assert!(ids.contains(&"src-001"));
    assert!(ids.contains(&"src-002"));
    assert!(ids.contains(&"src-003"));
}

#[test]
fn list_returns_correct_durations() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
    ]);

    let t1 = make_transcript("src-001", &[("Hello", 0, 5000)]);
    let t2 = make_transcript("src-002", &[("World", 0, 8000)]);

    setup_project(tmp.path(), &manifest, &[t1, t2]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    let r1 = result.iter().find(|r| r.source_id == "src-001").unwrap();
    let r2 = result.iter().find(|r| r.source_id == "src-002").unwrap();
    assert_eq!(r1.duration_ms, 5000);
    assert_eq!(r2.duration_ms, 8000);
}

#[test]
fn list_returns_correct_word_counts() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", true),
        make_source("src-003", true),
    ]);

    let t1 = make_transcript("src-001", &[("Hello", 0, 500), ("world", 500, 1000)]);
    let t2 = make_transcript(
        "src-002",
        &[("A", 0, 200), ("B", 200, 400), ("C", 400, 600)],
    );
    let t3 = make_transcript("src-003", &[("Only", 0, 500)]);

    setup_project(tmp.path(), &manifest, &[t1, t2, t3]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    let r1 = result.iter().find(|r| r.source_id == "src-001").unwrap();
    let r2 = result.iter().find(|r| r.source_id == "src-002").unwrap();
    let r3 = result.iter().find(|r| r.source_id == "src-003").unwrap();
    assert_eq!(r1.word_count, 2);
    assert_eq!(r2.word_count, 3);
    assert_eq!(r3.word_count, 1);
}

#[test]
fn list_excludes_untranscribed_sources() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![
        make_source("src-001", true),
        make_source("src-002", false), // not transcribed
        make_source("src-003", true),
    ]);

    let t1 = make_transcript("src-001", &[("Hello", 0, 500)]);
    let t3 = make_transcript("src-003", &[("World", 0, 500)]);

    setup_project(tmp.path(), &manifest, &[t1, t3]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.iter().all(|r| r.source_id != "src-002"));
}

#[test]
fn list_empty_project_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![]);
    setup_project(tmp.path(), &manifest, &[]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    assert!(result.is_empty());
}

#[test]
fn list_skips_missing_transcript_files() {
    let tmp = TempDir::new().unwrap();

    // Manifest says transcribed but no file on disk
    let manifest = make_manifest(vec![make_source("src-001", true)]);
    setup_project(tmp.path(), &manifest, &[]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    assert!(result.is_empty());
}

#[test]
fn list_entries_have_correct_paths() {
    let tmp = TempDir::new().unwrap();

    let manifest = make_manifest(vec![make_source("src-001", true)]);
    let t = make_transcript("src-001", &[("Hello", 0, 500)]);
    setup_project(tmp.path(), &manifest, &[t]);

    let result = transcript_ops::list(tmp.path()).unwrap();
    assert_eq!(result[0].path, "transcripts/src-001.transcript.json");
}
