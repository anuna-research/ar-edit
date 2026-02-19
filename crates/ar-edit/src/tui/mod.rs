mod input;
mod panels;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, ListState, Paragraph};

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::{EditDocument, ShotRange, Source};
use ar_edit_core::playback;

// ---------------------------------------------------------------------------
// App mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Command,
    Prompt,
    Search,
}

/// Which panel currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Timeline,
    Sources,
}

// ---------------------------------------------------------------------------
// Prompt action — what to do when a prompt is submitted
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum PromptAction {
    AddShotSource,
    AddShotRange { source: String },
    TrimShot,
    NoteInput,
    MarkerSource,
    MarkerRange { source: String },
    MarkerLabel { source: String, range: ShotRange },
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct App {
    pub edit: Option<EditDocument>,
    pub sources: Vec<Source>,
    pub resolved_shots: Vec<ResolvedShot>,
    pub selected_shot: ListState,
    pub selected_source: ListState,
    pub focus: Focus,
    pub mode: Mode,
    pub project_dir: PathBuf,
    pub status_message: String,
    pub should_quit: bool,
    pub edit_path: Option<PathBuf>,
    pub prompt_label: String,
    pub prompt_buffer: String,
    pub prompt_action: Option<PromptAction>,
    pub pending_play: Option<playback::PlayRequest>,
}

impl App {
    pub fn new(project_dir: PathBuf) -> Self {
        let mut selected_shot = ListState::default();
        selected_shot.select(Some(0));

        let mut selected_source = ListState::default();
        selected_source.select(Some(0));

        Self {
            edit: None,
            sources: Vec::new(),
            resolved_shots: Vec::new(),
            selected_shot,
            selected_source,
            focus: Focus::Timeline,
            mode: Mode::Normal,
            project_dir,
            status_message: input::default_status(),
            should_quit: false,
            edit_path: None,
            prompt_label: String::new(),
            prompt_buffer: String::new(),
            prompt_action: None,
            pending_play: None,
        }
    }

    /// Load the first edit document found in the project's `edits/` directory.
    pub fn load_project(&mut self) -> anyhow::Result<()> {
        // Load manifest for source list
        let manifest = ar_edit_core::project::read_manifest(&self.project_dir)?;
        self.sources = manifest.sources;

        // Load the first edit found
        let edits_dir = self.project_dir.join("edits");
        if edits_dir.is_dir() {
            if let Some(entry) = std::fs::read_dir(&edits_dir)?
                .filter_map(|e| e.ok())
                .find(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|ext| ext == "json")
                })
            {
                let path = entry.path();
                let doc =
                    EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
                self.status_message = format!("Loaded edit: {}", doc.name);
                self.edit = Some(doc);
                self.edit_path = Some(path);
                self.resolve_shots();
            }
        }

        Ok(())
    }

    /// Resolve shots from the current edit document for display.
    ///
    /// Uses `display::resolve_edit` to get duration and text previews. Falls
    /// back to basic shot data if resolution fails (e.g. missing transcripts).
    pub(crate) fn resolve_shots(&mut self) {
        self.resolved_shots = match &self.edit {
            Some(doc) => match display::resolve_edit(doc, &self.project_dir) {
                Ok(resolved) => resolved,
                Err(_) => panels::timeline::fallback_resolved(&doc.snapshot.shots),
            },
            None => Vec::new(),
        };
    }

    pub(crate) fn shot_count(&self) -> usize {
        self.resolved_shots.len()
    }

    pub(crate) fn select_next(&mut self) {
        let count = self.shot_count();
        if count == 0 {
            return;
        }
        let i = self.selected_shot.selected().unwrap_or(0);
        self.selected_shot.select(Some((i + 1).min(count - 1)));
    }

    pub(crate) fn select_previous(&mut self) {
        let i = self.selected_shot.selected().unwrap_or(0);
        self.selected_shot.select(Some(i.saturating_sub(1)));
    }

    pub(crate) fn selected_resolved_shot(&self) -> Option<&ResolvedShot> {
        let idx = self.selected_shot.selected()?;
        self.resolved_shots.get(idx)
    }

    pub(crate) fn selected_shot_id(&self) -> Option<String> {
        let idx = self.selected_shot.selected()?;
        self.resolved_shots.get(idx).map(|s| s.id.clone())
    }

    pub(crate) fn select_next_source(&mut self) {
        let count = self.sources.len();
        if count == 0 {
            return;
        }
        let i = self.selected_source.selected().unwrap_or(0);
        self.selected_source.select(Some((i + 1).min(count - 1)));
    }

    pub(crate) fn select_previous_source(&mut self) {
        let i = self.selected_source.selected().unwrap_or(0);
        self.selected_source.select(Some(i.saturating_sub(1)));
    }

    pub(crate) fn selected_source(&self) -> Option<&Source> {
        let idx = self.selected_source.selected()?;
        self.sources.get(idx)
    }
}

