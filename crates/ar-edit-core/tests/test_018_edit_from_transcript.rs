//! TEST-018: Edit from annotated transcript
//!
//! Verifies the end-to-end workflow of:
//! 1. Creating markers (annotations) on a source transcript
//! 2. Building an edit document from those marker ranges
//! 3. Resolving the edit to verify timestamps and previews
//! 4. Validating the resulting edit against project data

use ar_edit_core::display::resolve_edit;
use ar_edit_core::import::from_transcript_str;
use ar_edit_core::models::*;
use ar_edit_core::validate::validate;
use chrono::Utc;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transcript() -> Transcript {
    Transcript {
        source_id: "src-001".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 30000,
        segments: vec![
            TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: 10000,
                text: "Welcome to the demonstration video for the project".into(),
                words: (0..8)
                    .map(|i| Word {
                        index: i,
                        text: ["Welcome", "to", "the", "demonstration", "video", "for", "the", "project"][i as usize].into(),
                        start_ms: (i as u64) * 1200,
                        end_ms: (i as u64) * 1200 + 1000,
                        confidence: 0.95,
                    })
                    .collect(),
            },
            TranscriptSegment {
                index: 1,
                start_ms: 10000,
                end_ms: 20000,
                text: "Here we show the main features of the editor".into(),
                words: (8..16)
                    .map(|i| Word {
                        index: i,
                        text: ["Here", "we", "show", "the", "main", "features", "of", "the"][i as usize - 8].into(),
                        start_ms: (i as u64) * 1200,
                        end_ms: (i as u64) * 1200 + 1000,
                        confidence: 0.93,
                    })
                    .collect(),
            },
            TranscriptSegment {
                index: 2,
                start_ms: 20000,
                end_ms: 30000,
                text: "Thank you for watching this overview".into(),
                words: (16..22)
                    .map(|i| Word {
                        index: i,
                        text: ["Thank", "you", "for", "watching", "this", "overview"][i as usize - 16].into(),
                        start_ms: (i as u64) * 1200,
                        end_ms: (i as u64) * 1200 + 1000,
                        confidence: 0.96,
                    })
                    .collect(),
            },
        ],
        word_count: 22,
    }
}

fn make_manifest() -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        name: "test-project".into(),
        created: Utc::now(),
        sources: vec![Source {
            id: "src-001".into(),
            path: "sources/src-001.mp4".into(),
            original_filename: "interview.mp4".into(),
            duration_ms: 30000,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            resolution: (1920, 1080),
            frame_rate: 29.97,
            audio_channels: 2,
            audio_sample_rate: 48000,
            added: Utc::now(),
            transcribed: true,
            indexed: false,
        }],
        next_source_id: 2,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    }
}

fn setup_project(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("edits")).unwrap();
    std::fs::create_dir_all(dir.join("annotations")).unwrap();

    let transcript = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&transcript).unwrap(),
    )
    .unwrap();
}

// -- Workflow: annotate then build edit ---------------------------------------

#[test]
fn annotate_then_build_edit_from_markers() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // Step 1: Create markers (annotations) on the transcript
    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![
            Marker {
                id: "mark-001".into(),
                range: ShotRange::Words { from: 0, to: 7 },
                label: "select".into(),
                note: Some("Good opening".into()),
                created: Utc::now(),
            },
            Marker {
                id: "mark-002".into(),
                range: ShotRange::Words { from: 12, to: 15 },
                label: "select".into(),
                note: Some("Key features section".into()),
                created: Utc::now(),
            },
            Marker {
                id: "mark-003".into(),
                range: ShotRange::Words { from: 16, to: 21 },
                label: "select".into(),
                note: None,
                created: Utc::now(),
            },
        ],
    };

    // Save markers to disk
    std::fs::write(
        tmp.path().join("annotations/src-001.markers.json"),
        serde_json::to_string(&markers).unwrap(),
    )
    .unwrap();

    // Step 2: Build edit from selected markers
    let mut doc = EditDocument::create("rough-cut");
    for marker in &markers.markers {
        if marker.label == "select" {
            let shot_id = doc.add_shot("src-001", marker.range.clone()).unwrap().id.clone();

            // Transfer marker note as shot note if present
            if let Some(note) = &marker.note {
                doc.add_note(&shot_id, note.as_str()).unwrap();
            }
        }
    }

    // Step 3: Verify the edit structure
    assert_eq!(doc.snapshot.shots.len(), 3);
    assert_eq!(doc.snapshot.shots[0].range, ShotRange::Words { from: 0, to: 7 });
    assert_eq!(doc.snapshot.shots[1].range, ShotRange::Words { from: 12, to: 15 });
    assert_eq!(doc.snapshot.shots[2].range, ShotRange::Words { from: 16, to: 21 });
    assert_eq!(doc.snapshot.shots[0].notes.len(), 1);
    assert_eq!(doc.snapshot.shots[0].notes[0].text, "Good opening");
    assert_eq!(doc.snapshot.shots[2].notes.len(), 0);

    // Step 4: Resolve the edit to verify timestamps
    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);
    assert!(resolved[0].text_preview.is_some());
    assert!(resolved[1].text_preview.is_some());
    assert!(resolved[2].text_preview.is_some());
    assert!(resolved[0].start_ms < resolved[0].end_ms);

    // Step 5: Validate the edit
    let manifest = make_manifest();
    let validation = validate(&doc, &manifest, tmp.path());
    assert!(validation.valid, "errors: {:?}", validation.errors);
}

