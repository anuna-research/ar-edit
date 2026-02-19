//! TEST-020: Export then reimport round-trip produces identical edit
//!
//! Verifies the full export → reimport cycle:
//! 1. Set up a project with transcribed sources
//! 2. Export transcripts as annotated markdown via `export_editable()`
//! 3. Reimport the markdown via `from_transcript_str()`
//! 4. Verify the resulting edit matches the original transcript structure

use ar_edit_core::export::export_editable;
use ar_edit_core::import::from_transcript_str;
use ar_edit_core::models::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_source(id: &str, original_filename: &str) -> Source {
    Source {
        id: id.into(),
        path: format!("sources/{id}.mp4").into(),
        original_filename: original_filename.into(),
        duration_ms: 30000,
        video_codec: "h264".into(),
        audio_codec: "aac".into(),
        resolution: (1920, 1080),
        frame_rate: 29.97,
        audio_channels: 2,
        audio_sample_rate: 48000,
        added: "2026-02-19T12:00:00Z".parse().unwrap(),
        transcribed: true,
        indexed: false,
    }
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

fn make_transcript_with_segments(
    source_id: &str,
    segments: Vec<(Vec<(&str, u64, u64)>, &str)>,
) -> Transcript {
    let mut all_segments = Vec::new();
    let mut global_idx: u32 = 0;
    let mut duration_ms: u64 = 0;

    for (seg_idx, (words_data, seg_text)) in segments.into_iter().enumerate() {
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

        let seg_end = words.last().map_or(0, |w| w.end_ms);
        let seg_start = words.first().map_or(0, |w| w.start_ms);

        all_segments.push(TranscriptSegment {
            index: seg_idx as u32,
            start_ms: seg_start,
            end_ms: seg_end,
            text: seg_text.to_string(),
            words,
        });
    }

    Transcript {
        source_id: source_id.to_string(),
        model: "base".to_string(),
        language: "en".to_string(),
        duration_ms,
        word_count: global_idx,
        segments: all_segments,
    }
}

fn setup_project(dir: &std::path::Path, manifest: &Manifest, transcripts: &[Transcript]) {
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

// -- Single source round-trip ------------------------------------------------

#[test]
fn roundtrip_single_source_single_segment() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", "interview.mp4")]);
    let transcript = make_transcript_with_segments(
        "src-001",
        vec![(
            vec![
                ("Welcome", 0, 420),
                ("to", 420, 540),
                ("the", 540, 650),
                ("interview", 650, 1200),
            ],
            "Welcome to the interview",
        )],
    );
    setup_project(tmp.path(), &manifest, &[transcript]);

    let markdown = export_editable(tmp.path()).unwrap();
    let doc = from_transcript_str(&markdown, "roundtrip").unwrap();

    assert_eq!(doc.snapshot.shots.len(), 1);
    assert_eq!(doc.snapshot.shots[0].source, "src-001");
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 0, to: 3 }
    );
}

#[test]
fn roundtrip_single_source_multiple_segments() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", "interview.mp4")]);
    let transcript = make_transcript_with_segments(
        "src-001",
        vec![
            (
                vec![
                    ("Welcome", 0, 420),
                    ("to", 420, 540),
                    ("the", 540, 650),
                    ("interview", 650, 5230),
                ],
                "Welcome to the interview",
            ),
            (
                vec![
                    ("today", 5230, 5800),
                    ("we", 5800, 6200),
                    ("talk", 6200, 6800),
                ],
                "today we talk",
            ),
        ],
    );
    setup_project(tmp.path(), &manifest, &[transcript]);

    let markdown = export_editable(tmp.path()).unwrap();
    let doc = from_transcript_str(&markdown, "roundtrip").unwrap();

    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 0, to: 3 }
    );
    assert_eq!(
        doc.snapshot.shots[1].range,
        ShotRange::Words { from: 4, to: 6 }
    );
}

// -- Multi-source round-trip -------------------------------------------------

#[test]
fn roundtrip_multiple_sources() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", "interview-alice.mp4"),
        make_source("src-002", "interview-bob.mp4"),
    ]);
    let t1 = make_transcript_with_segments(
        "src-001",
        vec![(
            vec![
                ("Hello", 0, 500),
                ("from", 500, 800),
                ("Alice", 800, 1200),
            ],
            "Hello from Alice",
        )],
    );
    let t2 = make_transcript_with_segments(
        "src-002",
        vec![(
            vec![
                ("Hi", 0, 300),
                ("from", 300, 600),
                ("Bob", 600, 900),
            ],
            "Hi from Bob",
        )],
    );
    setup_project(tmp.path(), &manifest, &[t1, t2]);

    let markdown = export_editable(tmp.path()).unwrap();
    let doc = from_transcript_str(&markdown, "multi-roundtrip").unwrap();

    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].source, "src-001");
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 0, to: 2 }
    );
    assert_eq!(doc.snapshot.shots[1].source, "src-002");
    assert_eq!(
        doc.snapshot.shots[1].range,
        ShotRange::Words { from: 0, to: 2 }
    );
}

