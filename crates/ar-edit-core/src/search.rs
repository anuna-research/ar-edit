use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{Manifest, SourceIndex, Transcript};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("invalid regex '{pattern}': {source}")]
    InvalidRegex {
        pattern: String,
        source: regex::Error,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse JSON from {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The type of a unified search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultType {
    Transcript,
    Scene,
    Metadata,
}

/// Filter for which result types to include.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeFilter {
    Transcript,
    Scene,
    Metadata,
}

/// A unified search result spanning transcripts, scenes, and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub result_type: ResultType,
    pub source_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub matched_text: String,
    pub context: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Search across transcripts, scene descriptions, and metadata (markers).
///
/// Returns a unified list of results from all search domains. Results can
/// be filtered by source and type. The query is treated as a case-insensitive
/// regex pattern.
pub fn search(
    project_dir: &Path,
    query: &str,
    source_filter: Option<&str>,
    type_filter: Option<&TypeFilter>,
) -> Result<Vec<SearchResult>, SearchError> {
    let re = Regex::new(&format!("(?i){query}")).map_err(|e| SearchError::InvalidRegex {
        pattern: query.to_string(),
        source: e,
    })?;

    let manifest = load_manifest(project_dir)?;
    let mut results = Vec::new();

    for source in &manifest.sources {
        if let Some(filter) = source_filter {
            if source.id != filter {
                continue;
            }
        }

        // Search transcripts
        if (type_filter.is_none() || type_filter == Some(&TypeFilter::Transcript))
            && source.transcribed
        {
            let path = project_dir.join(format!("transcripts/{}.transcript.json", source.id));
            if let Ok(transcript) = load_transcript(&path) {
                search_transcript(&transcript, &re, &mut results);
            }
        }

        // Search scene descriptions
        if (type_filter.is_none() || type_filter == Some(&TypeFilter::Scene)) && source.indexed {
            let path = project_dir.join(format!("index/{}.index.json", source.id));
            if let Ok(index) = load_index(&path) {
                search_scenes(&index, &re, &mut results);
            }
        }

        // Search metadata (marker labels and notes)
        if type_filter.is_none() || type_filter == Some(&TypeFilter::Metadata) {
            let path = project_dir.join(format!("annotations/{}.markers.json", source.id));
            if path.exists() {
                if let Ok(markers) = load_markers(&path) {
                    search_markers(&markers, &re, &mut results);
                }
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn load_manifest(project_dir: &Path) -> Result<Manifest, SearchError> {
    let path = project_dir.join("manifest.json");
    let data = std::fs::read_to_string(&path)?;
    serde_json::from_str(&data).map_err(|e| SearchError::Json { path, source: e })
}

fn load_transcript(path: &Path) -> Result<Transcript, SearchError> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| SearchError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

fn load_index(path: &Path) -> Result<SourceIndex, SearchError> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| SearchError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

fn load_markers(path: &Path) -> Result<crate::models::SourceMarkers, SearchError> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(|e| SearchError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Number of context words to include before/after a transcript match.
const CONTEXT_WORDS: usize = 5;

/// Search within a single transcript and append results.
fn search_transcript(transcript: &Transcript, re: &Regex, results: &mut Vec<SearchResult>) {
    struct WordEntry {
        text: String,
        start_ms: u64,
        end_ms: u64,
    }

    let words: Vec<WordEntry> = transcript
        .segments
        .iter()
        .flat_map(|seg| {
            seg.words.iter().map(|w| WordEntry {
                text: w.text.clone(),
                start_ms: w.start_ms,
                end_ms: w.end_ms,
            })
        })
        .collect();

    if words.is_empty() {
        return;
    }

    // Build a full text string with word boundaries tracked.
    let mut full_text = String::new();
    let mut word_char_starts: Vec<usize> = Vec::with_capacity(words.len());
    let mut word_char_ends: Vec<usize> = Vec::with_capacity(words.len());

    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            full_text.push(' ');
        }
        word_char_starts.push(full_text.len());
        full_text.push_str(&w.text);
        word_char_ends.push(full_text.len());
    }

    for m in re.find_iter(&full_text) {
        let match_start = m.start();
        let match_end = m.end();

        let from_word_pos = match word_char_starts.binary_search(&match_start) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };

        let to_word_pos = match word_char_ends.binary_search(&match_end) {
            Ok(i) => i,
            Err(i) => i.min(words.len() - 1),
        };

        let start_ms = words[from_word_pos].start_ms;
        let end_ms = words[to_word_pos].end_ms;

        let matched_words: Vec<&str> = words[from_word_pos..=to_word_pos]
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        let matched_text = matched_words.join(" ");

        let word_texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        let context_before = build_context(&word_texts, from_word_pos, CONTEXT_WORDS, true);
        let context_after = build_context(&word_texts, to_word_pos, CONTEXT_WORDS, false);

        let mut context = String::new();
        if !context_before.is_empty() {
            context.push_str(&format!("...{} ", context_before));
        }
        context.push_str(&format!("[{}]", matched_text));
        if !context_after.is_empty() {
            context.push_str(&format!(" {}...", context_after));
        }

        results.push(SearchResult {
            result_type: ResultType::Transcript,
            source_id: transcript.source_id.clone(),
            start_ms,
            end_ms,
            matched_text,
            context,
        });
    }
}

/// Search scene descriptions in an index.
fn search_scenes(index: &SourceIndex, re: &Regex, results: &mut Vec<SearchResult>) {
    for scene in &index.scenes {
        if let Some(ref desc) = scene.description {
            if re.is_match(desc) {
                results.push(SearchResult {
                    result_type: ResultType::Scene,
                    source_id: index.source_id.clone(),
                    start_ms: scene.start_ms,
                    end_ms: scene.end_ms,
                    matched_text: desc.clone(),
                    context: format!("scene {}: {}", scene.index, desc),
                });
            }
        }
    }
}

/// Search marker labels and notes.
fn search_markers(
    markers: &crate::models::SourceMarkers,
    re: &Regex,
    results: &mut Vec<SearchResult>,
) {
    for marker in &markers.markers {
        let label_match = re.is_match(&marker.label);
        let note_match = marker
            .note
            .as_ref()
            .map(|n| re.is_match(n))
            .unwrap_or(false);

        if label_match || note_match {
            let (start_ms, end_ms) = range_to_ms(&marker.range);

            let matched_text = if note_match {
                marker.note.clone().unwrap_or_default()
            } else {
                marker.label.clone()
            };

            let context = match &marker.note {
                Some(note) => format!("{} [{}] \"{}\"", marker.id, marker.label, note),
                None => format!("{} [{}]", marker.id, marker.label),
            };

            results.push(SearchResult {
                result_type: ResultType::Metadata,
                source_id: markers.source_id.clone(),
                start_ms,
                end_ms,
                matched_text,
                context,
            });
        }
    }
}

/// Extract start/end milliseconds from a ShotRange.
///
/// For word and scene ranges, the timestamps are not directly available
/// without resolving against transcript/index data. We use the raw
/// numeric values as placeholders — these are indices, not ms.
/// Time ranges provide ms directly.
fn range_to_ms(range: &crate::models::ShotRange) -> (u64, u64) {
    match range {
        crate::models::ShotRange::Time { from_ms, to_ms } => (*from_ms, *to_ms),
        crate::models::ShotRange::Words { from, to } => (*from as u64, *to as u64),
        crate::models::ShotRange::Scenes { from, to } => (*from as u64, *to as u64),
    }
}

/// Build context string from surrounding words.
fn build_context(words: &[&str], word_pos: usize, count: usize, before: bool) -> String {
    if before {
        let start = word_pos.saturating_sub(count);
        if start == word_pos {
            return String::new();
        }
        words[start..word_pos].join(" ")
    } else {
        let end = (word_pos + 1 + count).min(words.len());
        if word_pos + 1 >= end {
            return String::new();
        }
        words[word_pos + 1..end].join(" ")
    }
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

    fn make_source(id: &str, transcribed: bool, indexed: bool) -> Source {
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
            indexed,
        }
    }

    fn make_transcript(source_id: &str, words_data: &[(&str, u64, u64)]) -> Transcript {
        let mut global_idx: u32 = 0;
        let mut duration_ms: u64 = 0;

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

        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let segments = vec![TranscriptSegment {
            index: 0,
            start_ms: words_data.first().map_or(0, |w| w.1),
            end_ms: words_data.last().map_or(0, |w| w.2),
            text,
            words,
        }];

        Transcript {
            source_id: source_id.into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms,
            word_count: global_idx,
            segments,
        }
    }

    fn make_index(source_id: &str, scenes: Vec<Scene>) -> SourceIndex {
        let scene_count = scenes.len() as u32;
        SourceIndex {
            source_id: source_id.into(),
            indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
            metadata: SourceMetadata {
                duration_ms: 124500,
                resolution: (1920, 1080),
                codec: "h264".into(),
                file_size_bytes: 52428800,
            },
            thumbnails: vec![],
            scene_count,
            scenes,
        }
    }

    fn make_scene(index: u32, start_ms: u64, end_ms: u64, description: Option<&str>) -> Scene {
        Scene {
            index,
            start_ms,
            end_ms,
            thumbnail: Default::default(),
            description: description.map(|s| s.to_string()),
        }
    }

    fn setup_project(
        dir: &Path,
        manifest: &Manifest,
        transcripts: &[Transcript],
        indexes: &[SourceIndex],
        markers: &[SourceMarkers],
    ) {
        std::fs::create_dir_all(dir.join("transcripts")).unwrap();
        std::fs::create_dir_all(dir.join("index")).unwrap();
        std::fs::create_dir_all(dir.join("annotations")).unwrap();

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

        for idx in indexes {
            std::fs::write(
                dir.join(format!("index/{}.index.json", idx.source_id)),
                serde_json::to_string_pretty(idx).unwrap(),
            )
            .unwrap();
        }

        for m in markers {
            std::fs::write(
                dir.join(format!("annotations/{}.markers.json", m.source_id)),
                serde_json::to_string_pretty(m).unwrap(),
            )
            .unwrap();
        }
    }

    // -- transcript search ---------------------------------------------------

    #[test]
    fn search_finds_transcript_matches() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, false)]);
        let t = make_transcript(
            "src-001",
            &[
                ("The", 0, 200),
                ("climate", 200, 600),
                ("policy", 600, 1000),
                ("has", 1000, 1200),
                ("changed", 1200, 1600),
            ],
        );
        setup_project(tmp.path(), &manifest, &[t], &[], &[]);

        let results = search(tmp.path(), "climate", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Transcript);
        assert_eq!(results[0].source_id, "src-001");
        assert_eq!(results[0].start_ms, 200);
        assert_eq!(results[0].end_ms, 600);
        assert_eq!(results[0].matched_text, "climate");
    }

    // -- scene search --------------------------------------------------------

    #[test]
    fn search_finds_scene_description_matches() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", false, true)]);
        let idx = make_index(
            "src-001",
            vec![
                make_scene(0, 0, 18000, Some("Interior office, wide shot")),
                make_scene(1, 18000, 45000, Some("Close-up interview")),
                make_scene(2, 45000, 87000, None),
            ],
        );
        setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

        let results = search(tmp.path(), "interview", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Scene);
        assert_eq!(results[0].source_id, "src-001");
        assert_eq!(results[0].start_ms, 18000);
        assert_eq!(results[0].end_ms, 45000);
        assert_eq!(results[0].matched_text, "Close-up interview");
        assert!(results[0].context.contains("scene 1:"));
    }

    // -- metadata search -----------------------------------------------------

    #[test]
    fn search_finds_marker_label_matches() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", false, false)]);
        let markers = SourceMarkers {
            source_id: "src-001".into(),
            markers: vec![
                Marker {
                    id: "mark-001".into(),
                    range: ShotRange::Time {
                        from_ms: 5000,
                        to_ms: 10000,
                    },
                    label: "hero".into(),
                    note: Some("Best take".into()),
                    created: "2026-02-19T14:00:00Z".parse().unwrap(),
                },
                Marker {
                    id: "mark-002".into(),
                    range: ShotRange::Time {
                        from_ms: 20000,
                        to_ms: 25000,
                    },
                    label: "avoid".into(),
                    note: None,
                    created: "2026-02-19T14:01:00Z".parse().unwrap(),
                },
            ],
        };
        setup_project(tmp.path(), &manifest, &[], &[], &[markers]);

        let results = search(tmp.path(), "hero", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Metadata);
        assert_eq!(results[0].source_id, "src-001");
        assert_eq!(results[0].start_ms, 5000);
        assert_eq!(results[0].end_ms, 10000);
        assert_eq!(results[0].matched_text, "hero");
    }

    #[test]
    fn search_finds_marker_note_matches() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", false, false)]);
        let markers = SourceMarkers {
            source_id: "src-001".into(),
            markers: vec![Marker {
                id: "mark-001".into(),
                range: ShotRange::Time {
                    from_ms: 5000,
                    to_ms: 10000,
                },
                label: "select".into(),
                note: Some("Best take of the climate answer".into()),
                created: "2026-02-19T14:00:00Z".parse().unwrap(),
            }],
        };
        setup_project(tmp.path(), &manifest, &[], &[], &[markers]);

        let results = search(tmp.path(), "climate answer", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Metadata);
        assert!(results[0].matched_text.contains("climate answer"));
    }

    // -- unified search across types -----------------------------------------

    #[test]
    fn search_returns_results_from_all_types() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, true)]);
        let t = make_transcript(
            "src-001",
            &[
                ("The", 0, 200),
                ("office", 200, 600),
                ("meeting", 600, 1000),
            ],
        );
        let idx = make_index(
            "src-001",
            vec![make_scene(0, 0, 18000, Some("Interior office, wide shot"))],
        );
        let markers = SourceMarkers {
            source_id: "src-001".into(),
            markers: vec![Marker {
                id: "mark-001".into(),
                range: ShotRange::Time {
                    from_ms: 0,
                    to_ms: 5000,
                },
                label: "select".into(),
                note: Some("Good office shot".into()),
                created: "2026-02-19T14:00:00Z".parse().unwrap(),
            }],
        };
        setup_project(tmp.path(), &manifest, &[t], &[idx], &[markers]);

        let results = search(tmp.path(), "office", None, None).unwrap();
        assert_eq!(results.len(), 3);

        let types: Vec<&ResultType> = results.iter().map(|r| &r.result_type).collect();
        assert!(types.contains(&&ResultType::Transcript));
        assert!(types.contains(&&ResultType::Scene));
        assert!(types.contains(&&ResultType::Metadata));
    }

    // -- source filter -------------------------------------------------------

    #[test]
    fn search_filters_by_source() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true, false),
            make_source("src-002", true, false),
        ]);
        let t1 = make_transcript("src-001", &[("climate", 0, 400)]);
        let t2 = make_transcript("src-002", &[("climate", 0, 400)]);
        setup_project(tmp.path(), &manifest, &[t1, t2], &[], &[]);

        let results = search(tmp.path(), "climate", Some("src-002"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_id, "src-002");
    }

    // -- type filter ---------------------------------------------------------

    #[test]
    fn search_filters_by_type_transcript() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, true)]);
        let t = make_transcript("src-001", &[("office", 0, 400)]);
        let idx = make_index(
            "src-001",
            vec![make_scene(0, 0, 18000, Some("office wide shot"))],
        );
        setup_project(tmp.path(), &manifest, &[t], &[idx], &[]);

        let results = search(tmp.path(), "office", None, Some(&TypeFilter::Transcript)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Transcript);
    }

    #[test]
    fn search_filters_by_type_scene() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, true)]);
        let t = make_transcript("src-001", &[("office", 0, 400)]);
        let idx = make_index(
            "src-001",
            vec![make_scene(0, 0, 18000, Some("office wide shot"))],
        );
        setup_project(tmp.path(), &manifest, &[t], &[idx], &[]);

        let results = search(tmp.path(), "office", None, Some(&TypeFilter::Scene)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Scene);
    }

    #[test]
    fn search_filters_by_type_metadata() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, true)]);
        let t = make_transcript("src-001", &[("office", 0, 400)]);
        let idx = make_index(
            "src-001",
            vec![make_scene(0, 0, 18000, Some("office wide shot"))],
        );
        let markers = SourceMarkers {
            source_id: "src-001".into(),
            markers: vec![Marker {
                id: "mark-001".into(),
                range: ShotRange::Time {
                    from_ms: 0,
                    to_ms: 5000,
                },
                label: "office-shot".into(),
                note: None,
                created: "2026-02-19T14:00:00Z".parse().unwrap(),
            }],
        };
        setup_project(tmp.path(), &manifest, &[t], &[idx], &[markers]);

        let results = search(tmp.path(), "office", None, Some(&TypeFilter::Metadata)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, ResultType::Metadata);
    }

    // -- edge cases ----------------------------------------------------------

    #[test]
    fn search_no_matches() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, false)]);
        let t = make_transcript("src-001", &[("Hello", 0, 400), ("world", 400, 800)]);
        setup_project(tmp.path(), &manifest, &[t], &[], &[]);

        let results = search(tmp.path(), "nonexistent", None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", false, true)]);
        let idx = make_index(
            "src-001",
            vec![make_scene(0, 0, 18000, Some("Interior OFFICE"))],
        );
        setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

        let results = search(tmp.path(), "office", None, None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![]);
        setup_project(tmp.path(), &manifest, &[], &[], &[]);

        let err = search(tmp.path(), "[invalid", None, None).unwrap_err();
        assert!(format!("{err}").contains("regex"));
    }

    #[test]
    fn search_empty_project() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![]);
        setup_project(tmp.path(), &manifest, &[], &[], &[]);

        let results = search(tmp.path(), "anything", None, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_across_multiple_sources() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true, false),
            make_source("src-002", false, true),
        ]);
        let t = make_transcript("src-001", &[("interview", 0, 400)]);
        let idx = make_index(
            "src-002",
            vec![make_scene(0, 0, 18000, Some("Interview setup"))],
        );
        setup_project(tmp.path(), &manifest, &[t], &[idx], &[]);

        let results = search(tmp.path(), "interview", None, None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].source_id, "src-001");
        assert_eq!(results[0].result_type, ResultType::Transcript);
        assert_eq!(results[1].source_id, "src-002");
        assert_eq!(results[1].result_type, ResultType::Scene);
    }

    #[test]
    fn search_transcript_provides_context() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true, false)]);
        let t = make_transcript(
            "src-001",
            &[
                ("The", 0, 200),
                ("big", 200, 400),
                ("climate", 400, 800),
                ("policy", 800, 1200),
                ("debate", 1200, 1600),
            ],
        );
        setup_project(tmp.path(), &manifest, &[t], &[], &[]);

        let results = search(tmp.path(), "climate", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].context.contains("The big"));
        assert!(results[0].context.contains("policy debate"));
    }

    #[test]
    fn search_scenes_without_descriptions_skipped() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", false, true)]);
        let idx = make_index(
            "src-001",
            vec![
                make_scene(0, 0, 18000, None),
                make_scene(1, 18000, 45000, None),
            ],
        );
        setup_project(tmp.path(), &manifest, &[], &[idx], &[]);

        let results = search(tmp.path(), "anything", None, None).unwrap();
        assert!(results.is_empty());
    }
}
