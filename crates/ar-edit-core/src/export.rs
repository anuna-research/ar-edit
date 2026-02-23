use std::fmt::Write as _;
use std::path::Path;

use crate::models::{Manifest, Transcript};
use crate::transcript_ops::TranscriptOpsError;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Export all project transcripts as annotated markdown (segment-level format).
///
/// Produces a single markdown document with per-source sections. Each segment
/// is wrapped in HTML comment annotations that encode the source ID and word
/// index range:
///
/// ```markdown
/// # Source: src-001 — interview-alice.mp4
///
/// <!-- ar-edit:src-001:w0-w8 -->
/// Welcome to the interview today we're going to talk about
/// <!-- /ar-edit:src-001 -->
/// ```
///
/// This format is designed so that deleting, reordering, or splitting blocks
/// preserves enough annotation data for `ar-edit edit from-transcript` to
/// reconstruct source/word-index pairs.
pub fn export_editable(project_dir: &Path) -> Result<String, TranscriptOpsError> {
    let manifest = load_manifest(project_dir)?;
    let mut output = String::new();
    let mut first = true;

    for source in &manifest.sources {
        if !source.transcribed {
            continue;
        }

        let path = project_dir.join(format!("transcripts/{}.transcript.json", source.id));
        let transcript = match load_transcript(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };

        if !first {
            output.push_str("\n---\n\n");
        }
        first = false;

        writeln!(
            output,
            "# Source: {} — {}\n",
            source.id, source.original_filename
        )
        .unwrap();

        for seg in &transcript.segments {
            if seg.words.is_empty() {
                // Segment with no word-level data — emit text without annotations
                writeln!(output, "{}\n", seg.text).unwrap();
                continue;
            }

            let first_word = seg.words.first().unwrap().index;
            let last_word = seg.words.last().unwrap().index;

            writeln!(
                output,
                "<!-- ar-edit:{}:w{}-w{} -->",
                source.id, first_word, last_word
            )
            .unwrap();
            writeln!(output, "{}", seg.text).unwrap();
            writeln!(output, "<!-- /ar-edit:{} -->\n", source.id).unwrap();
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn load_manifest(project_dir: &Path) -> Result<Manifest, TranscriptOpsError> {
    let path = project_dir.join("manifest.json");
    let data = std::fs::read_to_string(&path)?;
    serde_json::from_str(&data).map_err(|e| TranscriptOpsError::Json { path, source: e })
}

fn load_transcript(path: &Path) -> Result<Transcript, TranscriptOpsError> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| TranscriptOpsError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use tempfile::TempDir;

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

    fn make_source(id: &str, transcribed: bool, original_filename: &str) -> Source {
        Source {
            id: id.into(),
            path: format!("sources/{id}.mp4").into(),
            original_filename: original_filename.into(),
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

    #[test]
    fn export_single_source_single_segment() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, "interview-alice.mp4")]);
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

        let result = export_editable(tmp.path()).unwrap();

        assert!(result.contains("# Source: src-001 — interview-alice.mp4"));
        assert!(result.contains("<!-- ar-edit:src-001:w0-w3 -->"));
        assert!(result.contains("Welcome to the interview"));
        assert!(result.contains("<!-- /ar-edit:src-001 -->"));
    }

    #[test]
    fn export_single_source_multiple_segments() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, "interview-alice.mp4")]);
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

        let result = export_editable(tmp.path()).unwrap();

        // First segment: words 0-3
        assert!(result.contains("<!-- ar-edit:src-001:w0-w3 -->"));
        assert!(result.contains("Welcome to the interview"));
        // Second segment: words 4-6
        assert!(result.contains("<!-- ar-edit:src-001:w4-w6 -->"));
        assert!(result.contains("today we talk"));
    }

    #[test]
    fn export_multiple_sources_separated_by_hr() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true, "interview-alice.mp4"),
            make_source("src-002", true, "interview-bob.mp4"),
        ]);
        let t1 = make_transcript_with_segments(
            "src-001",
            vec![(vec![("Hello", 0, 500), ("world", 500, 1000)], "Hello world")],
        );
        let t2 = make_transcript_with_segments(
            "src-002",
            vec![(vec![("So", 0, 300), ("the", 300, 600)], "So the")],
        );
        setup_project(tmp.path(), &manifest, &[t1, t2]);

        let result = export_editable(tmp.path()).unwrap();

        assert!(result.contains("# Source: src-001 — interview-alice.mp4"));
        assert!(result.contains("---"));
        assert!(result.contains("# Source: src-002 — interview-bob.mp4"));

        // Verify ordering: src-001 before src-002
        let pos1 = result.find("src-001").unwrap();
        let pos2 = result.find("src-002").unwrap();
        assert!(pos1 < pos2);
    }

    #[test]
    fn export_skips_untranscribed_sources() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true, "interview-alice.mp4"),
            make_source("src-002", false, "b-roll.mp4"),
        ]);
        let t1 = make_transcript_with_segments("src-001", vec![(vec![("Hello", 0, 500)], "Hello")]);
        setup_project(tmp.path(), &manifest, &[t1]);

        let result = export_editable(tmp.path()).unwrap();

        assert!(result.contains("src-001"));
        assert!(!result.contains("src-002"));
        assert!(!result.contains("---")); // only one source, no separator
    }

    #[test]
    fn export_empty_project() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![]);
        setup_project(tmp.path(), &manifest, &[]);

        let result = export_editable(tmp.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn export_word_indices_are_global() {
        // Verify that word indices in annotations are global (not per-segment)
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, "test.mp4")]);
        let transcript = make_transcript_with_segments(
            "src-001",
            vec![
                (
                    vec![
                        ("Welcome", 0, 420),
                        ("to", 420, 540),
                        ("the", 540, 650),
                        ("interview", 650, 5230),
                        ("today", 5230, 5800),
                        ("we're", 5800, 6200),
                        ("going", 6200, 6800),
                        ("to", 6800, 7000),
                        ("talk", 7000, 7400),
                    ],
                    "Welcome to the interview today we're going to talk",
                ),
                (
                    vec![
                        ("the", 7400, 7800),
                        ("impact", 7800, 8200),
                        ("of", 8200, 8600),
                        ("climate", 8600, 9000),
                        ("policy", 9000, 9400),
                        ("on", 9400, 9800),
                        ("regional", 9800, 10200),
                        ("communities", 10200, 11800),
                    ],
                    "the impact of climate policy on regional communities",
                ),
            ],
        );
        setup_project(tmp.path(), &manifest, &[transcript]);

        let result = export_editable(tmp.path()).unwrap();

        // First segment: w0-w8
        assert!(result.contains("<!-- ar-edit:src-001:w0-w8 -->"));
        // Second segment: w9-w16 (global, not restarting at 0)
        assert!(result.contains("<!-- ar-edit:src-001:w9-w16 -->"));
    }

    #[test]
    fn export_annotations_survive_text_roundtrip() {
        // Verify annotations parse back to the correct source/word pairs
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, "test.mp4")]);
        let transcript = make_transcript_with_segments(
            "src-001",
            vec![(vec![("Hello", 0, 500), ("world", 500, 1000)], "Hello world")],
        );
        setup_project(tmp.path(), &manifest, &[transcript]);

        let result = export_editable(tmp.path()).unwrap();

        // Parse annotations back
        let re = regex::Regex::new(r"<!-- ar-edit:([^:]+):w(\d+)-w(\d+) -->").unwrap();
        let caps: Vec<_> = re.captures_iter(&result).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(&caps[0][1], "src-001");
        assert_eq!(&caps[0][2], "0");
        assert_eq!(&caps[0][3], "1");
    }

    #[test]
    fn export_skips_sources_with_missing_transcript_file() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true, "test.mp4"), // transcribed but no file
            make_source("src-002", true, "test2.mp4"),
        ]);
        let t2 = make_transcript_with_segments("src-002", vec![(vec![("Hello", 0, 500)], "Hello")]);
        setup_project(tmp.path(), &manifest, &[t2]);

        let result = export_editable(tmp.path()).unwrap();
        assert!(!result.contains("src-001"));
        assert!(result.contains("src-002"));
    }
}
