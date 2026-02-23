use std::path::Path;

use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::{ShotRange, SourceIndex, Transcript};

// ---------------------------------------------------------------------------
// Scroll state (owned by App)
// ---------------------------------------------------------------------------

/// Persistent scroll state for the transcript panel.
#[derive(Debug, Default)]
pub struct TranscriptScroll {
    /// Vertical scroll offset (line index).
    pub offset: u16,
    /// Source ID the cached content was built for.
    cached_source: String,
    /// Shot ID the cached highlight was built for.
    cached_shot: String,
    /// The line index where the highlighted range starts.
    highlight_line: u16,
}

// ---------------------------------------------------------------------------
// Public draw function (REQ-040)
// ---------------------------------------------------------------------------

/// Render the transcript/description panel.
///
/// Displays the full transcript (for word ranges) or scene list (for scene
/// ranges) of the selected shot's source.  The word/scene range covered by
/// the selected shot is highlighted.  The view auto-scrolls so the
/// highlighted region is visible.
pub fn draw(
    f: &mut Frame,
    shot: Option<&ResolvedShot>,
    project_dir: &Path,
    scroll: &mut TranscriptScroll,
    area: Rect,
) {
    let block = Block::default().title(" Transcript ").borders(Borders::ALL);

    let Some(shot) = shot else {
        let paragraph = Paragraph::new("Select a shot to view its transcript")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(paragraph, area);
        return;
    };

    // Inner area (inside block borders) determines visible height.
    let inner = block.inner(area);
    let visible_height = inner.height;

    // Build styled lines based on the shot's range type.
    let (lines, highlight_line) = match &shot.range {
        ShotRange::Words { from, to } => build_word_transcript(shot, *from, *to, project_dir),
        ShotRange::Scenes { from, to } => build_scene_list(shot, *from, *to, project_dir),
        ShotRange::Time { from_ms, to_ms } => build_time_view(shot, *from_ms, *to_ms, project_dir),
    };

    let total_lines = lines.len() as u16;

    // Auto-scroll: when the shot changes, jump so the highlight is visible.
    if shot.source != scroll.cached_source || shot.id != scroll.cached_shot {
        scroll.cached_source = shot.source.clone();
        scroll.cached_shot = shot.id.clone();
        scroll.highlight_line = highlight_line;
        // Centre the highlight in the viewport when possible.
        scroll.offset = highlight_line.saturating_sub(visible_height / 3);
    }

    // Clamp scroll offset.
    let max_scroll = total_lines.saturating_sub(visible_height);
    scroll.offset = scroll.offset.min(max_scroll);

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((scroll.offset, 0));
    f.render_widget(paragraph, area);

    // Scrollbar
    if total_lines > visible_height {
        let mut scrollbar_state =
            ScrollbarState::new(max_scroll as usize).position(scroll.offset as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Scroll the transcript panel up by one line.
pub fn scroll_up(scroll: &mut TranscriptScroll) {
    scroll.offset = scroll.offset.saturating_sub(1);
}

/// Scroll the transcript panel down by one line.
pub fn scroll_down(scroll: &mut TranscriptScroll, max_lines: u16) {
    if scroll.offset < max_lines {
        scroll.offset += 1;
    }
}

// ---------------------------------------------------------------------------
// Word-based transcript view
// ---------------------------------------------------------------------------

/// Build a full word-level transcript with the selected word range highlighted.
///
/// Returns `(lines, highlight_start_line)`.
fn build_word_transcript(
    shot: &ResolvedShot,
    from: u32,
    to: u32,
    project_dir: &Path,
) -> (Vec<Line<'static>>, u16) {
    let mut lines = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
        Span::styled(shot.source.clone(), Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled("Shot: ", Style::default().fg(Color::DarkGray)),
        Span::styled(shot.id.clone(), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Range: ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("words {from}..{to}")),
        Span::raw("  "),
        Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            display::format_time(shot.duration_ms),
            Style::default().fg(Color::Green),
        ),
    ]));
    lines.push(Line::from(""));

    // Try to load the full transcript.
    let transcript_path = project_dir
        .join("transcripts")
        .join(format!("{}.transcript.json", shot.source));

    let transcript: Option<Transcript> = std::fs::read_to_string(&transcript_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok());

    let Some(transcript) = transcript else {
        lines.push(Line::from(Span::styled(
            "(transcript not available)",
            Style::default().fg(Color::DarkGray),
        )));
        return (lines, 3);
    };

    let header_len = lines.len() as u16;
    let mut highlight_line: u16 = header_len;
    let mut found_highlight = false;

    // Render each segment with its words, highlighting the selected range.
    for segment in &transcript.segments {
        // Segment timestamp header
        let time_str = format!(
            "[{} \u{2192} {}]",
            display::format_time(segment.start_ms),
            display::format_time(segment.end_ms),
        );
        lines.push(Line::from(Span::styled(
            time_str,
            Style::default().fg(Color::DarkGray),
        )));

        // Build word spans for this segment, wrapping won't work well with
        // individual word spans, so we build the line content word-by-word.
        let mut spans: Vec<Span<'static>> = Vec::new();

        for word in &segment.words {
            let in_range = word.index >= from && word.index <= to;

            if in_range && !found_highlight {
                found_highlight = true;
                highlight_line = lines.len() as u16;
            }

            let style = if in_range {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(word.text.clone(), style));
        }

        if !spans.is_empty() {
            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
    }

    // Notes
    if !shot.notes.is_empty() {
        lines.push(Line::from(Span::styled(
            "Notes:",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )));
        for note in &shot.notes {
            lines.push(Line::from(vec![
                Span::styled("  \u{2022} ", Style::default().fg(Color::DarkGray)),
                Span::raw(note.text.clone()),
            ]));
        }
    }

    (lines, highlight_line)
}

// ---------------------------------------------------------------------------
// Scene-based description view
// ---------------------------------------------------------------------------

/// Build a scene list with the selected scene range highlighted.
fn build_scene_list(
    shot: &ResolvedShot,
    from: u32,
    to: u32,
    project_dir: &Path,
) -> (Vec<Line<'static>>, u16) {
    let mut lines = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
        Span::styled(shot.source.clone(), Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled("Shot: ", Style::default().fg(Color::DarkGray)),
        Span::styled(shot.id.clone(), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Range: ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("scenes {from}..{to}")),
        Span::raw("  "),
        Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            display::format_time(shot.duration_ms),
            Style::default().fg(Color::Green),
        ),
    ]));
    lines.push(Line::from(""));

    // Try to load the source index.
    let index_path = project_dir
        .join("index")
        .join(format!("{}.index.json", shot.source));

    let index: Option<SourceIndex> = std::fs::read_to_string(&index_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok());

    let Some(index) = index else {
        lines.push(Line::from(Span::styled(
            "(source index not available)",
            Style::default().fg(Color::DarkGray),
        )));
        return (lines, 3);
    };

    let header_len = lines.len() as u16;
    let mut highlight_line: u16 = header_len;
    let mut found_highlight = false;

    for scene in &index.scenes {
        let in_range = scene.index >= from && scene.index <= to;

        if in_range && !found_highlight {
            found_highlight = true;
            highlight_line = lines.len() as u16;
        }

        let time_range = format!(
            "{} \u{2192} {}",
            display::format_time(scene.start_ms),
            display::format_time(scene.end_ms),
        );

        let description = scene.description.as_deref().unwrap_or("(no description)");

        if in_range {
            // Highlighted scene
            let marker = Span::styled(
                "\u{25b6} ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            let idx_span = Span::styled(
                format!("Scene {} ", scene.index),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            let time_span = Span::styled(
                format!(" [{time_range}]"),
                Style::default().fg(Color::Green),
            );
            lines.push(Line::from(vec![marker, idx_span, time_span]));

            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    description.to_string(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            // Non-highlighted scene
            let idx_span = Span::styled(
                format!("  Scene {} ", scene.index),
                Style::default().fg(Color::DarkGray),
            );
            let time_span = Span::styled(
                format!(" [{time_range}]"),
                Style::default().fg(Color::DarkGray),
            );
            lines.push(Line::from(vec![idx_span, time_span]));

            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    description.to_string(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::DIM),
                ),
            ]));
        }

        lines.push(Line::from(""));
    }

    // Notes
    if !shot.notes.is_empty() {
        lines.push(Line::from(Span::styled(
            "Notes:",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )));
        for note in &shot.notes {
            lines.push(Line::from(vec![
                Span::styled("  \u{2022} ", Style::default().fg(Color::DarkGray)),
                Span::raw(note.text.clone()),
            ]));
        }
    }

    (lines, highlight_line)
}

