use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

use crate::display::ResolvedShot;
use crate::models::{ShotRange, Transcript, TranscriptSegment};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SubtitleError {
    #[error("ffmpeg subtitle embedding failed: {0}")]
    FfmpegFailed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse transcript JSON from {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
}

// ---------------------------------------------------------------------------
// SRT generation
// ---------------------------------------------------------------------------

/// A single subtitle cue.
#[derive(Debug, Clone)]
struct SubtitleCue {
    index: u32,
    start_ms: u64,
    end_ms: u64,
    text: String,
}

/// Format milliseconds as SRT timecode: `HH:MM:SS,mmm`.
fn format_srt_timecode(ms: u64) -> String {
    let total_secs = ms / 1000;
    let frac = ms % 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02},{frac:03}")
}

/// Generate SRT subtitle content from resolved edit shots.
///
/// For each shot, loads the transcript for the shot's source (if available)
/// and extracts word-level timing within the shot's time range. Words are
/// grouped by their transcript segment boundaries to produce natural subtitle
/// lines. Timing is adjusted to the output timeline.
///
/// Shots whose source has no transcript are silently skipped.
pub fn generate_srt(
    resolved: &[ResolvedShot],
    project_dir: &Path,
) -> Result<String, SubtitleError> {
    let mut transcripts: HashMap<String, Option<Transcript>> = HashMap::new();
    let mut cues: Vec<SubtitleCue> = Vec::new();
    let mut cue_index: u32 = 1;
    let mut timeline_offset_ms: u64 = 0;

    for shot in resolved {
        let transcript = load_transcript_cached(project_dir, &shot.source, &mut transcripts);

        if let Some(ref transcript) = transcript {
            let shot_cues = extract_shot_cues(transcript, shot, timeline_offset_ms, &mut cue_index);
            cues.extend(shot_cues);
        }

        timeline_offset_ms += shot.duration_ms;
    }

    Ok(format_srt(&cues))
}

/// Extract subtitle cues for a single shot from its transcript.
///
/// Finds words within the shot's source time range and groups them by
/// transcript segment. Each group becomes a subtitle cue with timing
/// adjusted to the output timeline.
fn extract_shot_cues(
    transcript: &Transcript,
    shot: &ResolvedShot,
    timeline_offset_ms: u64,
    cue_index: &mut u32,
) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();

    // Determine word index range for Words-type shots, or find words by time
    let word_groups = match &shot.range {
        ShotRange::Words { from, to } => {
            collect_word_groups_by_index(&transcript.segments, *from, *to)
        }
        _ => collect_word_groups_by_time(&transcript.segments, shot.start_ms, shot.end_ms),
    };

    for group in &word_groups {
        if group.words.is_empty() {
            continue;
        }

        let first = &group.words[0];
        let last = &group.words[group.words.len() - 1];

        // Compute output timeline timing:
        // output_start = (word_source_start - shot_source_start) + timeline_offset
        let output_start = first.start_ms.saturating_sub(shot.start_ms) + timeline_offset_ms;
        let output_end = last.end_ms.saturating_sub(shot.start_ms) + timeline_offset_ms;

        let text: String = group
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        cues.push(SubtitleCue {
            index: *cue_index,
            start_ms: output_start,
            end_ms: output_end,
            text,
        });
        *cue_index += 1;
    }

    cues
}

/// A group of words from a single transcript segment.
struct WordGroup {
    words: Vec<WordRef>,
}

/// Lightweight reference to a word's timing and text.
struct WordRef {
    text: String,
    start_ms: u64,
    end_ms: u64,
}

/// Collect words by global word index range, grouped by segment.
fn collect_word_groups_by_index(
    segments: &[TranscriptSegment],
    from: u32,
    to: u32,
) -> Vec<WordGroup> {
    let mut groups = Vec::new();

    for seg in segments {
        let words: Vec<WordRef> = seg
            .words
            .iter()
            .filter(|w| w.index >= from && w.index <= to)
            .map(|w| WordRef {
                text: w.text.clone(),
                start_ms: w.start_ms,
                end_ms: w.end_ms,
            })
            .collect();

        if !words.is_empty() {
            groups.push(WordGroup { words });
        }
    }

    groups
}

/// Collect words by source time range, grouped by segment.
fn collect_word_groups_by_time(
    segments: &[TranscriptSegment],
    start_ms: u64,
    end_ms: u64,
) -> Vec<WordGroup> {
    let mut groups = Vec::new();

    for seg in segments {
        let words: Vec<WordRef> = seg
            .words
            .iter()
            .filter(|w| w.start_ms >= start_ms && w.end_ms <= end_ms)
            .map(|w| WordRef {
                text: w.text.clone(),
                start_ms: w.start_ms,
                end_ms: w.end_ms,
            })
            .collect();

        if !words.is_empty() {
            groups.push(WordGroup { words });
        }
    }

    groups
}

