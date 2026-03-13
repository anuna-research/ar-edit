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
                    // Set up IPC socket for position capture (mpv and VLC)
                    let pid = std::process::id();
                    let ipc_path = match player.kind {
                        playback::PlayerKind::Mpv => {
                            let path = std::env::temp_dir()
                                .join(format!("ar-edit-mpv-{pid}.sock"));
                            req.ipc_socket = Some(path.clone());
                            Some(path)
                        }
                        playback::PlayerKind::Vlc => {
                            let path = std::env::temp_dir()
                                .join(format!("ar-edit-vlc-{pid}.sock"));
                            req.ipc_socket = Some(path.clone());
                            Some(path)
                        }
                        playback::PlayerKind::Ffplay => None,
                    };

                    // For mpv with POI mode: set up Lua script for in-player capture
                    let marker_file = if player.kind == playback::PlayerKind::Mpv
                        && req.source_id.is_some()
                    {
                        let marker = std::env::temp_dir()
                            .join(format!("ar-edit-poi-{pid}.txt"));
                        let script_path = std::env::temp_dir()
                            .join(format!("ar-edit-poi-{pid}.lua"));
                        let lua = playback::generate_mpv_poi_script(&marker);
                        let _ = std::fs::write(&script_path, lua);
                        req.mpv_script = Some(script_path);
                        req.marker_file = Some(marker.clone());
                        Some(marker)
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
                                    player.kind,
                                    marker_file.as_deref(),
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

                    // Clean up temp files (IPC socket, Lua script, marker file)
                    if let Some(ref path) = ipc_path {
                        let _ = std::fs::remove_file(path);
                    }
                    if let Some(ref path) = req.mpv_script {
                        let _ = std::fs::remove_file(path);
                    }
                    if let Some(ref path) = marker_file {
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

/// Monitor playback, capturing POI marks from in-player keys or terminal.
///
/// For **mpv**: A Lua script handles `i` → category selection entirely within
/// the mpv window (OSD prompts, no terminal focus needed). The script writes
/// `<timestamp_ms> <category>` lines to a marker file, which this function
/// polls and converts into POIs.
///
/// For **VLC / ffplay**: Falls back to terminal raw-mode capture (requires
/// terminal focus). VLC uses RC socket for precise position; ffplay uses
/// wall-clock estimation.
///
/// Terminal `q` kills the player in all modes.
///
/// Returns the IDs of any POIs created.
fn monitor_playback_with_poi(
    child: &mut std::process::Child,
    ipc_socket: Option<&std::path::Path>,
    player_kind: playback::PlayerKind,
    marker_file: Option<&std::path::Path>,
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
    let has_marker_file = marker_file.is_some();
    // Track how many marker lines we've already processed
    let mut marker_lines_read: usize = 0;

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
    if has_marker_file {
        eprintln!("  Press \x1b[33mi\x1b[0m in the player window to mark a POI");
    } else {
        eprintln!("  \x1b[33mi\x1b[0m mark POI (in terminal)   \x1b[33mq\x1b[0m quit player");
    }
    eprintln!();

    // Wait briefly for IPC socket to become available
    if let Some(sock) = ipc_socket {
        for _ in 0..10 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Enable raw mode to capture single keypresses (q to quit, i for non-mpv)
    let raw_mode = enable_raw_mode().is_ok();

    // Helper closure to create a POI from timestamp + category
    let create_poi =
        |timestamp_ms: u64,
         category: PoiCategory,
         transcript: &Option<Transcript>,
         pois: &mut Vec<String>| {
            let point = match transcript {
                Some(t) => match find_nearest_word(timestamp_ms, t) {
                    Some(word_idx) => PoiPoint::Word(word_idx),
                    None => PoiPoint::TimeMs(timestamp_ms),
                },
                None => PoiPoint::TimeMs(timestamp_ms),
            };

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
                    pois.push(poi.id);
                }
                Err(e) => {
                    eprintln!("  \x1b[31m\u{2717} POI failed: {e}\x1b[0m");
                }
            }
        };

    loop {
        // Check if player has exited
        match child.try_wait() {
            Ok(Some(_)) => {
                // Process any remaining markers before exiting
                if let Some(mf) = marker_file {
                    process_marker_file(
                        mf,
                        &mut marker_lines_read,
                        &transcript,
                        &mut pois_created,
                        &create_poi,
                    );
                }
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }

        // Check marker file for new POI marks from mpv Lua script
        if let Some(mf) = marker_file {
            process_marker_file(
                mf,
                &mut marker_lines_read,
                &transcript,
                &mut pois_created,
                &create_poi,
            );
        }

        // Poll for terminal keypresses (100ms timeout)
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
                // Process any remaining markers
                if let Some(mf) = marker_file {
                    process_marker_file(
                        mf,
                        &mut marker_lines_read,
                        &transcript,
                        &mut pois_created,
                        &create_poi,
                    );
                }
                break;
            }
            // Terminal-based POI capture (VLC/ffplay fallback)
            KeyCode::Char('i') if !has_marker_file => {
                let timestamp_ms = ipc_socket
                    .and_then(|sock| playback::get_player_position(sock, player_kind))
                    .unwrap_or_else(|| {
                        start_ms + playback_start.elapsed().as_millis() as u64
                    });

                eprint!(
                    "  \x1b[1mCategory:\x1b[0m \x1b[33mh\x1b[0m=highlight \
                     \x1b[33mi\x1b[0m=issue \x1b[33mt\x1b[0m=transition \
                     \x1b[33mc\x1b[0m=cue \x1b[33mn\x1b[0m=note \
                     \x1b[2m(Esc=cancel)\x1b[0m "
                );

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

                create_poi(timestamp_ms, category, &transcript, &mut pois_created);
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

/// Read new lines from the marker file written by mpv's Lua script.
///
/// Each line has format: `<timestamp_ms> <category_name>`
fn process_marker_file(
    path: &std::path::Path,
    lines_read: &mut usize,
    transcript: &Option<ar_edit_core::models::Transcript>,
    pois: &mut Vec<String>,
    create_poi: &dyn Fn(u64, ar_edit_core::models::PoiCategory, &Option<ar_edit_core::models::Transcript>, &mut Vec<String>),
) {
    use ar_edit_core::models::PoiCategory;

    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= *lines_read {
        return;
    }

    for line in &lines[*lines_read..] {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() != 2 {
            continue;
        }
        let Ok(timestamp_ms) = parts[0].parse::<u64>() else {
            continue;
        };
        let category = match parts[1] {
            "highlight" => PoiCategory::Highlight,
            "issue" => PoiCategory::Issue,
            "transition" => PoiCategory::Transition,
            "cue" => PoiCategory::Cue,
            "note" => PoiCategory::Note,
            _ => continue,
        };
        create_poi(timestamp_ms, category, transcript, pois);
    }

    *lines_read = lines.len();
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
