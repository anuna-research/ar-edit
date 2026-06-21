use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use thiserror::Error;

use crate::fuzzy;
use crate::models::{EditDocument, Manifest, ShotRange, Transcript};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ImportError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Edit(#[from] crate::edit::EditError),
    #[error("parse errors in annotated transcript:\n{}", format_errors(.0))]
    ParseErrors(Vec<ParseError>),
}

/// A single parse error with its line number in the source file.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

fn format_errors(errors: &[ParseError]) -> String {
    errors
        .iter()
        .map(|e| format!("  line {}: {}", e.line, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Parsed annotation block
// ---------------------------------------------------------------------------

/// A single annotation block parsed from the markdown.
#[derive(Debug, Clone)]
struct AnnotationBlock {
    source_id: String,
    from_word: u32,
    to_word: u32,
    /// 1-based line number of the opening annotation comment.
    open_line: usize,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse an annotated markdown file and produce an `EditDocument`.
///
/// The file is expected to contain annotation blocks in the format produced
/// by `export_editable()`:
///
/// ```markdown
/// <!-- ar-edit:src-001:w0-w8 -->
/// Welcome to the interview today we're going to talk about
/// <!-- /ar-edit:src-001 -->
/// ```
///
/// Each surviving annotation block becomes one shot with `ShotRange::Words`.
/// Deleted blocks produce no shots. Reordered blocks produce shots in the
/// new order.
///
/// Returns an error if the file contains malformed annotations (with line
/// numbers for each problem).
pub fn from_transcript(path: &Path, name: Option<&str>) -> Result<EditDocument, ImportError> {
    let content = std::fs::read_to_string(path)?;
    let edit_name = name.map(String::from).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported")
            .to_string()
    });
    from_transcript_str(&content, &edit_name)
}

/// Parse annotated markdown from a string and produce an `EditDocument`.
///
/// This is the pure-logic core, separated from I/O for testability.
pub fn from_transcript_str(content: &str, name: &str) -> Result<EditDocument, ImportError> {
    let (blocks, errors) = parse_annotations(content);

    if !errors.is_empty() {
        return Err(ImportError::ParseErrors(errors));
    }

    let mut doc = EditDocument::create(name);

    for block in &blocks {
        doc.add_shot(
            &block.source_id,
            ShotRange::Words {
                from: block.from_word,
                to: block.to_word,
            },
        )?;
    }

    Ok(doc)
}

// ---------------------------------------------------------------------------
// Fuzzy-aware public API
// ---------------------------------------------------------------------------

/// An orphaned text block found outside any annotation comments.
#[derive(Debug, Clone)]
struct OrphanedBlock {
    source_id: String,
    text: String,
    /// 1-based line number of the first text line.
    first_line: usize,
}

/// A content block: either an annotated block (with word indices from the
/// HTML comments) or an orphaned text block that needs fuzzy matching.
#[derive(Debug, Clone)]
enum ContentBlock {
    Annotated(AnnotationBlock),
    Orphaned(OrphanedBlock),
}

/// Parse an annotated markdown file using fuzzy text matching for orphaned
/// text blocks (blocks where the user removed or never added annotations).
///
/// This extends [`from_transcript`] by also handling split/merged blocks:
///
/// - **Annotated blocks** — resolved using the word indices in the HTML
///   comments (same as `from_transcript`).
/// - **Orphaned text** — matched against the source's transcript via
///   [`crate::fuzzy::match_text`] to recover word boundaries.
///
/// Transcripts are loaded from `project_dir/transcripts/<source>.transcript.json`.
pub fn from_transcript_fuzzy(
    path: &Path,
    name: Option<&str>,
    project_dir: &Path,
) -> Result<EditDocument, ImportError> {
    let content = std::fs::read_to_string(path)?;
    let edit_name = name.map(String::from).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported")
            .to_string()
    });

    let transcripts = load_project_transcripts(project_dir)?;
    from_transcript_str_fuzzy(&content, &edit_name, &transcripts)
}

