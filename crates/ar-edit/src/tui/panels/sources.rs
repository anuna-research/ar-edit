use std::path::Path;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::StatefulImage;

use ar_edit_core::display;
use ar_edit_core::models::Source;

// ---------------------------------------------------------------------------
// Public draw function
// ---------------------------------------------------------------------------

/// Render the sources panel: a selectable list of registered sources with
/// status indicators for transcription/indexing and duration.
pub fn draw(f: &mut Frame, sources: &[Source], selected: &mut ListState, area: Rect) {
    let block = Block::default().title(" Sources ").borders(Borders::ALL);

    if sources.is_empty() {
        let items = vec![ListItem::new(
            Line::from("(no sources)").style(Style::default().fg(Color::DarkGray)),
        )];
        let list = List::new(items).block(block);
        f.render_widget(list, area);
        return;
    }

    let items: Vec<ListItem> = sources.iter().map(|s| format_source_item(s)).collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, selected);
}

/// Render a detail view for a single source (transcript/index info + thumbnail).
pub fn draw_detail(
    f: &mut Frame,
    source: &Source,
    thumbnail_cache: &mut Option<(String, ratatui_image::protocol::StatefulProtocol)>,
    area: Rect,
) {
    let block = Block::default()
        .title(format!(" Source: {} ", source.id))
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let duration = display::format_time(source.duration_ms);
    let (w, h) = source.resolution;

    let mut lines = vec![
        Line::from(vec![
            Span::styled("File: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&source.original_filename),
        ]),
        Line::from(vec![
            Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
            Span::styled(duration, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Resolution: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{w}x{h}")),
        ]),
        Line::from(vec![
            Span::styled("Codecs: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} / {}", source.video_codec, source.audio_codec)),
        ]),
        Line::from(vec![
            Span::styled("Frame rate: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{:.2} fps", source.frame_rate)),
        ]),
        Line::from(vec![
            Span::styled("Audio: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(
                "{}ch @ {} Hz",
                source.audio_channels, source.audio_sample_rate
            )),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Transcribed: ", Style::default().fg(Color::DarkGray)),
            status_span(source.transcribed),
        ]),
        Line::from(vec![
            Span::styled("Indexed: ", Style::default().fg(Color::DarkGray)),
            status_span(source.indexed),
        ]),
    ];

    let added = source.added.format("%Y-%m-%d %H:%M").to_string();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Added: ", Style::default().fg(Color::DarkGray)),
        Span::raw(added),
    ]));

    let text_height = lines.len() as u16;

    // Render text info at the top
    let text_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: text_height.min(inner.height),
    };
    let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true });
    f.render_widget(paragraph, text_area);

    // Render thumbnail below the text if we have one
    if let Some((ref id, ref mut protocol)) = thumbnail_cache {
        if *id == source.id && inner.height > text_height + 1 {
            let img_area = Rect {
                x: inner.x,
                y: inner.y + text_height + 1,
                width: inner.width,
                height: inner.height.saturating_sub(text_height + 1),
            };
            let image_widget = StatefulImage::default();
            f.render_stateful_widget(image_widget, img_area, protocol);
        }
    }
}

/// Load the first thumbnail for a source, returning the cached protocol state.
pub fn load_thumbnail(
    picker: &ratatui_image::picker::Picker,
    source: &Source,
    project_dir: &Path,
) -> Option<(String, ratatui_image::protocol::StatefulProtocol)> {
    // Try loading the source index to find thumbnails
    let index = ar_edit_core::index::load_index(project_dir, &source.id).ok()?;
    let thumb_path = if let Some(thumb) = index.thumbnails.first() {
        project_dir.join(&thumb.path)
    } else if let Some(scene) = index.scenes.first() {
        project_dir.join(&scene.thumbnail)
    } else {
        return None;
    };
    let dyn_img = image::ImageReader::open(&thumb_path).ok()?.decode().ok()?;
    let protocol = picker.new_resize_protocol(dyn_img);
    Some((source.id.clone(), protocol))
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn format_source_item(source: &Source) -> ListItem<'static> {
    let duration = display::format_time(source.duration_ms);

    let transcribed = if source.transcribed { "T" } else { "-" };
    let indexed = if source.indexed { "I" } else { "-" };

    let line = Line::from(vec![
        Span::styled(source.id.clone(), Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(
            source.original_filename.clone(),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled(duration, Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(
            format!("[{transcribed}{indexed}]"),
            Style::default().fg(if source.transcribed && source.indexed {
                Color::Green
            } else if source.transcribed || source.indexed {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
    ]);

    ListItem::new(line)
}

fn status_span(done: bool) -> Span<'static> {
    if done {
        Span::styled("yes", Style::default().fg(Color::Green))
    } else {
        Span::styled("no", Style::default().fg(Color::DarkGray))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_source(id: &str, transcribed: bool, indexed: bool) -> Source {
        Source {
            id: id.to_string(),
            path: PathBuf::from(format!("sources/{id}.mp4")),
            original_filename: format!("{id}.mp4"),
            duration_ms: 60_000,
            video_codec: "h264".into(),
            audio_codec: "aac".into(),
            resolution: (1920, 1080),
            frame_rate: 29.97,
            audio_channels: 2,
            audio_sample_rate: 48000,
            added: "2026-02-19T15:00:00Z".parse().unwrap(),
            transcribed,
            indexed,
        }
    }

    #[test]
    fn format_source_shows_id_and_filename() {
        let source = sample_source("src-001", false, false);
        let item = format_source_item(&source);
        // ListItem was created without panic — basic smoke test.
        let _ = item;
    }

    #[test]
    fn format_source_status_both_done() {
        let source = sample_source("src-001", true, true);
        let item = format_source_item(&source);
        let _ = item;
    }

    #[test]
    fn format_source_status_partial() {
        let source = sample_source("src-001", true, false);
        let item = format_source_item(&source);
        let _ = item;
    }

    #[test]
    fn status_span_yes() {
        let span = status_span(true);
        assert_eq!(span.content, "yes");
    }

    #[test]
    fn status_span_no() {
        let span = status_span(false);
        assert_eq!(span.content, "no");
    }
}
