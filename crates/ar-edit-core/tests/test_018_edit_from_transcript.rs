//! TEST-018: Edit from annotated transcript
//!
//! Verifies the end-to-end workflow of:
//! 1. Creating markers (annotations) on a source transcript
//! 2. Building an edit document from those marker ranges
//! 3. Resolving the edit to verify timestamps and previews
//! 4. Validating the resulting edit against project data

use ar_edit_core::display::resolve_edit;
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
            let shot_id = doc.add_shot("src-001", marker.range.clone()).id.clone();

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
            doc.add_shot("src-001", marker.range.clone());
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
    doc.add_shot("src-001", ShotRange::Words { from: 16, to: 21 }); // closing
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 });   // opening
    doc.add_shot("src-001", ShotRange::Words { from: 8, to: 15 });  // middle

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
    doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 });
    doc.add_shot("src-001", ShotRange::Words { from: 12, to: 15 });
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