/// Parse annotated markdown from a string, using fuzzy text matching for
/// orphaned text blocks.
///
/// `transcripts` maps source IDs (e.g. `"src-001"`) to their transcript data.
/// Orphaned text is matched against the transcript for the enclosing source.
/// Unresolvable text produces errors with line numbers.
pub fn from_transcript_str_fuzzy(
    content: &str,
    name: &str,
    transcripts: &HashMap<String, Transcript>,
) -> Result<EditDocument, ImportError> {
    let (blocks, errors) = parse_content_blocks(content);

    if !errors.is_empty() {
        return Err(ImportError::ParseErrors(errors));
    }

    let mut doc = EditDocument::create(name);
    let mut match_errors = Vec::new();

    for block in &blocks {
        match block {
            ContentBlock::Annotated(ab) => {
                doc.add_shot(
                    &ab.source_id,
                    ShotRange::Words {
                        from: ab.from_word,
                        to: ab.to_word,
                    },
                )?;
            }
            ContentBlock::Orphaned(ob) => {
                if let Some(transcript) = transcripts.get(&ob.source_id) {
                    if let Some((from, to)) = fuzzy::match_text(&ob.text, transcript) {
                        doc.add_shot(&ob.source_id, ShotRange::Words { from, to })?;
                    } else {
                        match_errors.push(ParseError {
                            line: ob.first_line,
                            message: format!(
                                "could not match text against transcript for source '{}': \"{}\"",
                                ob.source_id,
                                truncate_text(&ob.text, 60),
                            ),
                        });
                    }
                } else {
                    match_errors.push(ParseError {
                        line: ob.first_line,
                        message: format!("no transcript available for source '{}'", ob.source_id,),
                    });
                }
            }
        }
    }

    if !match_errors.is_empty() {
        return Err(ImportError::ParseErrors(match_errors));
    }

    Ok(doc)
}

fn truncate_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

