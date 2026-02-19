use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{
    EditDocument, Shot, ShotNote, ShotRange, SourceIndex, Transcript, TranscriptSegment,
};
use crate::resolve;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DisplayError {
    #[error(transparent)]
    Resolve(#[from] resolve::ResolveError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse JSON from {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
}

// ---------------------------------------------------------------------------
// Resolved shot
// ---------------------------------------------------------------------------

/// A shot resolved against its source transcript/index, ready for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedShot {
    pub id: String,
    pub source: String,
    pub range: ShotRange,
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
    /// For words ranges: first ~80 chars of transcript text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
    /// For scenes ranges: joined scene descriptions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_preview: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ShotNote>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve all shots in an edit document's snapshot against source data on disk.
///
/// Loads transcripts and indices as needed (cached per source), resolves each
/// shot's range to absolute timestamps, and extracts text/scene previews.
///
/// Transcript files are expected at `<project_dir>/transcripts/<source_id>.transcript.json`.
/// Index files are expected at `<project_dir>/index/<source_id>.index.json`.
pub fn resolve_edit(
    doc: &EditDocument,
    project_dir: &Path,
) -> Result<Vec<ResolvedShot>, DisplayError> {
    let mut transcripts: HashMap<String, Option<Transcript>> = HashMap::new();
    let mut indices: HashMap<String, Option<SourceIndex>> = HashMap::new();

    let mut resolved = Vec::with_capacity(doc.snapshot.shots.len());

    for shot in &doc.snapshot.shots {
        let r = resolve_shot(shot, project_dir, &mut transcripts, &mut indices)?;
        resolved.push(r);
    }

    Ok(resolved)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

const PREVIEW_MAX_CHARS: usize = 80;

fn resolve_shot(
    shot: &Shot,
    project_dir: &Path,
    transcripts: &mut HashMap<String, Option<Transcript>>,
    indices: &mut HashMap<String, Option<SourceIndex>>,
) -> Result<ResolvedShot, DisplayError> {
    let transcript = load_transcript_cached(project_dir, &shot.source, transcripts);
    let index = load_index_cached(project_dir, &shot.source, indices);

    let (start_ms, end_ms) = resolve::resolve_range(
        &shot.range,
        transcript.as_ref(),
        index.as_ref(),
    )?;

    let duration_ms = end_ms.saturating_sub(start_ms);

    let text_preview = match &shot.range {
        ShotRange::Words { from, to } => {
            transcript.as_ref().map(|t| extract_text_preview(t, *from, *to))
        }
        _ => None,
    };

    let scene_preview = match &shot.range {
        ShotRange::Scenes { from, to } => {
            index.as_ref().map(|idx| extract_scene_preview(idx, *from, *to))
        }
        _ => None,
    };

    Ok(ResolvedShot {
        id: shot.id.clone(),
        source: shot.source.clone(),
        range: shot.range.clone(),
        start_ms,
        end_ms,
        duration_ms,
        text_preview,
        scene_preview,
        notes: shot.notes.clone(),
    })
}

fn load_transcript_cached<'a>(
    project_dir: &Path,
    source_id: &str,
    cache: &'a mut HashMap<String, Option<Transcript>>,
) -> &'a Option<Transcript> {
    cache.entry(source_id.to_string()).or_insert_with(|| {
        let path = project_dir
            .join("transcripts")
            .join(format!("{source_id}.transcript.json"));
        load_json::<Transcript>(&path).ok()
    })
}

fn load_index_cached<'a>(
    project_dir: &Path,
    source_id: &str,
    cache: &'a mut HashMap<String, Option<SourceIndex>>,
) -> &'a Option<SourceIndex> {
    cache.entry(source_id.to_string()).or_insert_with(|| {
        let path = project_dir
            .join("index")
            .join(format!("{source_id}.index.json"));
        load_json::<SourceIndex>(&path).ok()
    })
}

fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, DisplayError> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| DisplayError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Extract a text preview from transcript words in `[from..=to]`.
fn extract_text_preview(transcript: &Transcript, from: u32, to: u32) -> String {
    let words: Vec<&str> = collect_words_in_range(&transcript.segments, from, to);
    let full = words.join(" ");
    truncate_preview(&full, PREVIEW_MAX_CHARS)
}

