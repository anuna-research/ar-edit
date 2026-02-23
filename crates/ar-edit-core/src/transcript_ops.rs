use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{Manifest, Transcript};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum TranscriptOpsError {
    #[error("source not found: {0}")]
    SourceNotFound(String),
    #[error("source '{0}' has no transcript")]
    NoTranscript(String),
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

/// Summary of a transcript for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptInfo {
    pub source_id: String,
    pub duration_ms: u64,
    pub word_count: u32,
    pub path: String,
}

/// A search match within a transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub source_id: String,
    pub segment_index: u32,
    pub from_word: u32,
    pub to_word: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub context_before: String,
    pub context_after: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// List all available transcripts in the project.
///
/// Reads the manifest to find sources with `transcribed: true`, then loads
/// each transcript file to extract summary metadata.
pub fn list(project_dir: &Path) -> Result<Vec<TranscriptInfo>, TranscriptOpsError> {
    let manifest = load_manifest(project_dir)?;
    let mut results = Vec::new();

    for source in &manifest.sources {
        if !source.transcribed {
            continue;
        }

        let rel_path = format!("transcripts/{}.transcript.json", source.id);
        let abs_path = project_dir.join(&rel_path);

        match load_transcript(&abs_path) {
            Ok(transcript) => {
                results.push(TranscriptInfo {
                    source_id: transcript.source_id,
                    duration_ms: transcript.duration_ms,
                    word_count: transcript.word_count,
                    path: rel_path,
                });
            }
            Err(_) => {
                // Source claims transcribed but file is missing/corrupt — skip
                continue;
            }
        }
    }

    Ok(results)
}

/// Read a transcript for the given source.
///
/// Returns the full [`Transcript`] struct, suitable for both text rendering
/// and JSON output.
pub fn read(project_dir: &Path, source_id: &str) -> Result<Transcript, TranscriptOpsError> {
    let manifest = load_manifest(project_dir)?;

    // Verify source exists
    let source = manifest
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| TranscriptOpsError::SourceNotFound(source_id.to_string()))?;

    if !source.transcribed {
        return Err(TranscriptOpsError::NoTranscript(source_id.to_string()));
    }

    let path = project_dir.join(format!("transcripts/{source_id}.transcript.json"));
    load_transcript(&path)
}

/// Search transcripts for matches against a regex query.
///
/// Searches across all transcripts (or a specific source if `source_filter`
/// is provided). For each match, returns the word indices, timestamps, matched
/// text and surrounding context.
pub fn search(
    project_dir: &Path,
    query: &str,
    source_filter: Option<&str>,
) -> Result<Vec<SearchResult>, TranscriptOpsError> {
    let re = Regex::new(&format!("(?i){query}")).map_err(|e| TranscriptOpsError::InvalidRegex {
        pattern: query.to_string(),
        source: e,
    })?;

    let manifest = load_manifest(project_dir)?;
    let mut results = Vec::new();

    for source in &manifest.sources {
        if !source.transcribed {
            continue;
        }

        if let Some(filter) = source_filter {
            if source.id != filter {
                continue;
            }
        }

        let path = project_dir.join(format!("transcripts/{}.transcript.json", source.id));
        let transcript = match load_transcript(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };

        search_transcript(&transcript, &re, &mut results);
    }

    Ok(results)
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

/// Number of context words to include before/after a match.
const CONTEXT_WORDS: usize = 5;

/// Search within a single transcript and append results.
fn search_transcript(transcript: &Transcript, re: &Regex, results: &mut Vec<SearchResult>) {
    // Collect all words with their metadata for efficient lookups.
    struct WordEntry {
        text: String,
        index: u32,
        start_ms: u64,
        end_ms: u64,
        segment_index: u32,
    }

    let words: Vec<WordEntry> = transcript
        .segments
        .iter()
        .flat_map(|seg| {
            seg.words.iter().map(move |w| WordEntry {
                text: w.text.clone(),
                index: w.index,
                start_ms: w.start_ms,
                end_ms: w.end_ms,
                segment_index: seg.index,
            })
        })
        .collect();

    if words.is_empty() {
        return;
    }

    // Build a full text string with word boundaries tracked.
    // Each word occupies a known character range in the concatenated text.
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

    // Find all regex matches in the concatenated text.
    for m in re.find_iter(&full_text) {
        let match_start = m.start();
        let match_end = m.end();

        // Map character positions to word indices.
        let from_word_pos = match word_char_starts.binary_search(&match_start) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };

        let to_word_pos = match word_char_ends.binary_search(&match_end) {
            Ok(i) => i,
            Err(i) => i.min(words.len() - 1),
        };

        let from_word = words[from_word_pos].index;
        let to_word = words[to_word_pos].index;
        let start_ms = words[from_word_pos].start_ms;
        let end_ms = words[to_word_pos].end_ms;
        let segment_index = words[from_word_pos].segment_index;

        // Build context
        let context_before = build_context(
            &words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            from_word_pos,
            CONTEXT_WORDS,
            true,
        );

        let context_after = build_context(
            &words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            to_word_pos,
            CONTEXT_WORDS,
            false,
        );

        // Build matched text with context indicator
        let matched_words: Vec<&str> = words[from_word_pos..=to_word_pos]
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        let text = matched_words.join(" ");

        results.push(SearchResult {
            source_id: transcript.source_id.clone(),
            segment_index,
            from_word,
            to_word,
            start_ms,
            end_ms,
            text,
            context_before,
            context_after,
        });
    }
}