fn load_project_transcripts(
    project_dir: &Path,
) -> Result<HashMap<String, Transcript>, ImportError> {
    let manifest_path = project_dir.join("manifest.json");
    let manifest_data = std::fs::read_to_string(&manifest_path)?;
    let manifest: Manifest = serde_json::from_str(&manifest_data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let mut transcripts = HashMap::new();
    for source in &manifest.sources {
        if source.transcribed {
            let t_path = project_dir.join(format!("transcripts/{}.transcript.json", source.id));
            if let Ok(data) = std::fs::read_to_string(&t_path) {
                if let Ok(t) = serde_json::from_str::<Transcript>(&data) {
                    transcripts.insert(source.id.clone(), t);
                }
            }
        }
    }

    Ok(transcripts)
}

// ---------------------------------------------------------------------------
// Parser (strict, annotation-only)
// ---------------------------------------------------------------------------

/// Parse the annotated markdown content, extracting annotation blocks and
/// collecting any parse errors with line numbers.
fn parse_annotations(content: &str) -> (Vec<AnnotationBlock>, Vec<ParseError>) {
    let open_re = Regex::new(r"^<!--\s*ar-edit:([^:]+):w(\d+)-w(\d+)\s*-->$").unwrap();
    let close_re = Regex::new(r"^<!--\s*/ar-edit:([^>]+?)\s*-->$").unwrap();

    let mut blocks = Vec::new();
    let mut errors = Vec::new();

    // State: are we inside an open annotation block?
    let mut pending: Option<AnnotationBlock> = None;

    for (line_idx, line) in content.lines().enumerate() {
        let line_no = line_idx + 1; // 1-based
        let trimmed = line.trim();

        // Try to match an opening annotation
        if let Some(caps) = open_re.captures(trimmed) {
            // If we already have a pending block, it was never closed
            if let Some(prev) = pending.take() {
                errors.push(ParseError {
                    line: prev.open_line,
                    message: format!("unclosed annotation block for source '{}'", prev.source_id),
                });
            }

            let source_id = caps[1].to_string();
            let from_word: u32 = match caps[2].parse() {
                Ok(v) => v,
                Err(_) => {
                    errors.push(ParseError {
                        line: line_no,
                        message: format!("invalid from-word index: '{}'", &caps[2]),
                    });
                    continue;
                }
            };
            let to_word: u32 = match caps[3].parse() {
                Ok(v) => v,
                Err(_) => {
                    errors.push(ParseError {
                        line: line_no,
                        message: format!("invalid to-word index: '{}'", &caps[3]),
                    });
                    continue;
                }
            };

            if from_word > to_word {
                errors.push(ParseError {
                    line: line_no,
                    message: format!("from-word ({from_word}) is greater than to-word ({to_word})"),
                });
                continue;
            }

            pending = Some(AnnotationBlock {
                source_id,
                from_word,
                to_word,
                open_line: line_no,
            });
            continue;
        }

        // Try to match a closing annotation
        if let Some(caps) = close_re.captures(trimmed) {
            let close_source = &caps[1];

            match pending.take() {
                Some(block) if block.source_id == close_source => {
                    // Successfully matched open/close pair
                    blocks.push(block);
                }
                Some(block) => {
                    // Source ID mismatch between open and close
                    errors.push(ParseError {
                        line: line_no,
                        message: format!(
                            "closing annotation for '{}' does not match opening annotation for '{}' at line {}",
                            close_source, block.source_id, block.open_line
                        ),
                    });
                }
                None => {
                    // Closing annotation without a matching open
                    errors.push(ParseError {
                        line: line_no,
                        message: format!(
                            "closing annotation for '{close_source}' without matching opening"
                        ),
                    });
                }
            }
            continue;
        }
    }

    // Check for unclosed block at EOF
    if let Some(prev) = pending {
        errors.push(ParseError {
            line: prev.open_line,
            message: format!(
                "unclosed annotation block for source '{}' (reached end of file)",
                prev.source_id
            ),
        });
    }

    (blocks, errors)
}

// ---------------------------------------------------------------------------
// Parser (fuzzy-aware: annotations + orphaned text)
// ---------------------------------------------------------------------------

/// Parse annotated markdown, producing both annotated and orphaned content
/// blocks.  Orphaned text is any non-empty, non-structural line that appears
/// outside an annotation block.
///
/// Source context is tracked via `# Source: <id> — …` headers.  Orphaned text
/// inherits the most recent source header.
fn parse_content_blocks(content: &str) -> (Vec<ContentBlock>, Vec<ParseError>) {
    let open_re = Regex::new(r"^<!--\s*ar-edit:([^:]+):w(\d+)-w(\d+)\s*-->$").unwrap();
    let close_re = Regex::new(r"^<!--\s*/ar-edit:([^>]+?)\s*-->$").unwrap();
    let source_re = Regex::new(r"^#\s+Source:\s+(\S+)\s+—").unwrap();

    let mut blocks: Vec<ContentBlock> = Vec::new();
    let mut errors = Vec::new();

    let mut current_source: Option<String> = None;
    let mut pending: Option<AnnotationBlock> = None;

    // Accumulator for consecutive orphaned text lines.
    let mut orphan_lines: Vec<String> = Vec::new();
    let mut orphan_first_line: usize = 0;

    // Flush accumulated orphaned text into a ContentBlock.
    let flush_orphaned = |lines: &mut Vec<String>,
                          first_line: usize,
                          source: &Option<String>,
                          blocks: &mut Vec<ContentBlock>,
                          errors: &mut Vec<ParseError>| {
        if lines.is_empty() {
            return;
        }
        let text = lines.join(" ");
        lines.clear();

        if let Some(src) = source {
            blocks.push(ContentBlock::Orphaned(OrphanedBlock {
                source_id: src.clone(),
                text,
                first_line,
            }));
        } else {
            errors.push(ParseError {
                line: first_line,
                message: "text outside any source section cannot be matched".into(),
            });
        }
    };

    for (line_idx, line) in content.lines().enumerate() {
        let line_no = line_idx + 1;
        let trimmed = line.trim();

        // Source header — update context, flush orphaned text.
        if let Some(caps) = source_re.captures(trimmed) {
            flush_orphaned(
                &mut orphan_lines,
                orphan_first_line,
                &current_source,
                &mut blocks,
                &mut errors,
            );
            current_source = Some(caps[1].to_string());
            continue;
        }

        // HR separator.
        if trimmed == "---" {
            flush_orphaned(
                &mut orphan_lines,
                orphan_first_line,
                &current_source,
                &mut blocks,
                &mut errors,
            );
            continue;
        }

        // Opening annotation.
        if let Some(caps) = open_re.captures(trimmed) {
            flush_orphaned(
                &mut orphan_lines,
                orphan_first_line,
                &current_source,
                &mut blocks,
                &mut errors,
            );

            if let Some(prev) = pending.take() {
                errors.push(ParseError {
                    line: prev.open_line,
                    message: format!("unclosed annotation block for source '{}'", prev.source_id),
                });
            }

            let source_id = caps[1].to_string();
            let from_word: u32 = match caps[2].parse() {
                Ok(v) => v,
                Err(_) => {
                    errors.push(ParseError {
                        line: line_no,
                        message: format!("invalid from-word index: '{}'", &caps[2]),
                    });
                    continue;
                }
            };
            let to_word: u32 = match caps[3].parse() {
                Ok(v) => v,
                Err(_) => {
                    errors.push(ParseError {
                        line: line_no,
                        message: format!("invalid to-word index: '{}'", &caps[3]),
                    });
                    continue;
                }
            };

            if from_word > to_word {
                errors.push(ParseError {
                    line: line_no,
                    message: format!("from-word ({from_word}) is greater than to-word ({to_word})"),
                });
                continue;
            }

            pending = Some(AnnotationBlock {
                source_id,
                from_word,
                to_word,
                open_line: line_no,
            });
            continue;
        }

        // Closing annotation.
        if let Some(caps) = close_re.captures(trimmed) {
            let close_source = &caps[1];

            match pending.take() {
                Some(block) if block.source_id == close_source => {
                    blocks.push(ContentBlock::Annotated(block));
                }
                Some(block) => {
                    errors.push(ParseError {
                        line: line_no,
                        message: format!(
                            "closing annotation for '{}' does not match opening annotation for '{}' at line {}",
                            close_source, block.source_id, block.open_line
                        ),
                    });
                }
                None => {
                    errors.push(ParseError {
                        line: line_no,
                        message: format!(
                            "closing annotation for '{close_source}' without matching opening"
                        ),
                    });
                }
            }
            continue;
        }

        // Inside an annotation block — skip content lines.
        if pending.is_some() {
            continue;
        }

        // Orphaned text: non-empty lines outside annotations.
        if !trimmed.is_empty() {
            if orphan_lines.is_empty() {
                orphan_first_line = line_no;
            }
            orphan_lines.push(trimmed.to_string());
        } else {
            // Empty line: flush any accumulated orphaned text.
            flush_orphaned(
                &mut orphan_lines,
                orphan_first_line,
                &current_source,
                &mut blocks,
                &mut errors,
            );
        }
    }

    // Flush any remaining orphaned text.
    flush_orphaned(
        &mut orphan_lines,
        orphan_first_line,
        &current_source,
        &mut blocks,
        &mut errors,
    );

    // Check for unclosed annotation at EOF.
    if let Some(prev) = pending {
        errors.push(ParseError {
            line: prev.open_line,
            message: format!(
                "unclosed annotation block for source '{}' (reached end of file)",
                prev.source_id
            ),
        });
    }

    (blocks, errors)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ShotRange;

    // -- parse_annotations (unit) -------------------------------------------

    #[test]
    fn parse_single_block() {
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->
";
        let (blocks, errors) = parse_annotations(input);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source_id, "src-001");
        assert_eq!(blocks[0].from_word, 0);
        assert_eq!(blocks[0].to_word, 3);
    }

    #[test]
    fn parse_multiple_blocks() {
        let input = "\
# Source: src-001 — interview-alice.mp4

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w4-w6 -->
today we talk
<!-- /ar-edit:src-001 -->
";
        let (blocks, errors) = parse_annotations(input);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].from_word, 0);
        assert_eq!(blocks[0].to_word, 3);
        assert_eq!(blocks[1].from_word, 4);
        assert_eq!(blocks[1].to_word, 6);
    }

    #[test]
    fn parse_multiple_sources() {
        let input = "\
# Source: src-001 — test.mp4

<!-- ar-edit:src-001:w0-w1 -->
Hello world
<!-- /ar-edit:src-001 -->

---

# Source: src-002 — test2.mp4

<!-- ar-edit:src-002:w0-w1 -->
So the
<!-- /ar-edit:src-002 -->
";
        let (blocks, errors) = parse_annotations(input);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].source_id, "src-001");
        assert_eq!(blocks[1].source_id, "src-002");
    }

    #[test]
    fn parse_deleted_block_produces_fewer_results() {
        // User deleted the second block
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

";
        let (blocks, errors) = parse_annotations(input);
        assert!(errors.is_empty());
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn parse_reordered_blocks() {
        // User swapped the order of two blocks
        let input = "\
<!-- ar-edit:src-001:w4-w6 -->
today we talk
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->
";
        let (blocks, errors) = parse_annotations(input);
        assert!(errors.is_empty());
        assert_eq!(blocks.len(), 2);
        // First block in file should be first in output
        assert_eq!(blocks[0].from_word, 4);
        assert_eq!(blocks[1].from_word, 0);
    }

    #[test]
    fn parse_unclosed_block_error() {
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
";
        let (blocks, errors) = parse_annotations(input);
        assert!(blocks.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("unclosed"));
        assert_eq!(errors[0].line, 1);
    }

    #[test]
    fn parse_close_without_open_error() {
        let input = "\
Some text
<!-- /ar-edit:src-001 -->
";
        let (_, errors) = parse_annotations(input);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("without matching opening"));
        assert_eq!(errors[0].line, 2);
    }

    #[test]
    fn parse_mismatched_source_error() {
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-002 -->
";
        let (_, errors) = parse_annotations(input);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("does not match"));
    }

    #[test]
    fn parse_from_word_greater_than_to_word_error() {
        let input = "\
<!-- ar-edit:src-001:w5-w3 -->
Some text
<!-- /ar-edit:src-001 -->
";
        let (blocks, errors) = parse_annotations(input);
        assert!(blocks.is_empty());
        // Two errors: invalid range + orphaned close
        assert_eq!(errors.len(), 2);
        assert!(errors[0].message.contains("greater than"));
        assert!(errors[1].message.contains("without matching opening"));
    }

    #[test]
    fn parse_empty_input() {
        let (blocks, errors) = parse_annotations("");
        assert!(blocks.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_no_annotations() {
        let input = "# Just a regular markdown document\n\nWith some text.\n";
        let (blocks, errors) = parse_annotations(input);
        assert!(blocks.is_empty());
        assert!(errors.is_empty());
    }

    // -- from_transcript_str (integration) ----------------------------------

    #[test]
    fn from_transcript_single_source() {
        let input = "\
# Source: src-001 — interview-alice.mp4

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w4-w6 -->
today we talk
<!-- /ar-edit:src-001 -->
";
        let doc = from_transcript_str(input, "test-edit").unwrap();
        assert_eq!(doc.name, "test-edit");
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(doc.snapshot.shots[0].source, "src-001");
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 3 }
        );
        assert_eq!(
            doc.snapshot.shots[1].range,
            ShotRange::Words { from: 4, to: 6 }
        );
    }

    #[test]
    fn from_transcript_multiple_sources() {
        let input = "\
# Source: src-001 — interview-alice.mp4

<!-- ar-edit:src-001:w0-w1 -->
Hello world
<!-- /ar-edit:src-001 -->

---

# Source: src-002 — interview-bob.mp4

<!-- ar-edit:src-002:w0-w1 -->
So the
<!-- /ar-edit:src-002 -->
";
        let doc = from_transcript_str(input, "multi").unwrap();
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(doc.snapshot.shots[0].source, "src-001");
        assert_eq!(doc.snapshot.shots[1].source, "src-002");
    }

    #[test]
    fn from_transcript_deleted_blocks() {
        // Original had 3 blocks, user deleted the middle one
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w9-w16 -->
the impact of climate policy on regional communities
<!-- /ar-edit:src-001 -->
";
        let doc = from_transcript_str(input, "trimmed").unwrap();
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 3 }
        );
        assert_eq!(
            doc.snapshot.shots[1].range,
            ShotRange::Words { from: 9, to: 16 }
        );
    }

    #[test]
    fn from_transcript_reordered_blocks() {
        let input = "\
<!-- ar-edit:src-001:w4-w6 -->
today we talk
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->
";
        let doc = from_transcript_str(input, "reordered").unwrap();
        assert_eq!(doc.snapshot.shots.len(), 2);
        // Shots in file order: w4-w6 comes first
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 4, to: 6 }
        );
        assert_eq!(
            doc.snapshot.shots[1].range,
            ShotRange::Words { from: 0, to: 3 }
        );
    }

    #[test]
    fn from_transcript_parse_error() {
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
Welcome
";
        let err = from_transcript_str(input, "broken").unwrap_err();
        assert!(matches!(err, ImportError::ParseErrors(_)));
        if let ImportError::ParseErrors(errors) = err {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].line, 1);
        }
    }

    #[test]
    fn from_transcript_empty_produces_empty_edit() {
        let doc = from_transcript_str("", "empty").unwrap();
        assert_eq!(doc.snapshot.shots.len(), 0);
        assert_eq!(doc.name, "empty");
    }

    #[test]
    fn from_transcript_shot_ids_are_sequential() {
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
first
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-002:w0-w5 -->
second
<!-- /ar-edit:src-002 -->

<!-- ar-edit:src-001:w10-w15 -->
third
<!-- /ar-edit:src-001 -->
";
        let doc = from_transcript_str(input, "ids").unwrap();
        assert_eq!(doc.snapshot.shots[0].id, "shot-001");
        assert_eq!(doc.snapshot.shots[1].id, "shot-002");
        assert_eq!(doc.snapshot.shots[2].id, "shot-003");
    }

    #[test]
    fn from_transcript_ops_match_shots() {
        let input = "\
<!-- ar-edit:src-001:w0-w3 -->
text
<!-- /ar-edit:src-001 -->
";
        let doc = from_transcript_str(input, "ops").unwrap();
        assert_eq!(doc.ops.len(), 1);
        assert_eq!(doc.head, 0);
    }

    // -- parse_content_blocks (unit) ------------------------------------------

    #[test]
    fn content_blocks_annotated_only() {
        let input = "\
# Source: src-001 — test.mp4

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->
";
        let (blocks, errors) = parse_content_blocks(input);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], ContentBlock::Annotated(a) if a.from_word == 0 && a.to_word == 3)
        );
    }

    #[test]
    fn content_blocks_orphaned_text() {
        let input = "\
# Source: src-001 — test.mp4

Welcome to the interview
";
        let (blocks, errors) = parse_content_blocks(input);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], ContentBlock::Orphaned(o) if o.text == "Welcome to the interview" && o.source_id == "src-001")
        );
    }

    #[test]
    fn content_blocks_mixed_annotated_and_orphaned() {
        let input = "\
# Source: src-001 — test.mp4

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

today we talk
";
        let (blocks, errors) = parse_content_blocks(input);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Annotated(a) if a.from_word == 0));
        assert!(matches!(&blocks[1], ContentBlock::Orphaned(o) if o.text == "today we talk"));
    }

    #[test]
    fn content_blocks_orphan_tracks_first_line() {
        // Line 1: source header
        // Line 2: empty
        // Line 3: orphaned text
        let input = "\
# Source: src-001 — test.mp4

Welcome to the interview
";
        let (blocks, _) = parse_content_blocks(input);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Orphaned(o) = &blocks[0] {
            assert_eq!(o.first_line, 3);
        } else {
            panic!("expected orphaned block");
        }
    }

    #[test]
    fn content_blocks_multi_line_orphan() {
        let input = "\
# Source: src-001 — test.mp4

Welcome to the interview
today we're going to talk about
";
        let (blocks, errors) = parse_content_blocks(input);
        assert!(errors.is_empty());
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Orphaned(o) = &blocks[0] {
            assert_eq!(
                o.text,
                "Welcome to the interview today we're going to talk about"
            );
        } else {
            panic!("expected orphaned block");
        }
    }

    #[test]
    fn content_blocks_separate_orphan_paragraphs() {
        let input = "\
# Source: src-001 — test.mp4

Welcome to the interview

today we talk
";
        let (blocks, errors) = parse_content_blocks(input);
        assert!(errors.is_empty());
        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[0], ContentBlock::Orphaned(o) if o.text == "Welcome to the interview")
        );
        assert!(matches!(&blocks[1], ContentBlock::Orphaned(o) if o.text == "today we talk"));
    }

    #[test]
    fn content_blocks_multiple_sources() {
        let input = "\
# Source: src-001 — test.mp4

some text here

---

# Source: src-002 — test2.mp4

other text here
";
        let (blocks, errors) = parse_content_blocks(input);
        assert!(errors.is_empty());
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Orphaned(o) if o.source_id == "src-001"));
        assert!(matches!(&blocks[1], ContentBlock::Orphaned(o) if o.source_id == "src-002"));
    }

    #[test]
    fn content_blocks_orphan_without_source_errors() {
        let input = "some orphaned text without any source header\n";
        let (_, errors) = parse_content_blocks(input);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("cannot be matched"));
    }

    // -- from_transcript_str_fuzzy (integration) ------------------------------

    fn make_transcript(source_id: &str, words: &[&str]) -> Transcript {
        use crate::models::{TranscriptSegment, Word};
        let words_vec: Vec<Word> = words
            .iter()
            .enumerate()
            .map(|(i, text)| Word {
                index: i as u32,
                text: text.to_string(),
                start_ms: (i as u64) * 500,
                end_ms: (i as u64) * 500 + 400,
                confidence: 0.95,
            })
            .collect();
        let text = words.join(" ");
        Transcript {
            source_id: source_id.into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms: words_vec.last().map_or(0, |w| w.end_ms),
            word_count: words_vec.len() as u32,
            segments: vec![TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: words_vec.last().map_or(0, |w| w.end_ms),
                text,
                words: words_vec,
            }],
        }
    }

    fn make_transcripts(ts: Vec<Transcript>) -> HashMap<String, Transcript> {
        ts.into_iter().map(|t| (t.source_id.clone(), t)).collect()
    }

    #[test]
    fn fuzzy_split_block_resolves_both_halves() {
        // User split one segment block into two: annotated + orphaned.
        let input = "\
# Source: src-001 — test.mp4

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

today we talk about
";
        let t = make_transcript(
            "src-001",
            &[
                "Welcome",
                "to",
                "the",
                "interview",
                "today",
                "we",
                "talk",
                "about",
            ],
        );
        let transcripts = make_transcripts(vec![t]);

        let doc = from_transcript_str_fuzzy(input, "split-test", &transcripts).unwrap();
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 3 }
        );
        assert_eq!(
            doc.snapshot.shots[1].range,
            ShotRange::Words { from: 4, to: 7 }
        );
    }

    #[test]
    fn fuzzy_merged_blocks_resolve_to_combined_range() {
        // User removed all annotations — entire text is orphaned.
        let input = "\
# Source: src-001 — test.mp4

Welcome to the interview today we talk about
";
        let t = make_transcript(
            "src-001",
            &[
                "Welcome",
                "to",
                "the",
                "interview",
                "today",
                "we",
                "talk",
                "about",
            ],
        );
        let transcripts = make_transcripts(vec![t]);

        let doc = from_transcript_str_fuzzy(input, "merged-test", &transcripts).unwrap();
        assert_eq!(doc.snapshot.shots.len(), 1);
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 7 }
        );
    }

    #[test]
    fn fuzzy_annotated_blocks_pass_through_unchanged() {
        // All blocks still have annotations — fuzzy should work identically
        // to the strict parser.
        let input = "\
# Source: src-001 — test.mp4

<!-- ar-edit:src-001:w0-w3 -->
Welcome to the interview
<!-- /ar-edit:src-001 -->

<!-- ar-edit:src-001:w4-w7 -->
today we talk about
<!-- /ar-edit:src-001 -->
";
        let t = make_transcript(
            "src-001",
            &[
                "Welcome",
                "to",
                "the",
                "interview",
                "today",
                "we",
                "talk",
                "about",
            ],
        );
        let transcripts = make_transcripts(vec![t]);

        let doc = from_transcript_str_fuzzy(input, "annotated", &transcripts).unwrap();
        assert_eq!(doc.snapshot.shots.len(), 2);
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 3 }
        );
        assert_eq!(
            doc.snapshot.shots[1].range,
            ShotRange::Words { from: 4, to: 7 }
        );
    }

    #[test]
    fn fuzzy_unresolvable_text_produces_error_with_line() {
        let input = "\
# Source: src-001 — test.mp4

This text does not exist in the transcript
";
        let t = make_transcript("src-001", &["Welcome", "to", "the", "interview"]);
        let transcripts = make_transcripts(vec![t]);

        let err = from_transcript_str_fuzzy(input, "bad", &transcripts).unwrap_err();
        if let ImportError::ParseErrors(errors) = err {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].line, 3);
            assert!(errors[0].message.contains("could not match"));
        } else {
            panic!("expected ParseErrors");
        }
    }

    #[test]
    fn fuzzy_missing_transcript_produces_error() {
        let input = "\
# Source: src-999 — missing.mp4

some orphaned text
";
        let transcripts: HashMap<String, Transcript> = HashMap::new();

        let err = from_transcript_str_fuzzy(input, "no-transcript", &transcripts).unwrap_err();
        if let ImportError::ParseErrors(errors) = err {
            assert_eq!(errors.len(), 1);
            assert!(errors[0].message.contains("no transcript"));
        } else {
            panic!("expected ParseErrors");
        }
    }

    #[test]
    fn fuzzy_multiple_sources_split() {
        let input = "\
# Source: src-001 — test.mp4

Welcome to the interview

---

# Source: src-002 — test2.mp4

<!-- ar-edit:src-002:w0-w1 -->
So the
<!-- /ar-edit:src-002 -->

thing is
";
        let t1 = make_transcript("src-001", &["Welcome", "to", "the", "interview"]);
        let t2 = make_transcript("src-002", &["So", "the", "thing", "is"]);
        let transcripts = make_transcripts(vec![t1, t2]);

        let doc = from_transcript_str_fuzzy(input, "multi", &transcripts).unwrap();
        assert_eq!(doc.snapshot.shots.len(), 3);
        // src-001 orphaned → w0-w3
        assert_eq!(doc.snapshot.shots[0].source, "src-001");
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 3 }
        );
        // src-002 annotated → w0-w1
        assert_eq!(doc.snapshot.shots[1].source, "src-002");
        assert_eq!(
            doc.snapshot.shots[1].range,
            ShotRange::Words { from: 0, to: 1 }
        );
        // src-002 orphaned → w2-w3
        assert_eq!(doc.snapshot.shots[2].source, "src-002");
        assert_eq!(
            doc.snapshot.shots[2].range,
            ShotRange::Words { from: 2, to: 3 }
        );
    }

    #[test]
    fn fuzzy_empty_input() {
        let transcripts: HashMap<String, Transcript> = HashMap::new();
        let doc = from_transcript_str_fuzzy("", "empty", &transcripts).unwrap();
        assert_eq!(doc.snapshot.shots.len(), 0);
    }

    #[test]
    fn fuzzy_with_typo_in_orphaned_text() {
        let input = "\
# Source: src-001 — test.mp4

Welcome to the intervew today
";
        let t = make_transcript(
            "src-001",
            &["Welcome", "to", "the", "interview", "today", "we", "talk"],
        );
        let transcripts = make_transcripts(vec![t]);

        let doc = from_transcript_str_fuzzy(input, "typo", &transcripts).unwrap();
        assert_eq!(doc.snapshot.shots.len(), 1);
        assert_eq!(
            doc.snapshot.shots[0].range,
            ShotRange::Words { from: 0, to: 4 }
        );
    }
}