// ---------------------------------------------------------------------------
// Terminal setup / teardown
// ---------------------------------------------------------------------------

type Term = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> anyhow::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Term) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(project_dir: PathBuf) -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project_dir);

    // Best-effort project load; show error in status bar if it fails.
    if let Err(e) = app.load_project() {
        app.status_message = format!("Could not load project: {e:#}");
    }

    let result = event_loop(&mut terminal, &mut app);

    // Always restore the terminal, even on error.
    restore_terminal(&mut terminal)?;

    result
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

const TICK_RATE: Duration = Duration::from_millis(250);

fn event_loop(terminal: &mut Term, app: &mut App) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, app))?;

        let timeout = TICK_RATE.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                input::handle_key(app, key);
            }
        }

        if last_tick.elapsed() >= TICK_RATE {
            last_tick = Instant::now();
        }

        // Handle pending play: leave TUI, launch player, re-enter TUI.
        if let Some(req) = app.pending_play.take() {
            match playback::detect_player() {
                Ok(player) => {
                    restore_terminal(terminal)?;
                    match playback::launch_player(&player, &req) {
                        Ok(mut child) => {
                            let _ = child.wait();
                        }
                        Err(e) => {
                            app.status_message = format!("Play failed: {e}");
                        }
                    }
                    *terminal = setup_terminal()?;
                }
                Err(_) => {
                    app.status_message =
                        "No video player found (install VLC or ffplay)".into();
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// UI layout
// ---------------------------------------------------------------------------

fn ui(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Top-level: main area (everything except bottom status bar)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    let main_area = outer[0];
    let status_area = outer[1];

    // Main area: left column | right column
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_area);

    // Left column: timeline (top) + sources (bottom)
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(columns[0]);

    let timeline_area = left[0];
    let sources_area = left[1];
    let transcript_area = columns[1];

    // --- Timeline panel (REQ-039) ---
    panels::timeline::draw(
        f,
        &app.resolved_shots,
        &mut app.selected_shot,
        timeline_area,
    );

    // --- Right panel: transcript or source detail ---
    match app.focus {
        Focus::Sources => {
            if let Some(source) = app.selected_source().cloned() {
                panels::sources::draw_detail(f, &source, transcript_area);
            } else {
                draw_transcript(f, app, transcript_area);
            }
        }
        Focus::Timeline => {
            draw_transcript(f, app, transcript_area);
        }
    }

    // --- Sources panel (REQ-041) ---
    panels::sources::draw(
        f,
        &app.sources,
        &mut app.selected_source,
        sources_area,
    );

    // --- Status bar ---
    draw_status(f, app, status_area);
}

// ---------------------------------------------------------------------------
// Panel renderers
// ---------------------------------------------------------------------------

fn draw_transcript(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Transcript ")
        .borders(Borders::ALL);

    let text = match app.selected_resolved_shot() {
        Some(shot) => {
            let mut lines = format!("Shot: {}\nSource: {}\n", shot.id, shot.source);
            lines.push_str(&format!(
                "Duration: {}\n",
                display::format_time(shot.duration_ms)
            ));
            if let Some(ref preview) = shot.text_preview {
                lines.push_str(&format!("\n{preview}"));
            }
            if let Some(ref preview) = shot.scene_preview {
                lines.push_str(&format!("\n{preview}"));
            }
            if !shot.notes.is_empty() {
                lines.push_str("\n\nNotes:");
                for note in &shot.notes {
                    lines.push_str(&format!("\n  - {}", note.text));
                }
            }
            lines
        }
        None => String::from("Select a shot to view its transcript"),
    };

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let mode_label = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Command => "COMMAND",
        Mode::Prompt => "PROMPT",
        Mode::Search => "SEARCH",
    };

    let edit_label = app
        .edit
        .as_ref()
        .map(|e| e.name.as_str())
        .unwrap_or("(none)");

    let shot_info = match app.selected_shot.selected() {
        Some(i) if !app.resolved_shots.is_empty() => {
            format!("  shot {}/{}", i + 1, app.resolved_shots.len())
        }
        _ => String::new(),
    };

    let status = format!(
        " [{mode_label}]  edit: {edit_label}{shot_info}  | {}",
        app.status_message
    );

    let bar = Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(bar, area);
}
