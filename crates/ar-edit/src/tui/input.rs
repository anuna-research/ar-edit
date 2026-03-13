use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use ar_edit_core::models::{PoiCategory as PoiCat, PoiPoint, ShotRange};
use ar_edit_core::playback;
use ar_edit_core::search::{self, TypeFilter};

use super::{App, Focus, Mode, PromptAction};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Process a key event and update the app state.
pub fn handle_key(app: &mut App, key: KeyEvent) {
    // Ctrl-C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match app.mode {
        Mode::Normal => handle_normal(app, key),
        Mode::Command => handle_command(app, key),
        Mode::Prompt => handle_prompt(app, key),
        Mode::Search => handle_search(app, key),
        Mode::SearchResults => handle_search_results(app, key),
        Mode::PoiCategory => handle_poi_category(app, key),
    }
}

// ---------------------------------------------------------------------------
// Normal mode (REQ-043)
// ---------------------------------------------------------------------------

fn handle_normal(app: &mut App, key: KeyEvent) {
    // Ctrl shortcuts
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('z') => do_undo(app),
            KeyCode::Char('y') => do_redo(app),
            _ => {}
        }
        return;
    }

    // Tab toggles focus between panels
    if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
        app.focus = match app.focus {
            Focus::Timeline => Focus::Sources,
            Focus::Sources => Focus::Timeline,
        };
        return;
    }

    match app.focus {
        Focus::Timeline => handle_normal_timeline(app, key),
        Focus::Sources => handle_normal_sources(app, key),
    }
}

/// Normal-mode keys when the timeline panel is focused.
fn handle_normal_timeline(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_previous(),
        KeyCode::Char('{') => super::panels::transcript::scroll_up(&mut app.transcript_scroll),
        KeyCode::Char('}') => {
            super::panels::transcript::scroll_down(&mut app.transcript_scroll, u16::MAX)
        }
        KeyCode::Char('J') => do_move_shot_down(app),
        KeyCode::Char('K') => do_move_shot_up(app),
        KeyCode::Char('d') => do_delete_shot(app),
        KeyCode::Char('a') => {
            start_prompt(app, "source ID:", PromptAction::AddShotSource);
        }
        KeyCode::Char('t') => {
            start_prompt(
                app,
                "new range (words/scenes/time <from> <to>):",
                PromptAction::TrimShot,
            );
        }
        KeyCode::Char('n') => {
            start_prompt(app, "note:", PromptAction::NoteInput);
        }
        KeyCode::Char('m') => {
            start_prompt(app, "marker source ID:", PromptAction::MarkerSource);
        }
        KeyCode::Char('i') => do_start_poi(app),
        KeyCode::Char('p') | KeyCode::Enter => do_play(app),
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.prompt_buffer.clear();
            app.status_message = String::from("/");
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.status_message = String::from(":");
        }
        _ => {}
    }
}