// -- Idempotent round-trip ---------------------------------------------------

#[test]
fn roundtrip_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", "test.mp4"),
        make_source("src-002", "test2.mp4"),
    ]);
    let t1 = make_transcript_with_segments(
        "src-001",
        vec![
            (
                vec![
                    ("Welcome", 0, 420),
                    ("to", 420, 540),
                    ("the", 540, 650),
                    ("interview", 650, 5230),
                ],
                "Welcome to the interview",
            ),
            (
                vec![
                    ("today", 5230, 5800),
                    ("we", 5800, 6200),
                    ("talk", 6200, 6800),
                ],
                "today we talk",
            ),
        ],
    );
    let t2 = make_transcript_with_segments(
        "src-002",
        vec![(
            vec![
                ("The", 0, 300),
                ("economy", 300, 700),
                ("grew", 700, 1000),
            ],
            "The economy grew",
        )],
    );
    setup_project(tmp.path(), &manifest, &[t1, t2]);

    // First round-trip
    let markdown1 = export_editable(tmp.path()).unwrap();
    let doc1 = from_transcript_str(&markdown1, "trip1").unwrap();

    // Second round-trip (same project state)
    let markdown2 = export_editable(tmp.path()).unwrap();
    let doc2 = from_transcript_str(&markdown2, "trip2").unwrap();

    // Exported markdown should be identical
    assert_eq!(markdown1, markdown2);

    // Edit documents should have identical shots (ignoring name/timestamps)
    assert_eq!(doc1.snapshot.shots.len(), doc2.snapshot.shots.len());
    for (s1, s2) in doc1.snapshot.shots.iter().zip(doc2.snapshot.shots.iter()) {
        assert_eq!(s1.source, s2.source);
        assert_eq!(s1.range, s2.range);
    }
}

// -- Snapshot recomputation --------------------------------------------------

#[test]
fn roundtrip_snapshot_matches_recomputed() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![make_source("src-001", "test.mp4")]);
    let transcript = make_transcript_with_segments(
        "src-001",
        vec![
            (
                vec![
                    ("Welcome", 0, 420),
                    ("to", 420, 540),
                    ("the", 540, 650),
                    ("interview", 650, 5230),
                ],
                "Welcome to the interview",
            ),
            (
                vec![
                    ("today", 5230, 5800),
                    ("we", 5800, 6200),
                    ("talk", 6200, 6800),
                ],
                "today we talk",
            ),
        ],
    );
    setup_project(tmp.path(), &manifest, &[transcript]);

    let markdown = export_editable(tmp.path()).unwrap();
    let doc = from_transcript_str(&markdown, "recompute").unwrap();

    let recomputed = EditDocument::recompute_snapshot(&doc.ops, doc.head);
    assert_eq!(recomputed, doc.snapshot);
}

// -- Persistence roundtrip ---------------------------------------------------

#[test]
fn roundtrip_survives_save_and_load() {
    let tmp = TempDir::new().unwrap();
    let manifest = make_manifest(vec![
        make_source("src-001", "test.mp4"),
        make_source("src-002", "test2.mp4"),
    ]);
    let t1 = make_transcript_with_segments(
        "src-001",
        vec![(
            vec![("Hello", 0, 500), ("world", 500, 1000)],
            "Hello world",
        )],
    );
    let t2 = make_transcript_with_segments(
        "src-002",
        vec![(
            vec![("Good", 0, 400), ("morning", 400, 800)],
            "Good morning",
        )],
    );
    setup_project(tmp.path(), &manifest, &[t1, t2]);

    let markdown = export_editable(tmp.path()).unwrap();
    let doc = from_transcript_str(&markdown, "persist").unwrap();

    // Save to disk
    std::fs::create_dir_all(tmp.path().join("edits")).unwrap();
    let edit_path = tmp.path().join("edits/persist.edit.json");
    doc.save(&edit_path).unwrap();

    // Load back
    let loaded = EditDocument::load(&edit_path).unwrap();
    assert_eq!(loaded.snapshot, doc.snapshot);
    assert_eq!(loaded.ops.len(), doc.ops.len());
    assert_eq!(loaded.head, doc.head);

    // Verify shots survived
    assert_eq!(loaded.snapshot.shots.len(), 2);
    assert_eq!(loaded.snapshot.shots[0].source, "src-001");
    assert_eq!(loaded.snapshot.shots[1].source, "src-002");
}