fn collect_words_in_range<'a>(segments: &'a [TranscriptSegment], from: u32, to: u32) -> Vec<&'a str> {
    let mut words = Vec::new();
    for seg in segments {
        for w in &seg.words {
            if w.index >= from && w.index <= to {
                words.push(w.text.as_str());
            }
        }
    }
    words
}

/// Extract a preview from scene descriptions in `[from..=to]`.
fn extract_scene_preview(index: &SourceIndex, from: u32, to: u32) -> String {
    let descriptions: Vec<&str> = index
        .scenes
        .iter()
        .filter(|s| s.index >= from && s.index <= to)
        .filter_map(|s| s.description.as_deref())
        .collect();

    if descriptions.is_empty() {
        format!("scenes {from}..{to}")
    } else {
        let full = descriptions.join("; ");
        truncate_preview(&full, PREVIEW_MAX_CHARS)
    }
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max.min(s.len());
        // Don't break in the middle of a multi-byte char.
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Format milliseconds as `MM:SS.sss`.
pub fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let frac = ms % 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}.{frac:03}")
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
                        Word { index: 0, text: "Welcome".into(), start_ms: 0, end_ms: 420, confidence: 0.95 },
                        Word { index: 1, text: "to".into(), start_ms: 420, end_ms: 540, confidence: 0.97 },
                        Word { index: 2, text: "the".into(), start_ms: 540, end_ms: 650, confidence: 0.98 },
                        Word { index: 3, text: "interview".into(), start_ms: 650, end_ms: 1200, confidence: 0.96 },
                    ],
                },
                TranscriptSegment {
                    index: 1,
                    start_ms: 5230,
                    end_ms: 12400,
                    text: "Today we discuss climate".into(),
                    words: vec![
                        Word { index: 4, text: "Today".into(), start_ms: 5230, end_ms: 5600, confidence: 0.94 },
                        Word { index: 5, text: "we".into(), start_ms: 5600, end_ms: 5750, confidence: 0.99 },
                        Word { index: 6, text: "discuss".into(), start_ms: 5750, end_ms: 6200, confidence: 0.93 },
                        Word { index: 7, text: "climate".into(), start_ms: 6200, end_ms: 6800, confidence: 0.91 },
                    ],
                },
            ],
            word_count: 8,
        }
    }

    fn make_source_index() -> SourceIndex {
        SourceIndex {
            source_id: "src-002".into(),
            indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
            metadata: SourceMetadata {
                duration_ms: 90000,
                resolution: (1920, 1080),
                codec: "h264".into(),
                file_size_bytes: 52428800,
            },
            thumbnails: vec![],
            scene_count: 3,
            scenes: vec![
                Scene {
                    index: 0,
                    start_ms: 0,
                    end_ms: 18000,
                    thumbnail: "thumbnails/src-002_00m00s.jpg".into(),
                    description: Some("Interior office, wide shot".into()),
                },
                Scene {
                    index: 1,
                    start_ms: 18000,
                    end_ms: 45000,
                    thumbnail: "thumbnails/src-002_00m18s.jpg".into(),
                    description: None,
                },
                Scene {
                    index: 2,
                    start_ms: 45000,
                    end_ms: 90000,
                    thumbnail: "thumbnails/src-002_00m45s.jpg".into(),
                    description: Some("Close-up interview".into()),
                },
            ],
        }
    }

    fn setup_project(dir: &Path) {
        std::fs::create_dir_all(dir.join("transcripts")).unwrap();
        std::fs::create_dir_all(dir.join("index")).unwrap();
        std::fs::create_dir_all(dir.join("edits")).unwrap();

        let transcript = make_transcript();
        std::fs::write(
            dir.join("transcripts/src-001.transcript.json"),
            serde_json::to_string(&transcript).unwrap(),
        )
        .unwrap();

        let index = make_source_index();
        std::fs::write(
            dir.join("index/src-002.index.json"),
            serde_json::to_string(&index).unwrap(),
        )
        .unwrap();
    }

    // -- text_preview ---------------------------------------------------------

    #[test]
    fn text_preview_from_words() {
        let t = make_transcript();
        let preview = extract_text_preview(&t, 0, 3);
        assert_eq!(preview, "Welcome to the interview");
    }

    #[test]
    fn text_preview_cross_segment() {
        let t = make_transcript();
        let preview = extract_text_preview(&t, 2, 6);
        assert_eq!(preview, "the interview Today we discuss");
    }

    #[test]
    fn text_preview_single_word() {
        let t = make_transcript();
        let preview = extract_text_preview(&t, 4, 4);
        assert_eq!(preview, "Today");
    }

    // -- scene_preview --------------------------------------------------------

    #[test]
    fn scene_preview_with_descriptions() {
        let idx = make_source_index();
        let preview = extract_scene_preview(&idx, 0, 2);
        assert_eq!(preview, "Interior office, wide shot; Close-up interview");
    }

    #[test]
    fn scene_preview_no_descriptions() {
        let idx = make_source_index();
        let preview = extract_scene_preview(&idx, 1, 1);
        // Only scene 1 which has no description
        assert_eq!(preview, "scenes 1..1");
    }

    #[test]
    fn scene_preview_single_scene() {
        let idx = make_source_index();
        let preview = extract_scene_preview(&idx, 0, 0);
        assert_eq!(preview, "Interior office, wide shot");
    }

    // -- truncate_preview -----------------------------------------------------

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate_preview("hello", 80), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(100);
        let result = truncate_preview(&long, 80);
        assert_eq!(result.len(), 83); // 80 + "..."
        assert!(result.ends_with("..."));
    }

    // -- format_time ----------------------------------------------------------

    #[test]
    fn format_time_zero() {
        assert_eq!(format_time(0), "00:00.000");
    }

    #[test]
    fn format_time_typical() {
        assert_eq!(format_time(6800), "00:06.800");
    }

    #[test]
    fn format_time_minutes() {
        assert_eq!(format_time(90000), "01:30.000");
    }

    // -- resolve_edit ---------------------------------------------------------

    #[test]
    fn resolve_edit_words_shot() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });

        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "shot-001");
        assert_eq!(resolved[0].start_ms, 0);
        assert_eq!(resolved[0].end_ms, 1200);
        assert_eq!(resolved[0].duration_ms, 1200);
        assert_eq!(
            resolved[0].text_preview.as_deref(),
            Some("Welcome to the interview")
        );
        assert!(resolved[0].scene_preview.is_none());
    }

    #[test]
    fn resolve_edit_scenes_shot() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 });

        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].start_ms, 0);
        assert_eq!(resolved[0].end_ms, 90000);
        assert_eq!(resolved[0].duration_ms, 90000);
        assert_eq!(
            resolved[0].scene_preview.as_deref(),
            Some("Interior office, wide shot; Close-up interview")
        );
        assert!(resolved[0].text_preview.is_none());
    }

    #[test]
    fn resolve_edit_time_shot() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Time { from_ms: 5000, to_ms: 10000 });

        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].start_ms, 5000);
        assert_eq!(resolved[0].end_ms, 10000);
        assert_eq!(resolved[0].duration_ms, 5000);
        assert!(resolved[0].text_preview.is_none());
        assert!(resolved[0].scene_preview.is_none());
    }

    #[test]
    fn resolve_edit_multiple_shots() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 });
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 });
        doc.add_shot("src-001", ShotRange::Time { from_ms: 1000, to_ms: 3000 });

        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert_eq!(resolved.len(), 3);

        // First shot: words
        assert_eq!(resolved[0].source, "src-001");
        assert!(resolved[0].text_preview.is_some());

        // Second shot: scenes
        assert_eq!(resolved[1].source, "src-002");
        assert!(resolved[1].scene_preview.is_some());

        // Third shot: time
        assert_eq!(resolved[2].source, "src-001");
        assert!(resolved[2].text_preview.is_none());
        assert!(resolved[2].scene_preview.is_none());
    }

    #[test]
    fn resolve_edit_empty_document() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let doc = EditDocument::create("test");
        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolve_edit_preserves_notes() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let mut doc = EditDocument::create("test");
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
        doc.add_note("shot-001", "Great take").unwrap();

        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert_eq!(resolved[0].notes.len(), 1);
        assert_eq!(resolved[0].notes[0].text, "Great take");
    }

    #[test]
    fn resolve_edit_caches_transcripts() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let mut doc = EditDocument::create("test");
        // Two shots from same source — transcript should only be loaded once
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 });
        doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 });

        let resolved = resolve_edit(&doc, tmp.path()).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(
            resolved[0].text_preview.as_deref(),
            Some("Welcome to the interview")
        );
        assert_eq!(
            resolved[1].text_preview.as_deref(),
            Some("Today we discuss climate")
        );
    }
}
