use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use ar_edit_core::models::{EditDocument, Shot, Source};

// ---------------------------------------------------------------------------
// App mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Command,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct App {
    pub edit: Option<EditDocument>,
    pub sources: Vec<Source>,
    pub selected_shot: ListState,
    pub mode: Mode,
    pub project_dir: PathBuf,
    pub status_message: String,
    pub should_quit: bool,
}

impl App {
    pub fn new(project_dir: PathBuf) -> Self {
        let mut selected_shot = ListState::default();
        selected_shot.select(Some(0));

        Self {
            edit: None,
            sources: Vec::new(),
            selected_shot,
            mode: Mode::Normal,
            project_dir,
            status_message: String::from("Press q to quit, j/k to navigate"),
            should_quit: false,
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
                let doc =
                    EditDocument::load(&entry.path()).map_err(|e| anyhow::anyhow!("{e}"))?;
                self.status_message = format!("Loaded edit: {}", doc.name);
                self.edit = Some(doc);
            }
        }

        Ok(())
    }

    fn shot_count(&self) -> usize {
        self.edit
            .as_ref()
            .map(|e| e.snapshot.shots.len())
            .unwrap_or(0)
    }

    fn select_next(&mut self) {
        let count = self.shot_count();
        if count == 0 {
            return;
        }
        let i = self.selected_shot.selected().unwrap_or(0);
        self.selected_shot.select(Some((i + 1).min(count - 1)));
    }

    fn select_previous(&mut self) {
        let i = self.selected_shot.selected().unwrap_or(0);
        self.selected_shot.select(Some(i.saturating_sub(1)));
    }

    fn selected_shot(&self) -> Option<&Shot> {
        let idx = self.selected_shot.selected()?;
        self.edit.as_ref()?.snapshot.shots.get(idx)
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
                handle_key(app, key);
            }
        }

        if last_tick.elapsed() >= TICK_RATE {
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // Ctrl-C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match app.mode {
        Mode::Normal => match key.code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => app.select_next(),
            KeyCode::Char('k') | KeyCode::Up => app.select_previous(),
            KeyCode::Char(':') => {
                app.mode = Mode::Command;
                app.status_message = String::from(":");
            }
            _ => {}
        },
        Mode::Command => match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.status_message = String::from("Press q to quit, j/k to navigate");
            }
            KeyCode::Enter => {
                app.mode = Mode::Normal;
                app.status_message = String::from("Press q to quit, j/k to navigate");
            }
            _ => {}
        },
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
    draw_timeline(f, app, timeline_area);

    // --- Transcript panel (REQ-040) ---
    draw_transcript(f, app, transcript_area);

    // --- Sources panel (REQ-041) ---
    draw_sources(f, app, sources_area);

    // --- Status bar ---
    draw_status(f, app, status_area);
}

// ---------------------------------------------------------------------------
// Panel renderers
// ---------------------------------------------------------------------------

fn draw_timeline(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(" Timeline ")
        .borders(Borders::ALL);

    let items: Vec<ListItem> = match &app.edit {
        Some(doc) => doc
            .snapshot
            .shots
            .iter()
            .map(|shot| {
                let range_str = match &shot.range {
                    ar_edit_core::models::ShotRange::Words { from, to } => {
                        format!("words {from}..{to}")
                    }
                    ar_edit_core::models::ShotRange::Scenes { from, to } => {
                        format!("scenes {from}..{to}")
                    }
                    ar_edit_core::models::ShotRange::Time { from_ms, to_ms } => {
                        format!("{from_ms}ms..{to_ms}ms")
                    }
                };
                ListItem::new(format!("{} [{}] {}", shot.id, shot.source, range_str))
            })
            .collect(),
        None => vec![ListItem::new("(no edit loaded)")],
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, area, &mut app.selected_shot);
}

fn draw_transcript(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Transcript ")
        .borders(Borders::ALL);

    let text = match app.selected_shot() {
        Some(shot) => format!(
            "Shot: {}\nSource: {}\n\n(transcript content will appear here)",
            shot.id, shot.source
        ),
        None => String::from("Select a shot to view its transcript"),
    };

    let paragraph = Paragraph::new(text).block(block).wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn draw_sources(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Sources ")
        .borders(Borders::ALL);

    let items: Vec<ListItem> = if app.sources.is_empty() {
        vec![ListItem::new("(no sources)")]
    } else {
        app.sources
            .iter()
            .map(|s| {
                ListItem::new(format!(
                    "{} {} ({}x{})",
                    s.id, s.original_filename, s.resolution.0, s.resolution.1
                ))
            })
            .collect()
    };

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let mode_label = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Command => "COMMAND",
    };

    let edit_label = app
        .edit
        .as_ref()
        .map(|e| e.name.as_str())
        .unwrap_or("(none)");

    let status = format!(
        " [{mode_label}]  edit: {edit_label}  | {}",
        app.status_message
    );

    let bar = Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(bar, area);
}
