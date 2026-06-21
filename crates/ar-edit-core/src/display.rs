use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use chrono::{DateTime, Utc};

use crate::models::{
    EditDocument, Marker, Shot, ShotNote, ShotRange, SourceIndex, Transcript, TranscriptSegment,
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
    /// Human-readable author of the shot (collaborative attribution, REQ-091).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
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
// Resolved marker
// ---------------------------------------------------------------------------

/// A marker resolved against its source transcript/index, ready for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedMarker {
    pub id: String,
    pub source_id: String,
    pub range: ShotRange,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Human-readable author (collaborative attribution, REQ-091).
    #[serde(default)]
    pub author: String,
    pub created: DateTime<Utc>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
    /// For words ranges: first ~80 chars of transcript text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
    /// For scenes ranges: joined scene descriptions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_preview: Option<String>,
}

/// Resolve a list of markers for a given source against transcript/index data.
pub fn resolve_markers(
    markers: &[Marker],
    source_id: &str,
    project_dir: &Path,
) -> Result<Vec<ResolvedMarker>, DisplayError> {
    let mut transcripts: HashMap<String, Option<Transcript>> = HashMap::new();
    let mut indices: HashMap<String, Option<SourceIndex>> = HashMap::new();

    let mut resolved = Vec::with_capacity(markers.len());

    for marker in markers {
        let transcript = load_transcript_cached(project_dir, source_id, &mut transcripts);
        let index = load_index_cached(project_dir, source_id, &mut indices);

        let resolve_result =
            resolve::resolve_range(&marker.range, transcript.as_ref(), index.as_ref());

        // If resolution fails (e.g. transcript/index missing), fall back to
        // placeholder values so the marker still appears in listings.
        let (start_ms, end_ms, fallback) = match resolve_result {
            Ok((s, e)) => (s, e, false),
            Err(_) => (0, 0, true),
        };
        let duration_ms = end_ms.saturating_sub(start_ms);

        let text_preview = match &marker.range {
            ShotRange::Words { from, to } => {
                if fallback {
                    Some(format!("(transcript not available) words {from}..{to}"))
                } else {
                    transcript
                        .as_ref()
                        .map(|t| extract_text_preview(t, *from, *to))
                }
            }
            _ => None,
        };

        let scene_preview = match &marker.range {
            ShotRange::Scenes { from, to } => {
                if fallback {
                    Some(format!("(index not available) scenes {from}..{to}"))
                } else {
                    index
                        .as_ref()
                        .map(|idx| extract_scene_preview(idx, *from, *to))
                }
            }
            _ => None,
        };

        resolved.push(ResolvedMarker {
            id: marker.id.clone(),
            source_id: source_id.to_string(),
            range: marker.range.clone(),
            label: marker.label.clone(),
            note: marker.note.clone(),
            author: marker.author.clone(),
            created: marker.created,
            start_ms,
            end_ms,
            duration_ms,
            text_preview,
            scene_preview,
        });
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

    let (start_ms, end_ms) =
        resolve::resolve_range(&shot.range, transcript.as_ref(), index.as_ref())?;

    let duration_ms = end_ms.saturating_sub(start_ms);

    let text_preview = match &shot.range {
        ShotRange::Words { from, to } => transcript
            .as_ref()
            .map(|t| extract_text_preview(t, *from, *to)),
        _ => None,
    };

    let scene_preview = match &shot.range {
        ShotRange::Scenes { from, to } => index
            .as_ref()
            .map(|idx| extract_scene_preview(idx, *from, *to)),
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
        author: shot.author.clone(),
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

fn collect_words_in_range(segments: &[TranscriptSegment], from: u32, to: u32) -> Vec<&str> {
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

// ---------------------------------------------------------------------------
// Interleaved transcript + markers (REQ-052)
// ---------------------------------------------------------------------------

/// An item in an interleaved transcript+markers stream.
///
/// Used by `transcripts read --json --with-markers` to produce a
/// single chronological sequence that agents can consume directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TranscriptItem {
    Segment(TranscriptSegment),
    Marker(ResolvedMarker),
}

impl TranscriptItem {
    fn start_ms(&self) -> u64 {
        match self {
            TranscriptItem::Segment(s) => s.start_ms,
            TranscriptItem::Marker(m) => m.start_ms,
        }
    }
}

/// A transcript with markers interleaved at their timestamp positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterleavedTranscript {
    pub source_id: String,
    pub duration_ms: u64,
    pub word_count: u32,
    pub marker_count: usize,
    pub items: Vec<TranscriptItem>,
}

/// Build an interleaved transcript: segments and resolved markers merged
/// into a single list sorted by `start_ms`.
///
/// Markers are resolved against the transcript and source index on disk,
/// then spliced between (or among) the transcript segments in chronological
/// order. When a marker and a segment share the same `start_ms`, the
/// segment appears first.
pub fn interleave_transcript_with_markers(
    transcript: &Transcript,
    markers: &[ResolvedMarker],
) -> InterleavedTranscript {
    let mut items: Vec<TranscriptItem> =
        Vec::with_capacity(transcript.segments.len() + markers.len());

    for seg in &transcript.segments {
        items.push(TranscriptItem::Segment(seg.clone()));
    }

    for marker in markers {
        items.push(TranscriptItem::Marker(marker.clone()));
    }

    // Stable sort: segments before markers when start_ms ties.
    items.sort_by_key(|item| {
        let tie_break = match item {
            TranscriptItem::Segment(_) => 0u8,
            TranscriptItem::Marker(_) => 1u8,
        };
        (item.start_ms(), tie_break)
    });

    InterleavedTranscript {
        source_id: transcript.source_id.clone(),
        duration_ms: transcript.duration_ms,
        word_count: transcript.word_count,
        marker_count: markers.len(),
        items,
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

/// Format milliseconds as `HH:MM:SS.mmm`.
pub fn format_time_hms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let frac = ms % 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{frac:03}")
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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
            .unwrap();

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
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 2 })
            .unwrap();

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
        doc.add_shot(
            "src-001",
            ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
        )
        .unwrap();

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 7 })
            .unwrap();
        doc.add_shot("src-002", ShotRange::Scenes { from: 0, to: 1 })
            .unwrap();
        doc.add_shot(
            "src-001",
            ShotRange::Time {
                from_ms: 1000,
                to_ms: 3000,
            },
        )
        .unwrap();

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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
            .unwrap();
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
        doc.add_shot("src-001", ShotRange::Words { from: 0, to: 3 })
            .unwrap();
        doc.add_shot("src-001", ShotRange::Words { from: 4, to: 7 })
            .unwrap();

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

    // -- interleave_transcript_with_markers -----------------------------------

    fn make_resolved_marker(id: &str, label: &str, start_ms: u64, end_ms: u64) -> ResolvedMarker {
        ResolvedMarker {
            id: id.into(),
            source_id: "src-001".into(),
            range: ShotRange::Time {
                from_ms: start_ms,
                to_ms: end_ms,
            },
            label: label.into(),
            note: None,
            author: "tester".into(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
            start_ms,
            end_ms,
            duration_ms: end_ms.saturating_sub(start_ms),
            text_preview: None,
            scene_preview: None,
        }
    }

    #[test]
    fn interleave_no_markers() {
        let t = make_transcript();
        let result = interleave_transcript_with_markers(&t, &[]);
        assert_eq!(result.source_id, "src-001");
        assert_eq!(result.word_count, 8);
        assert_eq!(result.marker_count, 0);
        assert_eq!(result.items.len(), 2); // 2 segments
        assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
        assert!(matches!(&result.items[1], TranscriptItem::Segment(s) if s.index == 1));
    }

    #[test]
    fn interleave_marker_between_segments() {
        let t = make_transcript();
        let markers = vec![make_resolved_marker("mark-001", "select", 3000, 4000)];
        let result = interleave_transcript_with_markers(&t, &markers);
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.marker_count, 1);
        // seg 0 (0ms), marker (3000ms), seg 1 (5230ms)
        assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
        assert!(matches!(&result.items[1], TranscriptItem::Marker(m) if m.id == "mark-001"));
        assert!(matches!(&result.items[2], TranscriptItem::Segment(s) if s.index == 1));
    }

    #[test]
    fn interleave_marker_at_segment_start() {
        let t = make_transcript();
        // Marker at same start_ms as segment 0 — segment should come first
        let markers = vec![make_resolved_marker("mark-001", "review", 0, 500)];
        let result = interleave_transcript_with_markers(&t, &markers);
        assert_eq!(result.items.len(), 3);
        assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
        assert!(matches!(&result.items[1], TranscriptItem::Marker(m) if m.id == "mark-001"));
    }

    #[test]
    fn interleave_multiple_markers() {
        let t = make_transcript();
        let markers = vec![
            make_resolved_marker("mark-001", "select", 400, 1200),
            make_resolved_marker("mark-002", "avoid", 6000, 6800),
        ];
        let result = interleave_transcript_with_markers(&t, &markers);
        assert_eq!(result.items.len(), 4);
        assert_eq!(result.marker_count, 2);
        // seg 0 (0ms), mark-001 (400ms), seg 1 (5230ms), mark-002 (6000ms)
        assert!(matches!(&result.items[0], TranscriptItem::Segment(s) if s.index == 0));
        assert!(matches!(&result.items[1], TranscriptItem::Marker(m) if m.id == "mark-001"));
        assert!(matches!(&result.items[2], TranscriptItem::Segment(s) if s.index == 1));
        assert!(matches!(&result.items[3], TranscriptItem::Marker(m) if m.id == "mark-002"));
    }

    #[test]
    fn interleave_marker_after_all_segments() {
        let t = make_transcript();
        let markers = vec![make_resolved_marker("mark-001", "note", 100000, 110000)];
        let result = interleave_transcript_with_markers(&t, &markers);
        assert_eq!(result.items.len(), 3);
        assert!(matches!(&result.items[2], TranscriptItem::Marker(m) if m.id == "mark-001"));
    }

    #[test]
    fn interleave_preserves_transcript_metadata() {
        let t = make_transcript();
        let markers = vec![make_resolved_marker("mark-001", "select", 1000, 2000)];
        let result = interleave_transcript_with_markers(&t, &markers);
        assert_eq!(result.source_id, "src-001");
        assert_eq!(result.duration_ms, 124500);
        assert_eq!(result.word_count, 8);
    }

    #[test]
    fn interleave_serialization_roundtrip() {
        let t = make_transcript();
        let markers = vec![make_resolved_marker("mark-001", "select", 3000, 4000)];
        let result = interleave_transcript_with_markers(&t, &markers);
        let json = serde_json::to_value(&result).unwrap();

        // Verify tagged union serialization
        assert_eq!(json["items"][0]["type"], "segment");
        assert_eq!(json["items"][1]["type"], "marker");
        assert_eq!(json["items"][2]["type"], "segment");
        assert_eq!(json["marker_count"], 1);

        let back: InterleavedTranscript = serde_json::from_value(json).unwrap();
        assert_eq!(back.items.len(), 3);
    }

    // -- resolve_markers: missing transcript (Bug #7) -------------------------

    #[test]
    fn resolve_markers_word_range_without_transcript() {
        let tmp = TempDir::new().unwrap();
        // No transcript file on disk — only create the project dirs
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("index")).unwrap();

        let markers = vec![Marker {
            id: "mark-001".into(),
            range: ShotRange::Words { from: 0, to: 10 },
            label: "select".into(),
            note: None,
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }];

        // Previously this would crash with TranscriptRequired error.
        let resolved = resolve_markers(&markers, "src-missing", tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "mark-001");
        assert_eq!(resolved[0].start_ms, 0);
        assert_eq!(resolved[0].end_ms, 0);
        assert_eq!(resolved[0].duration_ms, 0);
        assert_eq!(
            resolved[0].text_preview.as_deref(),
            Some("(transcript not available) words 0..10")
        );
        assert!(resolved[0].scene_preview.is_none());
    }

    #[test]
    fn resolve_markers_scene_range_without_index() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("index")).unwrap();

        let markers = vec![Marker {
            id: "mark-002".into(),
            range: ShotRange::Scenes { from: 0, to: 3 },
            label: "avoid".into(),
            note: Some("bad lighting".into()),
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }];

        let resolved = resolve_markers(&markers, "src-missing", tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, "mark-002");
        assert_eq!(resolved[0].start_ms, 0);
        assert_eq!(resolved[0].end_ms, 0);
        assert_eq!(
            resolved[0].scene_preview.as_deref(),
            Some("(index not available) scenes 0..3")
        );
        assert!(resolved[0].text_preview.is_none());
    }

    #[test]
    fn resolve_markers_time_range_always_succeeds() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("index")).unwrap();

        let markers = vec![Marker {
            id: "mark-003".into(),
            range: ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
            label: "highlight".into(),
            note: None,
            author: String::new(),
            created: "2026-02-19T14:00:00Z".parse().unwrap(),
        }];

        // Time ranges never need transcript/index, so these should always work
        let resolved = resolve_markers(&markers, "src-missing", tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].start_ms, 5000);
        assert_eq!(resolved[0].end_ms, 10000);
        assert_eq!(resolved[0].duration_ms, 5000);
        assert!(resolved[0].text_preview.is_none());
        assert!(resolved[0].scene_preview.is_none());
    }
}