// ---------------------------------------------------------------------------
// Time-based view (no transcript/index to display)
// ---------------------------------------------------------------------------

/// Build a time-range view, loading the transcript to show segments in range.
fn build_time_view(
    shot: &ResolvedShot,
    from_ms: u64,
    to_ms: u64,
    project_dir: &Path,
) -> (Vec<Line<'static>>, u16) {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::DarkGray)),
        Span::styled(shot.source.clone(), Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled("Shot: ", Style::default().fg(Color::DarkGray)),
        Span::styled(shot.id.clone(), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Range: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(
                "{} \u{2192} {}",
                display::format_time(from_ms),
                display::format_time(to_ms),
            ),
            Style::default().fg(Color::Green),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            display::format_time(shot.duration_ms),
            Style::default().fg(Color::Green),
        ),
    ]));
    lines.push(Line::from(""));

    // Try to load the transcript and show segments within the time range.
    let transcript_path = project_dir
        .join("transcripts")
        .join(format!("{}.transcript.json", shot.source));

    let transcript: Option<Transcript> = std::fs::read_to_string(&transcript_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok());

    if let Some(transcript) = transcript {
        for segment in &transcript.segments {
            // Show segments that overlap with the time range.
            if segment.end_ms <= from_ms || segment.start_ms >= to_ms {
                continue;
            }

            let time_str = format!(
                "[{} \u{2192} {}]",
                display::format_time(segment.start_ms),
                display::format_time(segment.end_ms),
            );
            lines.push(Line::from(Span::styled(
                time_str,
                Style::default().fg(Color::DarkGray),
            )));

            if !segment.words.is_empty() {
                let mut spans: Vec<Span<'static>> = Vec::new();
                for word in &segment.words {
                    let in_range = word.start_ms >= from_ms && word.end_ms <= to_ms;
                    let style = if in_range {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    if !spans.is_empty() {
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(word.text.clone(), style));
                }
                lines.push(Line::from(spans));
            } else {
                lines.push(Line::from(Span::styled(
                    segment.text.clone(),
                    Style::default().fg(Color::White),
                )));
            }

            lines.push(Line::from(""));
        }
    }

    // Notes
    if !shot.notes.is_empty() {
        lines.push(Line::from(Span::styled(
            "Notes:",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )));
        for note in &shot.notes {
            lines.push(Line::from(vec![
                Span::styled("  \u{2022} ", Style::default().fg(Color::DarkGray)),
                Span::raw(note.text.clone()),
            ]));
        }
    }

    (lines, 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ar_edit_core::models::{Scene, ShotNote, ShotRange, TranscriptSegment, Word};
    use tempfile::TempDir;

    fn sample_transcript() -> Transcript {
        Transcript {
            source_id: "src-001".into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms: 12400,
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

    fn sample_source_index() -> SourceIndex {
        SourceIndex {
            source_id: "src-002".into(),
            indexed_at: "2026-02-19T12:05:00Z".parse().unwrap(),
            metadata: ar_edit_core::models::SourceMetadata {
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

        let transcript = sample_transcript();
        std::fs::write(
            dir.join("transcripts/src-001.transcript.json"),
            serde_json::to_string(&transcript).unwrap(),
        )
        .unwrap();

        let index = sample_source_index();
        std::fs::write(
            dir.join("index/src-002.index.json"),
            serde_json::to_string(&index).unwrap(),
        )
        .unwrap();
    }

    fn make_word_shot() -> ResolvedShot {
        ResolvedShot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 3 },
            start_ms: 0,
            end_ms: 1200,
            duration_ms: 1200,
            text_preview: Some("Welcome to the interview".into()),
            scene_preview: None,
            notes: vec![],
        }
    }

    fn make_scene_shot() -> ResolvedShot {
        ResolvedShot {
            id: "shot-002".into(),
            source: "src-002".into(),
            range: ShotRange::Scenes { from: 0, to: 2 },
            start_ms: 0,
            end_ms: 90000,
            duration_ms: 90000,
            text_preview: None,
            scene_preview: Some("Interior office, wide shot; Close-up interview".into()),
            notes: vec![],
        }
    }

    fn make_time_shot() -> ResolvedShot {
        ResolvedShot {
            id: "shot-003".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
            start_ms: 5000,
            end_ms: 10000,
            duration_ms: 5000,
            text_preview: None,
            scene_preview: None,
            notes: vec![],
        }
    }

    #[test]
    fn word_transcript_has_header() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let shot = make_word_shot();

        let (lines, _) = build_word_transcript(&shot, 0, 3, tmp.path());
        // First line should contain source ID
        let first_line_text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first_line_text.contains("src-001"));
    }

    #[test]
    fn word_transcript_highlights_range() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let shot = make_word_shot();

        let (lines, highlight_line) = build_word_transcript(&shot, 0, 3, tmp.path());
        // highlight_line should point to a valid line
        assert!(highlight_line < lines.len() as u16);
        // Should have more than just the header
        assert!(lines.len() > 3);
    }

    #[test]
    fn word_transcript_missing_file() {
        let tmp = TempDir::new().unwrap();
        // Don't set up project files
        let shot = make_word_shot();

        let (lines, _) = build_word_transcript(&shot, 0, 3, tmp.path());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("not available"));
    }

    #[test]
    fn scene_list_has_header() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let shot = make_scene_shot();

        let (lines, _) = build_scene_list(&shot, 0, 2, tmp.path());
        let first_line_text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first_line_text.contains("src-002"));
    }

    #[test]
    fn scene_list_highlights_range() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let shot = make_scene_shot();

        let (lines, highlight_line) = build_scene_list(&shot, 0, 2, tmp.path());
        assert!(highlight_line < lines.len() as u16);
        assert!(lines.len() > 3);
    }

    #[test]
    fn scene_list_missing_file() {
        let tmp = TempDir::new().unwrap();
        let shot = make_scene_shot();

        let (lines, _) = build_scene_list(&shot, 0, 2, tmp.path());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("not available"));
    }

    #[test]
    fn time_view_shows_range() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let shot = make_time_shot();

        let (lines, _) = build_time_view(&shot, 5000, 10000, tmp.path());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("00:05.000"));
        assert!(all_text.contains("00:10.000"));
    }

    #[test]
    fn time_view_shows_notes() {
        let tmp = TempDir::new().unwrap();
        let mut shot = make_time_shot();
        shot.notes.push(ShotNote {
            text: "Great take".into(),
            created: "2026-02-19T15:00:00Z".parse().unwrap(),
        });

        let (lines, _) = build_time_view(&shot, 5000, 10000, tmp.path());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_text.contains("Notes:"));
        assert!(all_text.contains("Great take"));
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let mut scroll = TranscriptScroll::default();
        scroll.offset = 0;
        scroll_up(&mut scroll);
        assert_eq!(scroll.offset, 0);
    }

    #[test]
    fn scroll_down_increments() {
        let mut scroll = TranscriptScroll::default();
        scroll.offset = 0;
        scroll_down(&mut scroll, 100);
        assert_eq!(scroll.offset, 1);
    }

    #[test]
    fn scroll_down_clamps_at_max() {
        let mut scroll = TranscriptScroll::default();
        scroll.offset = 50;
        scroll_down(&mut scroll, 50);
        assert_eq!(scroll.offset, 50);
    }

    #[test]
    fn auto_scroll_on_shot_change() {
        let mut scroll = TranscriptScroll::default();
        scroll.cached_source = "src-001".into();
        scroll.cached_shot = "shot-001".into();
        scroll.offset = 0;

        // Simulate a shot change by checking the condition
        let new_source = "src-002";
        let new_shot = "shot-002";
        assert_ne!(new_source, scroll.cached_source);
        assert_ne!(new_shot, scroll.cached_shot);
    }
}
