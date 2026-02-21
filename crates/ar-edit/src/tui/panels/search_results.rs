use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use ar_edit_core::display;
use ar_edit_core::search::{ResultType, SearchResult, TypeFilter};

// ---------------------------------------------------------------------------
// Public draw function (REQ-045)
// ---------------------------------------------------------------------------

/// Render the search results panel: a navigable list of unified search results
/// showing source ID, timestamp, result type, and context snippet.
pub fn draw(
    f: &mut Frame,
    results: &[SearchResult],
    selected: &mut ListState,
    query: &str,
    type_filter: Option<&TypeFilter>,
    area: Rect,
) {
    let filter_label = match type_filter {
        None => "all",
        Some(TypeFilter::Transcript) => "transcript",
        Some(TypeFilter::Scene) => "scene",
        Some(TypeFilter::Metadata) => "metadata",
    };

    let title = format!(
        " Search: \"{}\" ({} results, filter: {}) ",
        query,
        results.len(),
        filter_label,
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL);

    if results.is_empty() {
        let msg = if query.is_empty() {
            "Type / to search"
        } else {
            "No results found"
        };
        let paragraph = Paragraph::new(msg)
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .map(|(i, result)| format_result_item(i, result))
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, selected);
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn format_result_item(index: usize, result: &SearchResult) -> ListItem<'static> {
    let type_tag = match result.result_type {
        ResultType::Transcript => ("TRN", Color::Cyan),
        ResultType::Scene => ("SCN", Color::Yellow),
        ResultType::Metadata => ("MRK", Color::Magenta),
    };

    let time_range = format!(
        "{}\u{2192}{}",
        display::format_time(result.start_ms),
        display::format_time(result.end_ms),
    );

    // Line 1: index, type badge, source ID, time range
    let line1 = Line::from(vec![
        Span::styled(
            format!("{:>3} ", index + 1),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("[{}]", type_tag.0),
            Style::default().fg(type_tag.1).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            result.source_id.clone(),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(time_range, Style::default().fg(Color::Green)),
    ]);

    // Line 2: context snippet
    let context = truncate_context(&result.context, 72);
    let line2 = Line::from(vec![
        Span::raw("      "),
        Span::styled(
            context,
            Style::default().fg(Color::White).add_modifier(Modifier::DIM),
        ),
    ]);

    ListItem::new(Text::from(vec![line1, line2]))
}

/// Truncate a context string to fit within `max_chars`, appending "..." if needed.
fn truncate_context(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let mut truncated = s[..max_chars.saturating_sub(3)].to_string();
        truncated.push_str("...");
        truncated
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_results() -> Vec<SearchResult> {
        vec![
            SearchResult {
                result_type: ResultType::Transcript,
                source_id: "src-001".into(),
                start_ms: 200,
                end_ms: 600,
                matched_text: "climate".into(),
                context: "...The big [climate] policy debate...".into(),
            },
            SearchResult {
                result_type: ResultType::Scene,
                source_id: "src-002".into(),
                start_ms: 18000,
                end_ms: 45000,
                matched_text: "Close-up interview".into(),
                context: "scene 1: Close-up interview".into(),
            },
            SearchResult {
                result_type: ResultType::Metadata,
                source_id: "src-001".into(),
                start_ms: 5000,
                end_ms: 10000,
                matched_text: "hero".into(),
                context: "mark-001 [hero] \"Best take\"".into(),
            },
        ]
    }

    #[test]
    fn format_result_transcript() {
        let results = sample_results();
        let item = format_result_item(0, &results[0]);
        let _ = item; // smoke test — no panic
    }

    #[test]
    fn format_result_scene() {
        let results = sample_results();
        let item = format_result_item(1, &results[1]);
        let _ = item;
    }

    #[test]
    fn format_result_metadata() {
        let results = sample_results();
        let item = format_result_item(2, &results[2]);
        let _ = item;
    }

    #[test]
    fn truncate_context_short() {
        let result = truncate_context("short text", 72);
        assert_eq!(result, "short text");
    }

    #[test]
    fn truncate_context_long() {
        let long = "a".repeat(100);
        let result = truncate_context(&long, 72);
        assert!(result.len() <= 72);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_context_exact_boundary() {
        let exact = "a".repeat(72);
        let result = truncate_context(&exact, 72);
        assert_eq!(result.len(), 72);
        assert!(!result.ends_with("..."));
    }
}
