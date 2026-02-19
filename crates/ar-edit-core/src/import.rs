use std::path::Path;

use regex::Regex;
use thiserror::Error;

use crate::models::{EditDocument, ShotRange};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ImportError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
        );
    }

    Ok(doc)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse the annotated markdown content, extracting annotation blocks and
/// collecting any parse errors with line numbers.
fn parse_annotations(content: &str) -> (Vec<AnnotationBlock>, Vec<ParseError>) {
    let open_re =
        Regex::new(r"^<!--\s*ar-edit:([^:]+):w(\d+)-w(\d+)\s*-->$").unwrap();
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
                    message: format!(
                        "unclosed annotation block for source '{}'",
                        prev.source_id
                    ),
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
                    message: format!(
                        "from-word ({from_word}) is greater than to-word ({to_word})"
                    ),
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
                            "closing annotation for '{}' without matching opening",
                            close_source
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
}