/// Format subtitle cues as SRT text.
fn format_srt(cues: &[SubtitleCue]) -> String {
    let mut output = String::new();

    for (i, cue) in cues.iter().enumerate() {
        if i > 0 {
            output.push('\n');
        }
        output.push_str(&format!("{}\n", cue.index));
        output.push_str(&format!(
            "{} --> {}\n",
            format_srt_timecode(cue.start_ms),
            format_srt_timecode(cue.end_ms),
        ));
        output.push_str(&cue.text);
        output.push('\n');
    }

    output
}

// ---------------------------------------------------------------------------
// SRT embedding via ffmpeg
// ---------------------------------------------------------------------------

/// Embed an SRT subtitle file into a video using ffmpeg.
///
/// Runs: `ffmpeg -i <video> -i <srt> -c copy -c:s mov_text <output>`
///
/// The input video is replaced by the output (via a temp file and rename).
pub fn embed_subtitles(video: &Path, srt: &Path) -> Result<(), SubtitleError> {
    // Write to a temp file next to the video, then rename
    let temp_output = video.with_extension("tmp.mp4");

    let result = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(video)
        .args(["-i"])
        .arg(srt)
        .args(["-c", "copy", "-c:s", "mov_text"])
        .arg(&temp_output)
        .output()
        .map_err(|e| SubtitleError::FfmpegFailed(format!("failed to run ffmpeg: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        // Clean up temp file on failure
        let _ = std::fs::remove_file(&temp_output);
        return Err(SubtitleError::FfmpegFailed(format!(
            "subtitle embedding failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    // Replace original with the subtitled version
    std::fs::rename(&temp_output, video)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Transcript loading (mirrors display.rs pattern)
// ---------------------------------------------------------------------------

fn load_transcript_cached<'a>(
    project_dir: &Path,
    source_id: &str,
    cache: &'a mut HashMap<String, Option<Transcript>>,
) -> &'a Option<Transcript> {
    cache.entry(source_id.to_string()).or_insert_with(|| {
        let path = project_dir
            .join("transcripts")
            .join(format!("{source_id}.transcript.json"));
        load_transcript(&path).ok()
    })
}

fn load_transcript(path: &Path) -> Result<Transcript, SubtitleError> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| SubtitleError::Json {
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

    fn make_transcript() -> Transcript {
        Transcript {
            source_id: "src-001".into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms: 124500,
            segments: vec![
                TranscriptSegment {
                    index: 0,
                    start_ms: 0,
                    end_ms: 5230,
                    text: "Welcome to the interview".into(),
                    words: vec![
                        Word {
                            index: 0,
                            text: "Welcome".into(),
                            start_ms: 0,
                            end_ms: 420,
                            confidence: 0.95,
                        },
                        Word {
                            index: 1,
                            text: "to".into(),
                            start_ms: 420,
                            end_ms: 540,
                            confidence: 0.97,
                        },
                        Word {
                            index: 2,
                            text: "the".into(),
                            start_ms: 540,
                            end_ms: 650,
                            confidence: 0.98,
                        },
                        Word {
                            index: 3,
                            text: "interview".into(),
                            start_ms: 650,
                            end_ms: 1200,
                            confidence: 0.96,
                        },
                    ],
                },
                TranscriptSegment {
                    index: 1,
                    start_ms: 5230,
                    end_ms: 12400,
                    text: "Today we discuss climate".into(),
                    words: vec![
                        Word {
                            index: 4,
                            text: "Today".into(),
                            start_ms: 5230,
                            end_ms: 5600,
                            confidence: 0.94,
                        },
                        Word {
                            index: 5,
                            text: "we".into(),
                            start_ms: 5600,
                            end_ms: 5750,
                            confidence: 0.99,
                        },
                        Word {
                            index: 6,
                            text: "discuss".into(),
                            start_ms: 5750,
                            end_ms: 6200,
                            confidence: 0.93,
                        },
                        Word {
                            index: 7,
                            text: "climate".into(),
                            start_ms: 6200,
                            end_ms: 6800,
                            confidence: 0.91,
                        },
                    ],
                },
            ],
            word_count: 8,
        }
    }

    // -- format_srt_timecode --------------------------------------------------

    #[test]
    fn timecode_zero() {
        assert_eq!(format_srt_timecode(0), "00:00:00,000");
    }

    #[test]
    fn timecode_milliseconds_only() {
        assert_eq!(format_srt_timecode(500), "00:00:00,500");
    }

    #[test]
    fn timecode_seconds() {
        assert_eq!(format_srt_timecode(5230), "00:00:05,230");
    }

    #[test]
    fn timecode_minutes() {
        assert_eq!(format_srt_timecode(90000), "00:01:30,000");
    }

    #[test]
    fn timecode_hours() {
        assert_eq!(format_srt_timecode(3661500), "01:01:01,500");
    }

    // -- format_srt -----------------------------------------------------------

    #[test]
    fn format_srt_empty() {
        assert_eq!(format_srt(&[]), "");
    }

    #[test]
    fn format_srt_single_cue() {
        let cues = vec![SubtitleCue {
            index: 1,
            start_ms: 0,
            end_ms: 1200,
            text: "Welcome to the interview".into(),
        }];
        let srt = format_srt(&cues);
        assert_eq!(
            srt,
            "1\n00:00:00,000 --> 00:00:01,200\nWelcome to the interview\n"
        );
    }

    #[test]
    fn format_srt_multiple_cues() {
        let cues = vec![
            SubtitleCue {
                index: 1,
                start_ms: 0,
                end_ms: 1200,
                text: "Welcome to the interview".into(),
            },
            SubtitleCue {
                index: 2,
                start_ms: 5230,
                end_ms: 6800,
                text: "Today we discuss climate".into(),
            },
        ];
        let srt = format_srt(&cues);
        let expected = "\
1\n\
00:00:00,000 --> 00:00:01,200\n\
Welcome to the interview\n\
\n\
2\n\
00:00:05,230 --> 00:00:06,800\n\
Today we discuss climate\n";
        assert_eq!(srt, expected);
    }

    // -- collect_word_groups_by_index -----------------------------------------

    #[test]
    fn word_groups_by_index_same_segment() {
        let t = make_transcript();
        let groups = collect_word_groups_by_index(&t.segments, 0, 3);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].words.len(), 4);
        assert_eq!(groups[0].words[0].text, "Welcome");
        assert_eq!(groups[0].words[3].text, "interview");
    }

    #[test]
    fn word_groups_by_index_cross_segment() {
        let t = make_transcript();
        let groups = collect_word_groups_by_index(&t.segments, 2, 6);
        assert_eq!(groups.len(), 2);
        // First group: words 2-3 from segment 0
        assert_eq!(groups[0].words.len(), 2);
        assert_eq!(groups[0].words[0].text, "the");
        // Second group: words 4-6 from segment 1
        assert_eq!(groups[1].words.len(), 3);
        assert_eq!(groups[1].words[0].text, "Today");
    }

    #[test]
    fn word_groups_by_index_single_word() {
        let t = make_transcript();
        let groups = collect_word_groups_by_index(&t.segments, 4, 4);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].words.len(), 1);
        assert_eq!(groups[0].words[0].text, "Today");
    }

    // -- collect_word_groups_by_time ------------------------------------------

    #[test]
    fn word_groups_by_time_within_segment() {
        let t = make_transcript();
        let groups = collect_word_groups_by_time(&t.segments, 0, 1200);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].words.len(), 4);
    }

    #[test]
    fn word_groups_by_time_across_segments() {
        let t = make_transcript();
        let groups = collect_word_groups_by_time(&t.segments, 0, 6800);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn word_groups_by_time_partial_segment() {
        let t = make_transcript();
        // Only words 0 and 1 fit within 0..540
        let groups = collect_word_groups_by_time(&t.segments, 0, 540);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].words.len(), 2);
    }

    // -- extract_shot_cues ----------------------------------------------------

    #[test]
    fn shot_cues_words_range() {
        let t = make_transcript();
        let shot = ResolvedShot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 3 },
            start_ms: 0,
            end_ms: 1200,
            duration_ms: 1200,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
        };

        let mut idx = 1;
        let cues = extract_shot_cues(&t, &shot, 0, &mut idx);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].index, 1);
        assert_eq!(cues[0].start_ms, 0);
        assert_eq!(cues[0].end_ms, 1200);
        assert_eq!(cues[0].text, "Welcome to the interview");
        assert_eq!(idx, 2);
    }

    #[test]
    fn shot_cues_with_timeline_offset() {
        let t = make_transcript();
        let shot = ResolvedShot {
            id: "shot-002".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 4, to: 7 },
            start_ms: 5230,
            end_ms: 6800,
            duration_ms: 1570,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
        };

        // Suppose this shot appears after 1200ms of previous content
        let mut idx = 2;
        let cues = extract_shot_cues(&t, &shot, 1200, &mut idx);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].index, 2);
        // output_start = (5230 - 5230) + 1200 = 1200
        assert_eq!(cues[0].start_ms, 1200);
        // output_end = (6800 - 5230) + 1200 = 2770
        assert_eq!(cues[0].end_ms, 2770);
        assert_eq!(cues[0].text, "Today we discuss climate");
    }

    #[test]
    fn shot_cues_cross_segment() {
        let t = make_transcript();
        let shot = ResolvedShot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 7 },
            start_ms: 0,
            end_ms: 6800,
            duration_ms: 6800,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
        };

        let mut idx = 1;
        let cues = extract_shot_cues(&t, &shot, 0, &mut idx);
        // Two cues: one per segment
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Welcome to the interview");
        assert_eq!(cues[0].start_ms, 0);
        assert_eq!(cues[0].end_ms, 1200);
        assert_eq!(cues[1].text, "Today we discuss climate");
        assert_eq!(cues[1].start_ms, 5230);
        assert_eq!(cues[1].end_ms, 6800);
    }

    #[test]
    fn shot_cues_time_range() {
        let t = make_transcript();
        let shot = ResolvedShot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 6800,
            },
            start_ms: 0,
            end_ms: 6800,
            duration_ms: 6800,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
        };

        let mut idx = 1;
        let cues = extract_shot_cues(&t, &shot, 0, &mut idx);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Welcome to the interview");
        assert_eq!(cues[1].text, "Today we discuss climate");
    }

    // -- generate_srt (integration) -------------------------------------------

    #[test]
    fn generate_srt_with_transcript() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();

        let transcript = make_transcript();
        std::fs::write(
            tmp.path().join("transcripts/src-001.transcript.json"),
            serde_json::to_string(&transcript).unwrap(),
        )
        .unwrap();

        let resolved = vec![
            ResolvedShot {
                id: "shot-001".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 3 },
                start_ms: 0,
                end_ms: 1200,
                duration_ms: 1200,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
            ResolvedShot {
                id: "shot-002".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 4, to: 7 },
                start_ms: 5230,
                end_ms: 6800,
                duration_ms: 1570,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
        ];

        let srt = generate_srt(&resolved, tmp.path()).unwrap();

        // Cue 1: shot-001 words 0-3, timeline offset 0
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:01,200\nWelcome to the interview"));
        // Cue 2: shot-002 words 4-7, timeline offset 1200ms
        // output_start = (5230-5230) + 1200 = 1200
        // output_end = (6800-5230) + 1200 = 2770
        assert!(srt.contains("2\n00:00:01,200 --> 00:00:02,770\nToday we discuss climate"));
    }

    #[test]
    fn generate_srt_no_transcript() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();

        let resolved = vec![ResolvedShot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 0,
                to_ms: 5000,
            },
            start_ms: 0,
            end_ms: 5000,
            duration_ms: 5000,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
        }];

        // No transcript file — should produce empty SRT
        let srt = generate_srt(&resolved, tmp.path()).unwrap();
        assert!(srt.is_empty());
    }

    #[test]
    fn generate_srt_empty_edit() {
        let tmp = TempDir::new().unwrap();
        let srt = generate_srt(&[], tmp.path()).unwrap();
        assert!(srt.is_empty());
    }

    #[test]
    fn generate_srt_mixed_sources() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();

        // Only src-001 has a transcript
        let transcript = make_transcript();
        std::fs::write(
            tmp.path().join("transcripts/src-001.transcript.json"),
            serde_json::to_string(&transcript).unwrap(),
        )
        .unwrap();

        let resolved = vec![
            // Shot from source with transcript
            ResolvedShot {
                id: "shot-001".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 3 },
                start_ms: 0,
                end_ms: 1200,
                duration_ms: 1200,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
            // Shot from source without transcript (skipped)
            ResolvedShot {
                id: "shot-002".into(),
                source: "src-002".into(),
                range: ShotRange::Time {
                    from_ms: 0,
                    to_ms: 5000,
                },
                start_ms: 0,
                end_ms: 5000,
                duration_ms: 5000,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
            // Shot from source with transcript again
            ResolvedShot {
                id: "shot-003".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 4, to: 7 },
                start_ms: 5230,
                end_ms: 6800,
                duration_ms: 1570,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
        ];

        let srt = generate_srt(&resolved, tmp.path()).unwrap();

        // Should have 2 cues (shot-002 skipped since src-002 has no transcript)
        // Verify correct number of cues by checking sequential indices
        // Verify cue indices are sequential
        assert!(srt.contains("1\n"));
        assert!(srt.contains("2\n"));
        // Cue 2 timeline offset: 1200 (shot-001) + 5000 (shot-002) = 6200
        // output_start = (5230-5230) + 6200 = 6200
        assert!(srt.contains("00:00:06,200 -->"));
    }
}
