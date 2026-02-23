use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::{Shot, ShotRange};

// ---------------------------------------------------------------------------
// Public draw function
// ---------------------------------------------------------------------------

/// Render the timeline panel: a vertical shot list with highlighted selection.
///
/// Displays shot ID, source ID, duration, and text preview for each shot.
/// Scrolling is handled automatically by `ListState`.
pub fn draw(f: &mut Frame, shots: &[ResolvedShot], selected: &mut ListState, area: Rect) {
    let block = Block::default().title(" Timeline ").borders(Borders::ALL);

    if shots.is_empty() {
        let items = vec![ListItem::new(
            Line::from("(no shots)").style(Style::default().fg(Color::DarkGray)),
        )];
        let list = List::new(items).block(block);
        f.render_widget(list, area);
        return;
    }

    let items: Vec<ListItem> = shots
        .iter()
        .enumerate()
        .map(|(i, shot)| format_shot_item(i, shot))
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, selected);
}

// ---------------------------------------------------------------------------
// Fallback resolution
// ---------------------------------------------------------------------------

/// Build display-ready shots from raw snapshot data when full resolution fails
/// (e.g. missing transcript or index files on disk).
pub fn fallback_resolved(shots: &[Shot]) -> Vec<ResolvedShot> {
    shots
        .iter()
        .map(|shot| {
            let (start_ms, end_ms) = match &shot.range {
                ShotRange::Time { from_ms, to_ms } => (*from_ms, *to_ms),
                _ => (0, 0),
            };
            ResolvedShot {
                id: shot.id.clone(),
                source: shot.source.clone(),
                range: shot.range.clone(),
                start_ms,
                end_ms,
                duration_ms: end_ms.saturating_sub(start_ms),
                text_preview: None,
                scene_preview: None,
                notes: shot.notes.clone(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn format_shot_item(index: usize, shot: &ResolvedShot) -> ListItem<'static> {
    let duration = display::format_time(shot.duration_ms);

    // Line 1: index, shot ID, source ID, duration
    let line1 = Line::from(vec![
        Span::styled(
            format!("{:>2} ", index + 1),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(shot.id.clone(), Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(shot.source.clone(), Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(duration, Style::default().fg(Color::Green)),
    ]);

    // Line 2: range indicator + text preview
    let (range_tag, preview) = range_display(shot);

    let mut spans = vec![
        Span::raw("     "),
        Span::styled(range_tag, Style::default().fg(Color::DarkGray)),
    ];

    if !preview.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("\u{201c}{preview}\u{201d}"),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::DIM),
        ));
    }

    let line2 = Line::from(spans);

    ListItem::new(Text::from(vec![line1, line2]))
}

/// Produce a short range tag and optional preview text for a resolved shot.
fn range_display(shot: &ResolvedShot) -> (String, String) {
    match &shot.range {
        ShotRange::Words { from, to } => (
            format!("W {from}..{to}"),
            shot.text_preview.clone().unwrap_or_default(),
        ),
        ShotRange::Scenes { from, to } => (
            format!("S {from}..{to}"),
            shot.scene_preview.clone().unwrap_or_default(),
        ),
        ShotRange::Time { from_ms, to_ms } => (
            format!(
                "T {}\u{2192}{}",
                display::format_time(*from_ms),
                display::format_time(*to_ms)
            ),
            String::new(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ar_edit_core::models::{Shot, ShotNote, ShotRange};

    fn sample_resolved() -> Vec<ResolvedShot> {
        vec![
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
            },
            ResolvedShot {
                id: "shot-002".into(),
                source: "src-002".into(),
                range: ShotRange::Scenes { from: 0, to: 2 },
                start_ms: 0,
                end_ms: 90000,
                duration_ms: 90000,
                text_preview: None,
                scene_preview: Some("Interior office; Close-up interview".into()),
                notes: vec![],
            },
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
            },
        ]
    }

    #[test]
    fn fallback_preserves_shot_ids() {
        let shots = vec![
            Shot {
                id: "shot-001".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 52 },
                notes: vec![],
            },
            Shot {
                id: "shot-002".into(),
                source: "src-002".into(),
                range: ShotRange::Time {
                    from_ms: 5000,
                    to_ms: 10000,
                },
                notes: vec![],
            },
        ];

        let resolved = fallback_resolved(&shots);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id, "shot-001");
        assert_eq!(resolved[1].id, "shot-002");
    }

    #[test]
    fn fallback_time_range_has_duration() {
        let shots = vec![Shot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000,
            },
            notes: vec![],
        }];

        let resolved = fallback_resolved(&shots);
        assert_eq!(resolved[0].duration_ms, 5000);
        assert_eq!(resolved[0].start_ms, 5000);
        assert_eq!(resolved[0].end_ms, 10000);
    }

    #[test]
    fn fallback_word_range_zero_duration() {
        let shots = vec![Shot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 52 },
            notes: vec![],
        }];

        let resolved = fallback_resolved(&shots);
        assert_eq!(resolved[0].duration_ms, 0);
        assert!(resolved[0].text_preview.is_none());
    }

    #[test]
    fn fallback_preserves_notes() {
        let note = ShotNote {
            text: "Great take".into(),
            created: "2026-02-19T15:00:00Z".parse().unwrap(),
        };
        let shots = vec![Shot {
            id: "shot-001".into(),
            source: "src-001".into(),
            range: ShotRange::Words { from: 0, to: 52 },
            notes: vec![note.clone()],
        }];

        let resolved = fallback_resolved(&shots);
        assert_eq!(resolved[0].notes.len(), 1);
        assert_eq!(resolved[0].notes[0].text, "Great take");
    }

    #[test]
    fn range_display_words() {
        let shots = sample_resolved();
        let (tag, preview) = range_display(&shots[0]);
        assert_eq!(tag, "W 0..3");
        assert_eq!(preview, "Welcome to the interview");
    }

    #[test]
    fn range_display_scenes() {
        let shots = sample_resolved();
        let (tag, preview) = range_display(&shots[1]);
        assert_eq!(tag, "S 0..2");
        assert_eq!(preview, "Interior office; Close-up interview");
    }

    #[test]
    fn range_display_time() {
        let shots = sample_resolved();
        let (tag, preview) = range_display(&shots[2]);
        assert_eq!(tag, "T 00:05.000\u{2192}00:10.000");
        assert!(preview.is_empty());
    }
}