/// Normal-mode keys when the sources panel is focused.
fn handle_normal_sources(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.select_next_source(),
        KeyCode::Char('k') | KeyCode::Up => app.select_previous_source(),
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.prompt_buffer.clear();
            app.status_message = String::from("/");
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.status_message = String::from(":");
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Command mode
// ---------------------------------------------------------------------------

fn handle_command(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.status_message = default_status();
        }
        KeyCode::Enter => {
            app.mode = Mode::Normal;
            app.status_message = default_status();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Prompt mode — multi-step text input
// ---------------------------------------------------------------------------

fn handle_prompt(app: &mut App, key: KeyEvent) {
    // Ignore ctrl combos in text-input modes
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.prompt_action = None;
            app.prompt_buffer.clear();
            app.status_message = default_status();
        }
        KeyCode::Enter => {
            let buffer = app.prompt_buffer.clone();
            let action = app.prompt_action.take();
            app.prompt_buffer.clear();

            if let Some(action) = action {
                submit_prompt(app, action, &buffer);
            } else {
                app.mode = Mode::Normal;
                app.status_message = default_status();
            }
        }
        KeyCode::Backspace => {
            app.prompt_buffer.pop();
            update_prompt_display(app);
        }
        KeyCode::Char(c) => {
            app.prompt_buffer.push(c);
            update_prompt_display(app);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Search mode
// ---------------------------------------------------------------------------

fn handle_search(app: &mut App, key: KeyEvent) {
    // Ignore ctrl combos in text-input modes
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.prompt_buffer.clear();
            app.status_message = default_status();
        }
        KeyCode::Enter => {
            let query = app.prompt_buffer.clone();
            app.mode = Mode::Normal;
            app.prompt_buffer.clear();
            do_search(app, &query);
        }
        KeyCode::Backspace => {
            app.prompt_buffer.pop();
            app.status_message = format!("/{}", app.prompt_buffer);
        }
        KeyCode::Char(c) => {
            app.prompt_buffer.push(c);
            app.status_message = format!("/{}", app.prompt_buffer);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Prompt flow — chaining multi-step prompts
// ---------------------------------------------------------------------------

fn start_prompt(app: &mut App, label: &str, action: PromptAction) {
    app.mode = Mode::Prompt;
    app.prompt_label = label.to_string();
    app.prompt_buffer.clear();
    app.prompt_action = Some(action);
    app.status_message = format!("{label} ");
}

fn update_prompt_display(app: &mut App) {
    app.status_message = format!("{} {}", app.prompt_label, app.prompt_buffer);
}

fn submit_prompt(app: &mut App, action: PromptAction, input: &str) {
    let input = input.trim();
    if input.is_empty() {
        app.mode = Mode::Normal;
        app.status_message = "Cancelled (empty input)".into();
        return;
    }

    // Default to Normal; start_prompt will override when chaining.
    app.mode = Mode::Normal;

    match action {
        PromptAction::AddShotSource => {
            start_prompt(
                app,
                "range (words/scenes/time <from> <to>):",
                PromptAction::AddShotRange {
                    source: input.to_string(),
                },
            );
        }
        PromptAction::AddShotRange { source } => match parse_range_input(input) {
            Ok(range) => do_add_shot(app, &source, range),
            Err(msg) => app.status_message = format!("Invalid range: {msg}"),
        },
        PromptAction::TrimShot => match parse_range_input(input) {
            Ok(range) => do_trim_shot(app, range),
            Err(msg) => app.status_message = format!("Invalid range: {msg}"),
        },
        PromptAction::NoteInput => {
            do_add_note(app, input);
        }
        PromptAction::MarkerSource => {
            start_prompt(
                app,
                "marker range (words/scenes/time <from> <to>):",
                PromptAction::MarkerRange {
                    source: input.to_string(),
                },
            );
        }
        PromptAction::MarkerRange { source } => match parse_range_input(input) {
            Ok(range) => {
                start_prompt(
                    app,
                    "marker label:",
                    PromptAction::MarkerLabel { source, range },
                );
            }
            Err(msg) => app.status_message = format!("Invalid range: {msg}"),
        },
        PromptAction::MarkerLabel { source, range } => {
            do_add_marker(app, &source, range, input);
        }
    }
}

// ---------------------------------------------------------------------------
// Actions — each mutating op saves to disk immediately
// ---------------------------------------------------------------------------

fn do_add_shot(app: &mut App, source: &str, range: ShotRange) {
    let id = {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        match doc.add_shot(source, range) {
            Ok(shot) => shot.id.clone(),
            Err(e) => {
                app.status_message = format!("Add failed: {e}");
                return;
            }
        }
    };

    if !save_and_refresh(app) {
        return;
    }

    let count = app.shot_count();
    if count > 0 {
        app.selected_shot.select(Some(count - 1));
    }
    app.status_message = format!("Added {id}");
}

fn do_delete_shot(app: &mut App) {
    let shot_id = match app.selected_shot_id() {
        Some(id) => id,
        None => {
            app.status_message = "No shot selected".into();
            return;
        }
    };
    let idx = app.selected_shot.selected().unwrap_or(0);

    {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        if let Err(e) = doc.remove_shot(&shot_id) {
            app.status_message = format!("Delete failed: {e}");
            return;
        }
    }

    if !save_and_refresh(app) {
        return;
    }

    let count = app.shot_count();
    if count > 0 {
        app.selected_shot.select(Some(idx.min(count - 1)));
    }
    app.status_message = format!("Deleted {shot_id}");
}

fn do_move_shot_down(app: &mut App) {
    let idx = app.selected_shot.selected().unwrap_or(0);
    let count = app.shot_count();
    if count < 2 || idx >= count - 1 {
        return;
    }

    let shot_id = match app.selected_shot_id() {
        Some(id) => id,
        None => return,
    };

    {
        let Some(doc) = app.edit.as_mut() else { return };
        if doc.move_shot(&shot_id, idx + 1).is_err() {
            return;
        }
    }

    if !save_and_refresh(app) {
        return;
    }

    app.selected_shot.select(Some(idx + 1));
    app.status_message = format!("Moved {shot_id} down");
}

fn do_move_shot_up(app: &mut App) {
    let idx = app.selected_shot.selected().unwrap_or(0);
    if idx == 0 {
        return;
    }

    let shot_id = match app.selected_shot_id() {
        Some(id) => id,
        None => return,
    };

    {
        let Some(doc) = app.edit.as_mut() else { return };
        if doc.move_shot(&shot_id, idx - 1).is_err() {
            return;
        }
    }

    if !save_and_refresh(app) {
        return;
    }

    app.selected_shot.select(Some(idx - 1));
    app.status_message = format!("Moved {shot_id} up");
}

fn do_trim_shot(app: &mut App, new_range: ShotRange) {
    let shot_id = match app.selected_shot_id() {
        Some(id) => id,
        None => {
            app.status_message = "No shot selected".into();
            return;
        }
    };

    {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        if let Err(e) = doc.trim_shot(&shot_id, new_range) {
            app.status_message = format!("Trim failed: {e}");
            return;
        }
    }

    if !save_and_refresh(app) {
        return;
    }

    app.status_message = format!("Trimmed {shot_id}");
}

fn do_add_note(app: &mut App, text: &str) {
    let shot_id = match app.selected_shot_id() {
        Some(id) => id,
        None => {
            app.status_message = "No shot selected".into();
            return;
        }
    };

    {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        if let Err(e) = doc.add_note(&shot_id, text) {
            app.status_message = format!("Note failed: {e}");
            return;
        }
    }

    if !save_and_refresh(app) {
        return;
    }

    app.status_message = format!("Added note to {shot_id}");
}

fn do_undo(app: &mut App) {
    let info = {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        match doc.undo() {
            Ok(op) => (op.id, doc.head),
            Err(e) => {
                app.status_message = format!("{e}");
                return;
            }
        }
    };

    if !save_and_refresh(app) {
        return;
    }

    app.status_message = format!("Undo #{} (head \u{2192} {})", info.0, info.1);
}

fn do_redo(app: &mut App) {
    let info = {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        match doc.redo() {
            Ok(op) => (op.id, doc.head),
            Err(e) => {
                app.status_message = format!("{e}");
                return;
            }
        }
    };

    if !save_and_refresh(app) {
        return;
    }

    app.status_message = format!("Redo #{} (head \u{2192} {})", info.0, info.1);
}

fn do_play(app: &mut App) {
    let Some(shot) = app.selected_resolved_shot() else {
        app.status_message = "No shot selected".into();
        return;
    };

    let source_id = shot.source.clone();
    let shot_id = shot.id.clone();
    let start_ms = shot.start_ms;
    let end_ms = if shot.duration_ms > 0 {
        Some(shot.end_ms)
    } else {
        None
    };

    match playback::resolve_source_path(&source_id, &app.project_dir) {
        Ok((file, _)) => {
            app.pending_play = Some(playback::PlayRequest {
                file,
                start_ms,
                end_ms,
                ipc_socket: None,
                source_id: Some(source_id.clone()),
            });
            app.status_message = format!("Playing {shot_id}...");
        }
        Err(e) => {
            app.status_message = format!("Play failed: {e}");
        }
    }
}

fn do_add_marker(app: &mut App, source: &str, range: ShotRange, label: &str) {
    match ar_edit_core::marker::add_marker(&app.project_dir, source, range, label, None) {
        Ok(marker) => {
            app.status_message = format!("Created {} on {}", marker.id, source);
        }
        Err(e) => {
            app.status_message = format!("Marker failed: {e}");
        }
    }
}

fn do_start_poi(app: &mut App) {
    if app.selected_resolved_shot().is_none() {
        app.status_message = "No shot selected".into();
        return;
    }
    app.mode = Mode::PoiCategory;
    app.status_message =
        "POI category: h=highlight i=issue t=transition c=cue n=note".into();
}

fn get_poi_context(app: &App) -> Option<(String, PoiPoint)> {
    let shot = app.selected_resolved_shot()?;
    let source = shot.source.clone();
    let point = match &shot.range {
        ShotRange::Words { from, .. } => PoiPoint::Word(*from),
        ShotRange::Scenes { from, .. } => PoiPoint::Scene(*from),
        ShotRange::Time { from_ms, .. } => PoiPoint::TimeMs(*from_ms),
    };
    Some((source, point))
}

fn do_create_poi(app: &mut App, source: &str, point: PoiPoint, category: PoiCat) {
    match ar_edit_core::poi::add_poi(&app.project_dir, source, point, category, None) {
        Ok(poi) => {
            app.status_message =
                format!("Created {} [{}] on {}", poi.id, poi.category, source);
        }
        Err(e) => {
            app.status_message = format!("POI failed: {e}");
        }
    }
}

fn handle_poi_category(app: &mut App, key: KeyEvent) {
    let category = match key.code {
        KeyCode::Char('h') => Some(PoiCat::Highlight),
        KeyCode::Char('i') => Some(PoiCat::Issue),
        KeyCode::Char('t') => Some(PoiCat::Transition),
        KeyCode::Char('c') => Some(PoiCat::Cue),
        KeyCode::Char('n') => Some(PoiCat::Note),
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.status_message = default_status();
            return;
        }
        _ => None,
    };

    if let Some(cat) = category {
        if let Some((source, point)) = get_poi_context(app) {
            do_create_poi(app, &source, point, cat);
        } else {
            app.status_message = "No word position available for POI".into();
        }
        app.mode = Mode::Normal;
    }
}

fn do_search(app: &mut App, query: &str) {
    if query.is_empty() {
        app.status_message = default_status();
        return;
    }

    match search::search(
        &app.project_dir,
        query,
        None,
        app.search_type_filter.as_ref(),
    ) {
        Ok(results) => {
            let count = results.len();
            app.search_query = query.to_string();
            app.search_results = results;
            app.search_selected = ratatui::widgets::ListState::default();
            if count > 0 {
                app.search_selected.select(Some(0));
            }
            app.mode = Mode::SearchResults;
            app.status_message = format!(
                "{count} result{} \u{2014} j/k:nav Enter:jump a:add-shot Tab:filter Esc:close",
                if count == 1 { "" } else { "s" },
            );
        }
        Err(e) => {
            app.mode = Mode::Normal;
            app.status_message = format!("Search error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Search results mode (REQ-045)
// ---------------------------------------------------------------------------

fn handle_search_results(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = Mode::Normal;
            app.search_results.clear();
            app.status_message = default_status();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.search_results.len();
            if count > 0 {
                let i = app.search_selected.selected().unwrap_or(0);
                app.search_selected.select(Some((i + 1).min(count - 1)));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let i = app.search_selected.selected().unwrap_or(0);
            app.search_selected.select(Some(i.saturating_sub(1)));
        }
        KeyCode::Tab => {
            // Cycle type filter: all -> transcript -> scene -> metadata -> all
            app.search_type_filter = match app.search_type_filter {
                None => Some(TypeFilter::Transcript),
                Some(TypeFilter::Transcript) => Some(TypeFilter::Scene),
                Some(TypeFilter::Scene) => Some(TypeFilter::Metadata),
                Some(TypeFilter::Metadata) => None,
            };
            // Re-run search with new filter
            let query = app.search_query.clone();
            do_search(app, &query);
        }
        KeyCode::Enter => {
            do_jump_to_result(app);
        }
        KeyCode::Char('a') => {
            do_add_result_as_shot(app);
        }
        KeyCode::Char('/') => {
            // Start a new search
            app.mode = Mode::Search;
            app.prompt_buffer.clear();
            app.status_message = String::from("/");
        }
        _ => {}
    }
}

/// Jump to the selected search result: find the matching shot in the timeline
/// or scroll to the source's transcript.
fn do_jump_to_result(app: &mut App) {
    let result = match selected_search_result(app) {
        Some(r) => r.clone(),
        None => return,
    };

    // Try to find a shot in the timeline that covers this result's time range
    // and matches the source.
    let matching_shot = app.resolved_shots.iter().position(|shot| {
        shot.source == result.source_id
            && shot.start_ms <= result.start_ms
            && shot.end_ms >= result.end_ms
    });

    if let Some(idx) = matching_shot {
        app.selected_shot.select(Some(idx));
        app.mode = Mode::Normal;
        app.focus = Focus::Timeline;
        app.search_results.clear();
        app.status_message = format!(
            "Jumped to {} ({})",
            app.resolved_shots[idx].id, result.source_id,
        );
    } else {
        // No matching shot — stay in results but show info about the result.
        app.mode = Mode::Normal;
        app.search_results.clear();
        app.status_message = format!(
            "{} @ {} in {} \u{2014} press 'a' to add as shot",
            result.matched_text,
            ar_edit_core::display::format_time(result.start_ms),
            result.source_id,
        );
    }
}

/// Add the selected search result as a new shot in the edit timeline.
fn do_add_result_as_shot(app: &mut App) {
    let result = match selected_search_result(app) {
        Some(r) => r.clone(),
        None => return,
    };

    let range = ShotRange::Time {
        from_ms: result.start_ms,
        to_ms: result.end_ms,
    };

    let id = {
        let Some(doc) = app.edit.as_mut() else {
            app.status_message = "No edit loaded".into();
            return;
        };
        match doc.add_shot(&result.source_id, range) {
            Ok(shot) => shot.id.clone(),
            Err(e) => {
                app.status_message = format!("Add failed: {e}");
                return;
            }
        }
    };

    if !save_and_refresh(app) {
        return;
    }

    let count = app.shot_count();
    if count > 0 {
        app.selected_shot.select(Some(count - 1));
    }
    app.mode = Mode::Normal;
    app.search_results.clear();
    app.status_message = format!("Added {} from {} search result", id, result.source_id);
}

fn selected_search_result(app: &App) -> Option<&search::SearchResult> {
    let idx = app.search_selected.selected()?;
    app.search_results.get(idx)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Save the current edit document to disk and refresh resolved shots.
///
/// Returns `true` on success. On failure, sets `status_message` and returns
/// `false`.
fn save_and_refresh(app: &mut App) -> bool {
    if let (Some(doc), Some(path)) = (app.edit.as_ref(), app.edit_path.as_ref()) {
        if let Err(e) = doc.save(path) {
            app.status_message = format!("Save failed: {e}");
            return false;
        }
    }
    app.resolve_shots();
    true
}

/// Parse a range string: `words 0 52`, `scenes 0 2`, or `time 5000 10000`.
///
/// Accepts short aliases `w`, `s`, `t` for the range type.
fn parse_range_input(input: &str) -> Result<ShotRange, String> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.len() != 3 {
        return Err("expected: <type> <from> <to> (e.g. words 0 52)".into());
    }

    match parts[0] {
        "words" | "w" => {
            let from: u32 = parts[1]
                .parse()
                .map_err(|_| "invalid 'from' number".to_string())?;
            let to: u32 = parts[2]
                .parse()
                .map_err(|_| "invalid 'to' number".to_string())?;
            Ok(ShotRange::Words { from, to })
        }
        "scenes" | "s" => {
            let from: u32 = parts[1]
                .parse()
                .map_err(|_| "invalid 'from' number".to_string())?;
            let to: u32 = parts[2]
                .parse()
                .map_err(|_| "invalid 'to' number".to_string())?;
            Ok(ShotRange::Scenes { from, to })
        }
        "time" | "t" => {
            let from_ms: u64 = parts[1]
                .parse()
                .map_err(|_| "invalid 'from_ms' number".to_string())?;
            let to_ms: u64 = parts[2]
                .parse()
                .map_err(|_| "invalid 'to_ms' number".to_string())?;
            Ok(ShotRange::Time { from_ms, to_ms })
        }
        other => Err(format!(
            "unknown range type: {other} (use words, scenes, or time)"
        )),
    }
}

/// Default status bar message showing available key bindings.
pub fn default_status() -> String {
    String::from("a:add d:del J/K:move t:trim p:play n:note m:mark i:poi /:search ^z/^y:undo/redo q:quit")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_words() {
        let range = parse_range_input("words 0 52").unwrap();
        assert_eq!(range, ShotRange::Words { from: 0, to: 52 });
    }

    #[test]
    fn parse_range_words_short() {
        let range = parse_range_input("w 10 200").unwrap();
        assert_eq!(range, ShotRange::Words { from: 10, to: 200 });
    }

    #[test]
    fn parse_range_scenes() {
        let range = parse_range_input("scenes 0 2").unwrap();
        assert_eq!(range, ShotRange::Scenes { from: 0, to: 2 });
    }

    #[test]
    fn parse_range_scenes_short() {
        let range = parse_range_input("s 1 5").unwrap();
        assert_eq!(range, ShotRange::Scenes { from: 1, to: 5 });
    }

    #[test]
    fn parse_range_time() {
        let range = parse_range_input("time 5000 10000").unwrap();
        assert_eq!(
            range,
            ShotRange::Time {
                from_ms: 5000,
                to_ms: 10000
            }
        );
    }

    #[test]
    fn parse_range_time_short() {
        let range = parse_range_input("t 0 3000").unwrap();
        assert_eq!(
            range,
            ShotRange::Time {
                from_ms: 0,
                to_ms: 3000
            }
        );
    }

    #[test]
    fn parse_range_invalid_type() {
        let err = parse_range_input("frames 0 100").unwrap_err();
        assert!(err.contains("unknown range type"));
    }

    #[test]
    fn parse_range_too_few_parts() {
        let err = parse_range_input("words 0").unwrap_err();
        assert!(err.contains("expected"));
    }

    #[test]
    fn parse_range_too_many_parts() {
        let err = parse_range_input("words 0 52 extra").unwrap_err();
        assert!(err.contains("expected"));
    }

    #[test]
    fn parse_range_invalid_number() {
        let err = parse_range_input("words abc 52").unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn default_status_contains_keys() {
        let status = default_status();
        assert!(status.contains("a:add"));
        assert!(status.contains("d:del"));
        assert!(status.contains("q:quit"));
        assert!(status.contains("^z"));
        assert!(status.contains("i:poi"));
    }
}