#[test]
fn markers_with_avoid_label_are_excluded() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let markers = SourceMarkers {
        source_id: "src-001".into(),
        markers: vec![
            Marker {
                id: "mark-001".into(),
                range: ShotRange::Words { from: 0, to: 7 },
                label: "select".into(),
                note: None,
                created: Utc::now(),
            },
            Marker {
                id: "mark-002".into(),
                range: ShotRange::Words { from: 8, to: 11 },
                label: "avoid".into(),
                note: Some("Bad take".into()),
                created: Utc::now(),
            },
            Marker {
                id: "mark-003".into(),
                range: ShotRange::Words { from: 16, to: 21 },
                label: "select".into(),
                note: None,
                created: Utc::now(),
            },
        ],
    };

    // Only include "select" markers in the edit
    let mut doc = EditDocument::create("filtered-edit");
    for marker in &markers.markers {
        if marker.label == "select" {
            doc.add_shot("src-001", marker.range.clone()).unwrap();
        }
    }

    assert_eq!(doc.snapshot.shots.len(), 2);
    assert_eq!(doc.snapshot.shots[0].range, ShotRange::Words { from: 0, to: 7 });
    assert_eq!(doc.snapshot.shots[1].range, ShotRange::Words { from: 16, to: 21 });
}

#[test]
fn edit_from_markers_with_reordering() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    // Build edit from markers, then reorder
    let mut doc = EditDocument::create("reordered");
    doc.add_shot("src-001", ShotRange::Words { from: 16, to: 21 }).unwrap(); // closing
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 }).unwrap();   // opening
    doc.add_shot("src-001", ShotRange::Words { from: 8, to: 15 }).unwrap();  // middle

    // Reorder: move opening to front
    doc.move_shot("shot-002", 0).unwrap();

    assert_eq!(doc.snapshot.shots[0].range, ShotRange::Words { from: 0, to: 7 });
    assert_eq!(doc.snapshot.shots[1].range, ShotRange::Words { from: 16, to: 21 });
    assert_eq!(doc.snapshot.shots[2].range, ShotRange::Words { from: 8, to: 15 });
}

#[test]
fn edit_from_markers_roundtrip() {
    let tmp = TempDir::new().unwrap();
    setup_project(tmp.path());

    let mut doc = EditDocument::create("roundtrip");
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 }).unwrap();
    doc.add_shot("src-001", ShotRange::Words { from: 12, to: 15 }).unwrap();
    doc.add_note("shot-001", "Opening sequence").unwrap();

    let path = tmp.path().join("edits/roundtrip.edit.json");
    doc.save(&path).unwrap();

    let loaded = EditDocument::load(&path).unwrap();
    assert_eq!(loaded.snapshot.shots.len(), 2);
    assert_eq!(loaded.snapshot.shots[0].notes.len(), 1);
    assert_eq!(loaded.snapshot.shots[0].notes[0].text, "Opening sequence");

    let recomputed = EditDocument::recompute_snapshot(&loaded.ops, loaded.head);
    assert_eq!(recomputed, loaded.snapshot);
}

// -- Import from annotated markdown with multiple sources ---------------------

fn make_transcript_src002() -> Transcript {
    Transcript {
        source_id: "src-002".into(),
        model: "base".into(),
        language: "en".into(),
        duration_ms: 10000,
        segments: vec![TranscriptSegment {
            index: 0,
            start_ms: 0,
            end_ms: 10000,
            text: "The economy has shown strong growth this quarter".into(),
            words: (0..8)
                .map(|i| Word {
                    index: i,
                    text: [
                        "The", "economy", "has", "shown", "strong", "growth",
                        "this", "quarter",
                    ][i as usize]
                        .into(),
                    start_ms: (i as u64) * 1200,
                    end_ms: (i as u64) * 1200 + 1000,
                    confidence: 0.94,
                })
                .collect(),
        }],
        word_count: 8,
    }
}