/// Build context string from surrounding words.
///
/// If `before` is true, collects up to `count` words before `word_pos`.
/// If `before` is false, collects up to `count` words after `word_pos`.
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

    fn make_transcript(source_id: &str, words_data: &[(&str, u64, u64)]) -> Transcript {
        let mut segments = Vec::new();
        let mut global_idx: u32 = 0;
        let mut duration_ms: u64 = 0;

        // Put all words in a single segment for simplicity
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

        segments.push(TranscriptSegment {
            index: 0,
            start_ms: words_data.first().map_or(0, |w| w.1),
            end_ms: words_data.last().map_or(0, |w| w.2),
            text,
            words,
        });

        Transcript {
            source_id: source_id.into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms,
            word_count: global_idx,
            segments,
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

    // -- list ----------------------------------------------------------------

    #[test]
    fn list_returns_transcribed_sources() {
        let tmp = TempDir::new().unwrap();

        let manifest = make_manifest(vec![
            make_source("src-001", true),
            make_source("src-002", false),
            make_source("src-003", true),
        ]);

        let t1 = make_transcript("src-001", &[("Hello", 0, 500), ("world", 500, 1000)]);
        let t3 = make_transcript(
            "src-003",
            &[
                ("Climate", 0, 400),
                ("policy", 400, 800),
                ("matters", 800, 1200),
            ],
        );

        setup_project(tmp.path(), &manifest, &[t1, t3]);

        let result = list(tmp.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_id, "src-001");
        assert_eq!(result[0].word_count, 2);
        assert_eq!(result[1].source_id, "src-003");
        assert_eq!(result[1].word_count, 3);
    }

    #[test]
    fn list_empty_project() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![]);
        setup_project(tmp.path(), &manifest, &[]);

        let result = list(tmp.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_skips_missing_transcript_files() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true), // transcribed=true but no file
        ]);
        setup_project(tmp.path(), &manifest, &[]);

        let result = list(tmp.path()).unwrap();
        assert!(result.is_empty());
    }

    // -- read ----------------------------------------------------------------

    #[test]
    fn read_returns_transcript() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        let t = make_transcript("src-001", &[("Hello", 0, 500), ("world", 500, 1000)]);
        setup_project(tmp.path(), &manifest, &[t.clone()]);

        let result = read(tmp.path(), "src-001").unwrap();
        assert_eq!(result.source_id, "src-001");
        assert_eq!(result.word_count, 2);
        assert_eq!(result, t);
    }

    #[test]
    fn read_rejects_unknown_source() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        setup_project(tmp.path(), &manifest, &[]);

        let err = read(tmp.path(), "src-999").unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[test]
    fn read_rejects_untranscribed_source() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", false)]);
        setup_project(tmp.path(), &manifest, &[]);

        let err = read(tmp.path(), "src-001").unwrap_err();
        assert!(format!("{err}").contains("no transcript"));
    }

    // -- search --------------------------------------------------------------

    #[test]
    fn search_finds_matching_words() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
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
        setup_project(tmp.path(), &manifest, &[t]);

        let results = search(tmp.path(), "climate", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_id, "src-001");
        assert_eq!(results[0].from_word, 1);
        assert_eq!(results[0].to_word, 1);
        assert_eq!(results[0].start_ms, 200);
        assert_eq!(results[0].end_ms, 600);
        assert_eq!(results[0].text, "climate");
    }

    #[test]
    fn search_across_multiple_sources() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true),
            make_source("src-002", true),
            make_source("src-003", true),
        ]);
        let t1 = make_transcript(
            "src-001",
            &[
                ("The", 0, 200),
                ("climate", 200, 600),
                ("policy", 600, 1000),
            ],
        );
        let t2 = make_transcript(
            "src-002",
            &[("No", 0, 200), ("match", 200, 500), ("here", 500, 800)],
        );
        let t3 = make_transcript(
            "src-003",
            &[
                ("Our", 0, 200),
                ("climate", 200, 600),
                ("future", 600, 1000),
            ],
        );
        setup_project(tmp.path(), &manifest, &[t1, t2, t3]);

        let results = search(tmp.path(), "climate", None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].source_id, "src-001");
        assert_eq!(results[1].source_id, "src-003");
    }

    #[test]
    fn search_with_source_filter() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![
            make_source("src-001", true),
            make_source("src-003", true),
        ]);
        let t1 = make_transcript("src-001", &[("climate", 0, 400)]);
        let t3 = make_transcript("src-003", &[("climate", 0, 400)]);
        setup_project(tmp.path(), &manifest, &[t1, t3]);

        let results = search(tmp.path(), "climate", Some("src-003")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_id, "src-003");
    }

    #[test]
    fn search_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        let t = make_transcript("src-001", &[("Climate", 0, 400), ("POLICY", 400, 800)]);
        setup_project(tmp.path(), &manifest, &[t]);

        let results = search(tmp.path(), "climate", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "Climate");
    }

    #[test]
    fn search_regex_pattern() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        let t = make_transcript(
            "src-001",
            &[
                ("The", 0, 200),
                ("climate", 200, 600),
                ("climates", 600, 1000),
                ("warming", 1000, 1400),
            ],
        );
        setup_project(tmp.path(), &manifest, &[t]);

        // Regex: word boundary match
        let results = search(tmp.path(), "climate\\b", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].from_word, 1);
    }

    #[test]
    fn search_no_matches() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        let t = make_transcript("src-001", &[("Hello", 0, 400), ("world", 400, 800)]);
        setup_project(tmp.path(), &manifest, &[t]);

        let results = search(tmp.path(), "nonexistent", None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        setup_project(tmp.path(), &manifest, &[]);

        let err = search(tmp.path(), "[invalid", None).unwrap_err();
        assert!(format!("{err}").contains("regex"));
    }

    #[test]
    fn search_provides_context() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
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
        setup_project(tmp.path(), &manifest, &[t]);

        let results = search(tmp.path(), "climate", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].context_before, "The big");
        assert_eq!(results[0].context_after, "policy debate");
    }

    #[test]
    fn search_multi_word_match() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_manifest(vec![make_source("src-001", true)]);
        let t = make_transcript(
            "src-001",
            &[
                ("climate", 0, 400),
                ("policy", 400, 800),
                ("debate", 800, 1200),
            ],
        );
        setup_project(tmp.path(), &manifest, &[t]);

        let results = search(tmp.path(), "climate policy", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].from_word, 0);
        assert_eq!(results[0].to_word, 1);
        assert_eq!(results[0].start_ms, 0);
        assert_eq!(results[0].end_ms, 800);
        assert_eq!(results[0].text, "climate policy");
    }
}
