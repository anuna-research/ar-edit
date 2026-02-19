use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use ar_edit_core::display::{self, ResolvedShot};

use crate::tui::Mode;

// ---------------------------------------------------------------------------
// Render progress state (REQ-044)
// ---------------------------------------------------------------------------

/// Tracks the progress of an in-progress render operation.
#[derive(Debug, Clone)]
pub struct RenderProgress {
    pub current_shot: usize,
    pub total_shots: usize,
    /// 0.0 .. 1.0
    pub fraction: f64,
    pub eta_secs: Option<u64>,
}

// ---------------------------------------------------------------------------
// Public draw function
// ---------------------------------------------------------------------------

/// Render the status bar: mode indicator, playback position, render progress,
/// and status message.
///
/// The bar is a single-line strip at the bottom of the screen.
///
/// Layout:
///   [MODE]  edit: <name>  shot X/Y  pos/total  | <status_message>
///
/// When a render is in progress the middle section shows a progress bar:
///   [MODE]  Rendering 3/5 [████░░░░] 48%  ETA 00:32  | <status_message>
pub fn draw(
    f: &mut Frame,
    mode: Mode,
    edit_name: Option<&str>,
    selected_idx: Option<usize>,
    shots: &[ResolvedShot],
    render_progress: Option<&RenderProgress>,
    status_message: &str,
    area: Rect,
) {
    let width = area.width as usize;
    if width == 0 {
        return;
    }

    // -- Build the left section: mode badge -----------------------------------
    let (mode_label, mode_style) = mode_badge(mode);

    // -- Build the middle section ---------------------------------------------
    let middle = match render_progress {
        Some(rp) => render_progress_text(rp, area.width),
        None => position_text(edit_name, selected_idx, shots),
    };

    // -- Compose into a single Line with styled spans -------------------------
    let mut spans: Vec<Span> = Vec::new();

    // Mode badge
    spans.push(Span::styled(
        format!(" {mode_label} "),
        mode_style,
    ));
    spans.push(Span::raw("  "));

    // Middle section
    spans.extend(middle);

    // Separator + status message
    if !status_message.is_empty() {
        spans.push(Span::styled("  | ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::raw(status_message));
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// Mode badge
// ---------------------------------------------------------------------------

fn mode_badge(mode: Mode) -> (&'static str, Style) {
    match mode {
        Mode::Normal => (
            "NORMAL",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Mode::Command => (
            "COMMAND",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Mode::Prompt => (
            "PROMPT",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Mode::Search => (
            "SEARCH",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    }
}

// ---------------------------------------------------------------------------
// Position text (normal operation)
// ---------------------------------------------------------------------------

fn position_text<'a>(
    edit_name: Option<&str>,
    selected_idx: Option<usize>,
    shots: &[ResolvedShot],
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    // Edit name
    let label = edit_name.unwrap_or("(none)");
    spans.push(Span::styled("edit: ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(
        label.to_string(),
        Style::default().fg(Color::White),
    ));

    if shots.is_empty() {
        return spans;
    }

    // Shot position: "shot 2/5"
    if let Some(idx) = selected_idx {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("shot {}/{}", idx + 1, shots.len()),
            Style::default().fg(Color::Cyan),
        ));

        // Playback position: show selected shot's position within the total
        // timeline as "pos / total".
        let total_ms: u64 = shots.iter().map(|s| s.duration_ms).sum();
        let position_ms: u64 = shots.iter().take(idx).map(|s| s.duration_ms).sum();

        if total_ms > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!(
                    "{}/{}",
                    display::format_time(position_ms),
                    display::format_time(total_ms)
                ),
                Style::default().fg(Color::Green),
            ));
        }
    }

    spans
}

// ---------------------------------------------------------------------------
// Render progress bar (REQ-044)
// ---------------------------------------------------------------------------

fn render_progress_text(rp: &RenderProgress, width: u16) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    // "Rendering 3/5"
    spans.push(Span::styled(
        format!("Rendering {}/{}", rp.current_shot, rp.total_shots),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("  "));

    // Progress bar: [████░░░░]
    //
    // We give the bar roughly 1/3 of the available width, clamped to a
    // reasonable range so it looks decent on small and large terminals.
    let bar_width = ((width as usize) / 3).clamp(8, 30);
    let filled = ((rp.fraction * bar_width as f64).round() as usize).min(bar_width);
    let empty = bar_width - filled;

    let bar_str = format!(
        "[{}{}]",
        "\u{2588}".repeat(filled),   // █ (full block)
        "\u{2591}".repeat(empty),     // ░ (light shade)
    );
    spans.push(Span::styled(bar_str, Style::default().fg(Color::Green)));

    // Percentage
    let pct = (rp.fraction * 100.0).round() as u32;
    spans.push(Span::raw(format!(" {pct}%")));

    // ETA
    if let Some(eta) = rp.eta_secs {
        let mins = eta / 60;
        let secs = eta % 60;
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("ETA {mins:02}:{secs:02}"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    spans
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ar_edit_core::models::ShotRange;

    fn sample_shots() -> Vec<ResolvedShot> {
        vec![
            ResolvedShot {
                id: "shot-001".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 0, to: 3 },
                start_ms: 0,
                end_ms: 5000,
                duration_ms: 5000,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
            ResolvedShot {
                id: "shot-002".into(),
                source: "src-001".into(),
                range: ShotRange::Words { from: 4, to: 7 },
                start_ms: 5000,
                end_ms: 10000,
                duration_ms: 5000,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
            ResolvedShot {
                id: "shot-003".into(),
                source: "src-002".into(),
                range: ShotRange::Time {
                    from_ms: 0,
                    to_ms: 20000,
                },
                start_ms: 0,
                end_ms: 20000,
                duration_ms: 20000,
                text_preview: None,
                scene_preview: None,
                notes: vec![],
            },
        ]
    }

    #[test]
    fn mode_badge_normal() {
        let (label, style) = mode_badge(Mode::Normal);
        assert_eq!(label, "NORMAL");
        assert_eq!(style.fg, Some(Color::Black));
        assert_eq!(style.bg, Some(Color::Blue));
    }

    #[test]
    fn mode_badge_search() {
        let (label, _) = mode_badge(Mode::Search);
        assert_eq!(label, "SEARCH");
    }

    #[test]
    fn mode_badge_command() {
        let (label, _) = mode_badge(Mode::Command);
        assert_eq!(label, "COMMAND");
    }

    #[test]
    fn mode_badge_prompt() {
        let (label, _) = mode_badge(Mode::Prompt);
        assert_eq!(label, "PROMPT");
    }

    #[test]
    fn position_text_no_edit() {
        let spans = position_text(None, None, &[]);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("(none)"));
    }

    #[test]
    fn position_text_with_shots() {
        let shots = sample_shots();
        let spans = position_text(Some("my-edit"), Some(1), &shots);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("my-edit"));
        assert!(text.contains("shot 2/3"));
        // Position at shot index 1 = 5000ms into total 30000ms
        assert!(text.contains("00:05.000"));
        assert!(text.contains("00:30.000"));
    }

    #[test]
    fn position_text_first_shot() {
        let shots = sample_shots();
        let spans = position_text(Some("edit"), Some(0), &shots);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("shot 1/3"));
        // First shot => position 0
        assert!(text.contains("00:00.000"));
    }

    #[test]
    fn render_progress_displays_bar() {
        let rp = RenderProgress {
            current_shot: 3,
            total_shots: 5,
            fraction: 0.5,
            eta_secs: Some(45),
        };
        let spans = render_progress_text(&rp, 80);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("Rendering 3/5"));
        assert!(text.contains("50%"));
        assert!(text.contains("ETA 00:45"));
        assert!(text.contains('['));
        assert!(text.contains(']'));
    }

    #[test]
    fn render_progress_no_eta() {
        let rp = RenderProgress {
            current_shot: 1,
            total_shots: 10,
            fraction: 0.1,
            eta_secs: None,
        };
        let spans = render_progress_text(&rp, 60);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("Rendering 1/10"));
        assert!(text.contains("10%"));
        assert!(!text.contains("ETA"));
    }

    #[test]
    fn render_progress_complete() {
        let rp = RenderProgress {
            current_shot: 5,
            total_shots: 5,
            fraction: 1.0,
            eta_secs: Some(0),
        };
        let spans = render_progress_text(&rp, 80);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("100%"));
    }

    #[test]
    fn render_progress_narrow_terminal() {
        let rp = RenderProgress {
            current_shot: 2,
            total_shots: 4,
            fraction: 0.25,
            eta_secs: None,
        };
        // Very narrow — bar should still render at minimum width of 8
        let spans = render_progress_text(&rp, 20);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("25%"));
    }
}
