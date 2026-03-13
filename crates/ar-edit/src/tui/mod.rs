mod events;
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
use ratatui::widgets::ListState;

use ar_edit_core::display::{self, ResolvedShot};
use ar_edit_core::models::{EditDocument, ShotRange, Source};
use ar_edit_core::playback;
use ar_edit_core::search::{SearchResult, TypeFilter};

use panels::status::RenderProgress;

// ---------------------------------------------------------------------------
// App mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Command,
    Prompt,
    Search,
    SearchResults,
    PoiCategory,
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
    pub render_progress: Option<RenderProgress>,
    pub transcript_scroll: panels::transcript::TranscriptScroll,
    pub search_results: Vec<SearchResult>,
    pub search_selected: ListState,
    pub search_query: String,
    pub search_type_filter: Option<TypeFilter>,
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
            render_progress: None,
            transcript_scroll: panels::transcript::TranscriptScroll::default(),
            search_results: Vec::new(),
            search_selected: ListState::default(),
            search_query: String::new(),
            search_type_filter: None,
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
                .find(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            {
                let path = entry.path();
                let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
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

    /// Reload the edit document from disk (called when the watcher detects a
    /// change).  Preserves the current shot selection when possible.
    pub(crate) fn reload_edit(&mut self) {
        let path = match self.edit_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        match EditDocument::load(&path) {
            Ok(doc) => {
                self.status_message = format!("Reloaded: {}", doc.name);
                self.edit = Some(doc);
                self.resolve_shots();
                // Clamp selection to the (possibly changed) shot count.
                let count = self.shot_count();
                if count > 0 {
                    let idx = self.selected_shot.selected().unwrap_or(0);
                    self.selected_shot.select(Some(idx.min(count - 1)));
                }
            }
            Err(e) => {
                self.status_message = format!("Reload failed: {e}");
            }
        }
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

    // Start watching the edit document for external changes.
    let mut watcher = app.edit_path.as_deref().and_then(events::FileWatcher::new);

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

        // Check for external file changes and reload if needed.
        if let Some(w) = watcher.as_mut() {
            if w.poll() {
                app.reload_edit();
            }
        }

        // Handle pending play: leave TUI, launch player, re-enter TUI.
        if let Some(mut req) = app.pending_play.take() {
            match playback::detect_player() {
                Ok(player) => {
                    // For mpv, set up IPC socket for position capture
                    let ipc_path = if player.kind == playback::PlayerKind::Mpv {
                        let path = std::env::temp_dir().join(format!(
                            "ar-edit-mpv-{}.sock",
                            std::process::id()
                        ));
                        req.ipc_socket = Some(path.clone());
                        Some(path)
                    } else {
                        None
                    };

                    restore_terminal(terminal)?;

                    match playback::launch_player(&player, &req) {
                        Ok(mut child) => {
                            if let Some(ref src_id) = req.source_id {
                                let pois = monitor_playback_with_poi(
                                    &mut child,
                                    ipc_path.as_deref(),
                                    src_id,
                                    &app.project_dir,
                                    req.start_ms,
                                );
                                if !pois.is_empty() {
                                    app.status_message = format!(
                                        "Created {} POI(s) during playback",
                                        pois.len(),
                                    );
                                }
                            } else {
                                let _ = child.wait();
                            }
                        }
                        Err(e) => {
                            app.status_message = format!("Play failed: {e}");
                        }
                    }

                    // Clean up IPC socket
                    if let Some(ref path) = ipc_path {
                        let _ = std::fs::remove_file(path);
                    }

                    *terminal = setup_terminal()?;
                }
                Err(_) => {
                    app.status_message =
                        "No video player found (install mpv, VLC, or ffplay)".into();
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

// ---------------------------------------------------------------------------
// Interactive playback with POI creation (REQ-059)
// ---------------------------------------------------------------------------

/// Monitor mpv playback, capturing POI keypresses.
///
/// While the player runs, we put the terminal in raw mode (no alternate
/// screen) to capture single keypresses. Press `i` to drop a POI at the
/// current playback position, `q` to quit the player.
///
/// When an mpv IPC socket is available, position is read precisely via IPC.
/// For other players (VLC, ffplay) or when IPC is unavailable, position is
/// estimated as `start_ms + wall-clock elapsed time`.
///
/// Returns the IDs of any POIs created.
fn monitor_playback_with_poi(
    child: &mut std::process::Child,
    ipc_socket: Option<&std::path::Path>,
    source_id: &str,
    project_dir: &std::path::Path,
    start_ms: u64,
) -> Vec<String> {
    use ar_edit_core::display::format_time;
    use ar_edit_core::models::{PoiCategory, PoiPoint, Transcript};
    use ar_edit_core::resolve::find_nearest_word;
    use crossterm::event::{self, Event, KeyCode, KeyEvent};

    let mut pois_created = Vec::new();
    let playback_start = Instant::now();

    // Load transcript once (best-effort)
    let transcript: Option<Transcript> = {
        let path = project_dir
            .join("transcripts")
            .join(format!("{source_id}.transcript.json"));
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
    };

    // Print instructions
    eprintln!();
    eprintln!("  \x1b[1;36m\u{25b6} Playing {source_id}\x1b[0m");
    eprintln!("  \x1b[33mi\x1b[0m mark POI   \x1b[33mq\x1b[0m quit player");
    eprintln!();

    // Wait briefly for mpv IPC socket to become available
    if let Some(sock) = ipc_socket {
        for _ in 0..10 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Enable raw mode to capture single keypresses
    let raw_mode = enable_raw_mode().is_ok();

    loop {
        // Check if player has exited
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }

        // Poll for keypresses (100ms timeout)
        if !event::poll(Duration::from_millis(100)).unwrap_or(false) {
            continue;
        }

        let Ok(Event::Key(KeyEvent { code, .. })) = event::read() else {
            continue;
        };

        match code {
            KeyCode::Char('q') => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            KeyCode::Char('i') => {
                // Get current playback position: try IPC first, fall back to wall-clock
                let timestamp_ms = ipc_socket
                    .and_then(playback::mpv_get_position)
                    .unwrap_or_else(|| {
                        start_ms + playback_start.elapsed().as_millis() as u64
                    });

                // Show category prompt
                eprint!(
                    "  \x1b[1mCategory:\x1b[0m \x1b[33mh\x1b[0m=highlight \
                     \x1b[33mi\x1b[0m=issue \x1b[33mt\x1b[0m=transition \
                     \x1b[33mc\x1b[0m=cue \x1b[33mn\x1b[0m=note \
                     \x1b[2m(Esc=cancel)\x1b[0m "
                );

                // Wait for category keypress
                let category = loop {
                    if let Ok(Event::Key(KeyEvent { code: cat, .. })) = event::read() {
                        match cat {
                            KeyCode::Char('h') => break Some(PoiCategory::Highlight),
                            KeyCode::Char('i') => break Some(PoiCategory::Issue),
                            KeyCode::Char('t') => break Some(PoiCategory::Transition),
                            KeyCode::Char('c') => break Some(PoiCategory::Cue),
                            KeyCode::Char('n') => break Some(PoiCategory::Note),
                            KeyCode::Esc => break None,
                            _ => {}
                        }
                    }
                };

                let Some(category) = category else {
                    eprintln!("\x1b[2mcancelled\x1b[0m");
                    continue;
                };

                // Resolve timestamp to word index if transcript available
                let point = match &transcript {
                    Some(t) => match find_nearest_word(timestamp_ms, t) {
                        Some(word_idx) => PoiPoint::Word(word_idx),
                        None => PoiPoint::TimeMs(timestamp_ms),
                    },
                    None => PoiPoint::TimeMs(timestamp_ms),
                };

                // Create the POI
                match ar_edit_core::poi::add_poi(
                    project_dir,
                    source_id,
                    point.clone(),
                    category.clone(),
                    None,
                ) {
                    Ok(poi) => {
                        let word_info = match &point {
                            PoiPoint::Word(idx) => format!(" (word {idx})"),
                            PoiPoint::Scene(idx) => format!(" (scene {idx})"),
                            PoiPoint::TimeMs(_) => String::new(),
                        };
                        eprintln!(
                            "  \x1b[1;32m\u{2713} {}\x1b[0m [{category}] @ {}{word_info}",
                            poi.id,
                            format_time(timestamp_ms),
                        );
                        pois_created.push(poi.id);
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m\u{2717} POI failed: {e}\x1b[0m");
                    }
                }
            }
            _ => {}
        }
    }

    // Restore terminal state
    if raw_mode {
        let _ = disable_raw_mode();
    }

    pois_created
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

    // --- Right panel: search results, transcript, or source detail ---
    //
    // Extract the selected shot index first to avoid overlapping borrows
    // between `resolved_shots` (immutable) and `transcript_scroll` (mutable).
    let selected_idx = app.selected_shot.selected();
    if app.mode == Mode::SearchResults {
        // REQ-045: show search results in the right panel
        panels::search_results::draw(
            f,
            &app.search_results,
            &mut app.search_selected,
            &app.search_query,
            app.search_type_filter.as_ref(),
            transcript_area,
        );
    } else {
        match app.focus {
            Focus::Sources => {
                if let Some(source) = app.selected_source().cloned() {
                    panels::sources::draw_detail(f, &source, transcript_area);
                } else {
                    let shot = selected_idx.and_then(|i| app.resolved_shots.get(i));
                    panels::transcript::draw(
                        f,
                        shot,
                        &app.project_dir,
                        &mut app.transcript_scroll,
                        transcript_area,
                    );
                }
            }
            Focus::Timeline => {
                let shot = selected_idx.and_then(|i| app.resolved_shots.get(i));
                panels::transcript::draw(
                    f,
                    shot,
                    &app.project_dir,
                    &mut app.transcript_scroll,
                    transcript_area,
                );
            }
        }
    }

    // --- Sources panel (REQ-041) ---
    panels::sources::draw(f, &app.sources, &mut app.selected_source, sources_area);

    // --- Status bar (REQ-044) ---
    let edit_name = app.edit.as_ref().map(|e| e.name.as_str());
    panels::status::draw(
        f,
        app.mode,
        edit_name,
        selected_idx,
        &app.resolved_shots,
        app.render_progress.as_ref(),
        &app.status_message,
        status_area,
    );
}