fn make_manifest_multi() -> Manifest {
    Manifest {
        version: "1.0.0".into(),
        name: "test-project".into(),
        created: Utc::now(),
        sources: vec![
            Source {
                id: "src-001".into(),
                path: "sources/src-001.mp4".into(),
                original_filename: "interview.mp4".into(),
                duration_ms: 30000,
                video_codec: "h264".into(),
                audio_codec: "aac".into(),
                resolution: (1920, 1080),
                frame_rate: 29.97,
                audio_channels: 2,
                audio_sample_rate: 48000,
                added: Utc::now(),
                transcribed: true,
                indexed: false,
            },
            Source {
                id: "src-002".into(),
                path: "sources/src-002.mp4".into(),
                original_filename: "economy.mp4".into(),
                duration_ms: 10000,
                video_codec: "h264".into(),
                audio_codec: "aac".into(),
                resolution: (1920, 1080),
                frame_rate: 29.97,
                audio_channels: 2,
                audio_sample_rate: 48000,
                added: Utc::now(),
                transcribed: true,
                indexed: false,
            },
        ],
        next_source_id: 3,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    }
}

fn setup_project_multi(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("transcripts")).unwrap();
    std::fs::create_dir_all(dir.join("index")).unwrap();
    std::fs::create_dir_all(dir.join("edits")).unwrap();
    std::fs::create_dir_all(dir.join("annotations")).unwrap();

    let t1 = make_transcript();
    std::fs::write(
        dir.join("transcripts/src-001.transcript.json"),
        serde_json::to_string(&t1).unwrap(),
    )
    .unwrap();

    let t2 = make_transcript_src002();
    std::fs::write(
        dir.join("transcripts/src-002.transcript.json"),
        serde_json::to_string(&t2).unwrap(),
    )
    .unwrap();
}

#[test]
fn import_annotated_transcript_three_segments_two_sources() {
    let tmp = TempDir::new().unwrap();
    setup_project_multi(tmp.path());

    // Annotated markdown with 3 segments from 2 sources:
    // - 2 from src-001 (opening + closing, skipping the middle segment)
    // - 1 from src-002
    let markdown = "\
# Source: src-001 — interview.mp4

<!-- ar-edit:src-001:w0-w7 -->
Welcome to the demonstration video for the project
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w16-w21 -->
Thank you for watching this overview
<!-- /ar-edit:src-001 -->

---

# Source: src-002 — economy.mp4

<!-- ar-edit:src-002:w0-w7 -->
The economy has shown strong growth this quarter
<!-- /ar-edit:src-002 -->
";

    // Step 1: Import from annotated transcript
    let doc = from_transcript_str(markdown, "multi-source-edit").unwrap();

    // Step 2: Verify edit structure — 3 shots from 2 sources
    assert_eq!(doc.name, "multi-source-edit");
    assert_eq!(doc.snapshot.shots.len(), 3);

    // Shot 1: src-001, words 0-7 (opening)
    assert_eq!(doc.snapshot.shots[0].source, "src-001");
    assert_eq!(
        doc.snapshot.shots[0].range,
        ShotRange::Words { from: 0, to: 7 }
    );

    // Shot 2: src-001, words 16-21 (closing)
    assert_eq!(doc.snapshot.shots[1].source, "src-001");
    assert_eq!(
        doc.snapshot.shots[1].range,
        ShotRange::Words { from: 16, to: 21 }
    );

    // Shot 3: src-002, words 0-7
    assert_eq!(doc.snapshot.shots[2].source, "src-002");
    assert_eq!(
        doc.snapshot.shots[2].range,
        ShotRange::Words { from: 0, to: 7 }
    );

    // Step 3: Verify sequential shot IDs
    assert_eq!(doc.snapshot.shots[0].id, "shot-001");
    assert_eq!(doc.snapshot.shots[1].id, "shot-002");
    assert_eq!(doc.snapshot.shots[2].id, "shot-003");

    // Step 4: Verify ops match
    assert_eq!(doc.ops.len(), 3);
    assert_eq!(doc.head, 2);

    // Step 5: Resolve the edit (loads transcripts from disk)
    let resolved = resolve_edit(&doc, tmp.path()).unwrap();
    assert_eq!(resolved.len(), 3);

    // Verify resolved shots have text previews and valid timestamp ranges
    for r in &resolved {
        assert!(r.text_preview.is_some());
        assert!(r.start_ms < r.end_ms);
        assert!(r.duration_ms > 0);
    }

    // Verify resolved shots reference correct sources
    assert_eq!(resolved[0].source, "src-001");
    assert_eq!(resolved[1].source, "src-001");
    assert_eq!(resolved[2].source, "src-002");

    // Step 6: Validate the edit against the project
    let manifest = make_manifest_multi();
    let validation = validate(&doc, &manifest, tmp.path());
    assert!(validation.valid, "errors: {:?}", validation.errors);
}
