mod cli;
#[cfg(feature = "tui")]
mod tui;

use std::path::{Path, PathBuf};
use std::process;

use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange, Source};
use ar_edit_core::playback;
use clap::Parser;
use cli::{
    exit_code, Cli, Commands, EditCommand, IndexCommand, PlayArgs, RangeArgs, SchemaCommand,
    SearchType, TranscriptsCommand,
};

use std::thread;

fn main() {
    let cli = Cli::parse();

    let code = match run(&cli) {
        Ok(()) => exit_code::SUCCESS,
        Err(e) => {
            if cli.json {
                let msg = serde_json::json!({ "error": format!("{e:#}") });
                eprintln!("{msg}");
            } else {
                eprintln!("Error: {e:#}");
            }
            exit_code::USER_ERROR
        }
    };

    process::exit(code);
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Init { name } => {
            todo!("init: {name}")
        }
        Commands::Add { files } => {
            todo!("add: {files:?}")
        }
        Commands::Doctor => {
            todo!("doctor")
        }
        Commands::Transcribe(args) => {
            todo!("transcribe: source_id={:?}, all={}", args.source_id, args.all)
        }
        Commands::Transcripts { command } => match command {
            TranscriptsCommand::List => cmd_transcripts_list(cli),
            TranscriptsCommand::Read { source_id, with_markers } => {
                cmd_transcripts_read(cli, source_id, *with_markers)
            }
            TranscriptsCommand::Search { query, source } => {
                cmd_transcripts_search(cli, query, source.as_deref())
            }
            TranscriptsCommand::Export { format, output } => {
                cmd_transcripts_export(cli, format, output.as_deref())
            }
        },
        Commands::Edit { command } => match command {
            EditCommand::Create { name } => todo!("edit create: {name}"),
            EditCommand::AddSegment(args) => {
                todo!("edit add-segment: edit={}, source={}", args.edit, args.source)
            }
            EditCommand::MoveSegment {
                edit,
                shot,
                position,
            } => todo!("edit move-segment: {edit}, shot={shot}, pos={position}"),
            EditCommand::RemoveSegment { edit, shot } => {
                todo!("edit remove-segment: {edit}, shot={shot}")
            }
            EditCommand::TrimSegment(args) => {
                todo!("edit trim-segment: edit={}, shot={}", args.edit, args.shot)
            }
            EditCommand::Show { edit } => cmd_show(cli, edit),
            EditCommand::History { edit } => cmd_history(cli, edit),
            EditCommand::FromTranscript { file, output } => {
                cmd_from_transcript(cli, file, output.as_deref())
            }
            EditCommand::Note { edit, shot, text } => cmd_note(cli, edit, shot, text),
        },
        Commands::Undo { edit } => cmd_undo(cli, edit),
        Commands::Redo { edit } => cmd_redo(cli, edit),
        Commands::Validate { edit } => todo!("validate: {edit}"),
        Commands::Play(args) => cmd_play(cli, args),
        Commands::Render(args) => cmd_render(cli, args),
        Commands::Index(args) => match &args.command {
            Some(IndexCommand::Show { source_id }) => cmd_index_show(cli, source_id),
            Some(IndexCommand::SetDescription {
                source_id,
                scene,
                text,
            }) => cmd_index_set_description(cli, source_id, *scene, text),
            None => cmd_index_run(cli, &args.run),
        },
        Commands::Search(args) => cmd_search(cli, args),
        Commands::Mark(args) => cmd_mark(cli, args),
        Commands::Markers { source_id, label } => cmd_markers(cli, source_id.as_deref(), label.as_deref()),
        Commands::Schema { command } => match command {
            SchemaCommand::Edit => {
                println!("{}", ar_edit_core::schema::edit_document_schema());
                Ok(())
            }
        },
        Commands::Tui => {
            #[cfg(feature = "tui")]
            {
                tui::run(PathBuf::from("."))
            }
            #[cfg(not(feature = "tui"))]
            {
                anyhow::bail!(
                    "TUI support is not enabled. Rebuild with: cargo build --features tui"
                )
            }
        }
        Commands::Completions { shell } => {
            Cli::print_completions(*shell);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the on-disk path for an edit document: `edits/<name>.edit.json`.
fn edit_path(name: &str) -> PathBuf {
    PathBuf::from("edits").join(format!("{name}.edit.json"))
}

/// Return a human-readable label and the affected shot ID for an op.
fn op_summary(kind: &EditOpKind) -> (&str, &str) {
    match kind {
        EditOpKind::AddShot { shot } => ("add_shot", &shot.id),
        EditOpKind::RemoveShot { shot_id, .. } => ("remove_shot", shot_id),
        EditOpKind::MoveShot { shot_id, .. } => ("move_shot", shot_id),
        EditOpKind::TrimShot { shot_id, .. } => ("trim_shot", shot_id),
        EditOpKind::ReplaceRangeType { shot_id, .. } => ("replace_range_type", shot_id),
        EditOpKind::AddNote { shot_id, .. } => ("add_note", shot_id),
    }
}

// ---------------------------------------------------------------------------
// Command handlers: undo / redo / history
// ---------------------------------------------------------------------------

fn cmd_undo(cli: &Cli, edit: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let mut doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let undone = doc.undo().map_err(|e| anyhow::anyhow!("{e}"))?.clone();
    doc.save(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({
            "head": doc.head,
            "undone_op": undone,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let (op_type, shot_id) = op_summary(&undone.op);
        println!("Undone: #{} {} {} (head \u{2192} {})", undone.id, op_type, shot_id, doc.head);
    }
    Ok(())
}

fn cmd_redo(cli: &Cli, edit: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let mut doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let redone = doc.redo().map_err(|e| anyhow::anyhow!("{e}"))?.clone();
    doc.save(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({
            "head": doc.head,
            "redone_op": redone,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let (op_type, shot_id) = op_summary(&redone.op);
        println!("Redone: #{} {} {} (head \u{2192} {})", redone.id, op_type, shot_id, doc.head);
    }
    Ok(())
}

fn cmd_history(cli: &Cli, edit: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({
            "head": doc.head,
            "ops": doc.ops,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if doc.ops.is_empty() {
            println!("No operations.");
        } else {
            for (i, op) in doc.ops.iter().enumerate() {
                let (op_type, shot_id) = op_summary(&op.op);
                let marker = if i as i32 == doc.head { "\u{2192}" } else { " " };
                let ts = op.ts.format("%Y-%m-%d %H:%M:%S");
                let suffix = if (i as i32) > doc.head { "  (undone)" } else { "" };
                println!("{marker} {id:>3}  {op_type:<19} {shot_id:<12} {ts}{suffix}",
                    id = op.id);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: transcripts (CON-003, REQ-008, REQ-009, REQ-010)
// ---------------------------------------------------------------------------

fn cmd_transcripts_list(cli: &Cli) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let transcripts = ar_edit_core::transcript_ops::list(&project_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({ "transcripts": transcripts });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if transcripts.is_empty() {
            println!("No transcripts.");
        } else {
            println!(
                "  {:<12} {:<12} {:<12} {}",
                "SOURCE", "DURATION", "WORDS", "PATH"
            );
            println!("  {}", "-".repeat(60));
            for t in &transcripts {
                println!(
                    "  {:<12} {:<12} {:<12} {}",
                    t.source_id,
                    ar_edit_core::display::format_time(t.duration_ms),
                    t.word_count,
                    t.path,
                );
            }
        }
    }

    Ok(())
}

fn cmd_transcripts_read(cli: &Cli, source_id: &str, with_markers: bool) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let transcript = ar_edit_core::transcript_ops::read(&project_dir, source_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if with_markers {
        // Load and resolve markers for this source
        let source_markers = ar_edit_core::marker::list_markers(&project_dir, source_id)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let resolved = ar_edit_core::display::resolve_markers(
            &source_markers.markers,
            source_id,
            &project_dir,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let interleaved =
            ar_edit_core::display::interleave_transcript_with_markers(&transcript, &resolved);

        if cli.json {
            println!("{}", serde_json::to_string_pretty(&interleaved)?);
        } else {
            for item in &interleaved.items {
                match item {
                    ar_edit_core::display::TranscriptItem::Segment(seg) => {
                        println!("{}", seg.text);
                        println!();
                    }
                    ar_edit_core::display::TranscriptItem::Marker(m) => {
                        let time_range = format!(
                            "{}-{}",
                            ar_edit_core::display::format_time(m.start_ms),
                            ar_edit_core::display::format_time(m.end_ms),
                        );
                        let note_part = m
                            .note
                            .as_deref()
                            .map(|n| format!(" \"{n}\""))
                            .unwrap_or_default();
                        println!(
                            "  [{} {}] [{}]{}",
                            m.id, m.label, time_range, note_part
                        );
                        println!();
                    }
                }
            }
        }
    } else if cli.json {
        println!("{}", serde_json::to_string_pretty(&transcript)?);
    } else {
        for seg in &transcript.segments {
            println!("{}", seg.text);
            println!();
        }
    }

    Ok(())
}

fn cmd_transcripts_search(
    cli: &Cli,
    query: &str,
    source: Option<&str>,
) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let results = ar_edit_core::transcript_ops::search(&project_dir, query, source)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({ "results": results });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if results.is_empty() {
            println!("No matches.");
        } else {
            for r in &results {
                println!(
                    "  {} [words {}..{}] {}-{}",
                    r.source_id,
                    r.from_word,
                    r.to_word,
                    ar_edit_core::display::format_time(r.start_ms),
                    ar_edit_core::display::format_time(r.end_ms),
                );
                let mut line = String::new();
                if !r.context_before.is_empty() {
                    line.push_str(&format!("...{} ", r.context_before));
                }
                line.push_str(&format!("[{}]", r.text));
                if !r.context_after.is_empty() {
                    line.push_str(&format!(" {}...", r.context_after));
                }
                println!("    {line}");
                println!();
            }
        }
    }

    Ok(())
}

fn cmd_transcripts_export(
    _cli: &Cli,
    format: &str,
    output: Option<&Path>,
) -> anyhow::Result<()> {
    if format != "editable" {
        anyhow::bail!("unsupported export format: {format} (only 'editable' is supported)");
    }

    let project_dir = PathBuf::from(".");
    let markdown = ar_edit_core::export::export_editable(&project_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if let Some(path) = output {
        std::fs::write(path, &markdown)?;
        eprintln!("Exported to {}", path.display());
    } else {
        print!("{markdown}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: show (REQ-016)
// ---------------------------------------------------------------------------

fn cmd_show(cli: &Cli, edit: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let project_dir = PathBuf::from(".");

    let resolved = ar_edit_core::display::resolve_edit(&doc, &project_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({
            "name": doc.name,
            "head": doc.head,
            "shot_count": resolved.len(),
            "shots": resolved,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Edit: {}  ({} shots, head: {})", doc.name, resolved.len(), doc.head);
        println!();

        if resolved.is_empty() {
            println!("  (no shots)");
        } else {
            // Header
            println!(
                "  {:<12} {:<10} {:<12} {:<12} {:<10} {}",
                "SHOT", "SOURCE", "START", "END", "DURATION", "PREVIEW"
            );
            println!("  {}", "-".repeat(78));

            for shot in &resolved {
                let fallback = range_summary(&shot.range);
                let preview = shot
                    .text_preview
                    .as_deref()
                    .or(shot.scene_preview.as_deref())
                    .unwrap_or(&fallback);

                println!(
                    "  {:<12} {:<10} {:<12} {:<12} {:<10} {}",
                    shot.id,
                    shot.source,
                    ar_edit_core::display::format_time(shot.start_ms),
                    ar_edit_core::display::format_time(shot.end_ms),
                    ar_edit_core::display::format_time(shot.duration_ms),
                    preview,
                );

                for note in &shot.notes {
                    println!("  {:>12} note: {}", "", note.text);
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: note (REQ-051, REQ-053)
// ---------------------------------------------------------------------------

fn cmd_note(cli: &Cli, edit: &str, shot: &str, text: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let mut doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let note = doc.add_note(shot, text).map_err(|e| anyhow::anyhow!("{e}"))?.clone();
    doc.save(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&note)?);
    } else {
        println!("Added note to {shot}: {text}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: play (REQ-021, REQ-024)
// ---------------------------------------------------------------------------

fn cmd_play(cli: &Cli, args: &PlayArgs) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");

    let player = playback::detect_player().map_err(|e| anyhow::anyhow!("{e}"))?;

    let is_source = args.target.starts_with("src-");
    let overlay_mode = ar_edit_core::overlay::OverlayMode::from_flag(args.overlay.as_deref());

    if !is_source && args.shot.is_none() {
        // Full edit playback (REQ-022): render all shots concatenated, then play
        return cmd_play_full(cli, &project_dir, &args.target, &player, overlay_mode);
    }

    // Capture shot/source context for feedback before building the request
    let (shot_id, source_id) = if is_source {
        (None, args.target.clone())
    } else {
        let shot_id_str = args.shot.as_deref().unwrap();
        let path = edit_path(&args.target);
        let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;
        let shot = doc
            .snapshot
            .shots
            .iter()
            .find(|s| s.id == shot_id_str)
            .ok_or_else(|| anyhow::anyhow!("shot not found: {shot_id_str}"))?;
        (Some(shot_id_str.to_string()), shot.source.clone())
    };

    let req = if is_source {
        build_source_play_request(&project_dir, &args.target, args)?
    } else {
        build_edit_play_request(&project_dir, &args.target, args.shot.as_deref().unwrap())?
    };

    if !cli.json {
        let end_info = match req.end_ms {
            Some(end) => format!(
                " to {}",
                ar_edit_core::display::format_time(end)
            ),
            None => String::new(),
        };
        println!(
            "Playing {} from {}{}  [{}]",
            req.file.display(),
            ar_edit_core::display::format_time(req.start_ms),
            end_info,
            player.name,
        );
    }

    let mut child = playback::launch_player(&player, &req).map_err(|e| anyhow::anyhow!("{e}"))?;
    child.wait()?;

    // REQ-037: Output structured feedback after playback exits
    if cli.json {
        let feedback = ar_edit_core::feedback::build_feedback(
            req.start_ms,
            shot_id.as_deref(),
            &source_id,
            &project_dir,
        );
        println!("{}", serde_json::to_string_pretty(&feedback)?);
    }

    Ok(())
}

/// Full edit playback: render a preview of all shots concatenated, then launch player.
fn cmd_play_full(
    cli: &Cli,
    project_dir: &PathBuf,
    edit_name: &str,
    player: &playback::Player,
    overlay_mode: ar_edit_core::overlay::OverlayMode,
) -> anyhow::Result<()> {
    let path = edit_path(edit_name);
    let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let shot_count = doc.snapshot.shots.len();
    if !cli.json {
        let overlay_label = match overlay_mode {
            ar_edit_core::overlay::OverlayMode::Clean => "",
            ar_edit_core::overlay::OverlayMode::Full => " [overlay: full]",
            ar_edit_core::overlay::OverlayMode::Minimal => " [overlay: minimal]",
        };
        println!("Rendering full preview of \"{edit_name}\" ({shot_count} shots){overlay_label}...");
    }

    // Resolve shots for feedback timings before rendering
    let resolved = ar_edit_core::display::resolve_edit(&doc, project_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let preview_path = ar_edit_core::render::render_preview(&doc, project_dir, overlay_mode)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let req = playback::PlayRequest {
        file: preview_path.clone(),
        start_ms: 0,
        end_ms: None,
    };

    if !cli.json {
        println!(
            "Playing full edit preview  [{}]",
            player.name,
        );
    }

    let mut child = playback::launch_player(player, &req).map_err(|e| anyhow::anyhow!("{e}"))?;
    child.wait()?;

    // REQ-037: Output structured feedback after playback exits
    if cli.json {
        // Build cumulative timeline offsets for each shot
        let mut shot_timings = Vec::with_capacity(resolved.len());
        let mut offset = 0u64;
        for shot in &resolved {
            shot_timings.push((
                shot.id.clone(),
                shot.source.clone(),
                offset,
                offset + shot.duration_ms,
            ));
            offset += shot.duration_ms;
        }

        let feedback = ar_edit_core::feedback::build_edit_feedback(
            0, // start of preview
            &shot_timings,
            project_dir,
        );
        println!("{}", serde_json::to_string_pretty(&feedback)?);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: render (CON-007, REQ-022)
// ---------------------------------------------------------------------------

fn cmd_render(cli: &Cli, args: &cli::RenderArgs) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let path = edit_path(&args.edit);
    let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let shot_count = doc.snapshot.shots.len();
    if shot_count == 0 {
        anyhow::bail!("edit '{}' has no shots", args.edit);
    }

    let overlay_mode = if args.burn_overlay {
        ar_edit_core::overlay::OverlayMode::Full
    } else {
        ar_edit_core::overlay::OverlayMode::Clean
    };

    if !cli.json {
        let overlay_label = if args.burn_overlay { " [overlay: full]" } else { "" };
        eprintln!(
            "Rendering '{}' ({} shots) to {}{}...",
            args.edit, shot_count, args.output.display(), overlay_label
        );
    }

    // Ensure parent directory exists for the output file
    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    ar_edit_core::render::render_to_file(&doc, &project_dir, &args.output, overlay_mode)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({
            "edit": args.edit,
            "output": args.output.display().to_string(),
            "shot_count": shot_count,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        eprintln!("Done. Output: {}", args.output.display());
    }

    Ok(())
}

fn build_source_play_request(
    project_dir: &PathBuf,
    source_id: &str,
    args: &PlayArgs,
) -> anyhow::Result<playback::PlayRequest> {
    let (file, _source) =
        playback::resolve_source_path(source_id, project_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    let start_ms = if let Some(ref tc) = args.at {
        playback::parse_timecode(tc).map_err(|e| anyhow::anyhow!("{e}"))?
    } else if let Some(word_idx) = args.at_word {
        let range = ShotRange::Words {
            from: word_idx,
            to: word_idx,
        };
        let transcripts_dir = project_dir.join("transcripts");
        let (start, _end) = ar_edit_core::resolve::resolve_range_from_dir(
            &range,
            source_id,
            &transcripts_dir,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        start
    } else if let Some(scene_idx) = args.at_scene {
        let range = ShotRange::Scenes {
            from: scene_idx,
            to: scene_idx,
        };
        let index_dir = project_dir.join("index");
        let (start, _end) = ar_edit_core::resolve::resolve_range_from_dir(
            &range,
            source_id,
            &index_dir,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        start
    } else {
        0
    };

    Ok(playback::PlayRequest {
        file,
        start_ms,
        end_ms: None,
    })
}

fn build_edit_play_request(
    project_dir: &PathBuf,
    edit_name: &str,
    shot_id: &str,
) -> anyhow::Result<playback::PlayRequest> {
    let path = edit_path(edit_name);
    let doc = EditDocument::load(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    let shot = doc
        .snapshot
        .shots
        .iter()
        .find(|s| s.id == shot_id)
        .ok_or_else(|| anyhow::anyhow!("shot not found: {shot_id}"))?;

    let source_id = &shot.source;
    let (file, _source) =
        playback::resolve_source_path(source_id, project_dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Resolve shot range to timestamps
    let range = &shot.range;
    let dir = match range {
        ShotRange::Words { .. } => project_dir.join("transcripts"),
        ShotRange::Scenes { .. } => project_dir.join("index"),
        ShotRange::Time { .. } => project_dir.to_path_buf(),
    };
    let (start_ms, end_ms) =
        ar_edit_core::resolve::resolve_range_from_dir(range, source_id, &dir)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(playback::PlayRequest {
        file,
        start_ms,
        end_ms: Some(end_ms),
    })
}

// ---------------------------------------------------------------------------
// Command handlers: mark / markers (REQ-049, REQ-050)
// ---------------------------------------------------------------------------

fn cmd_mark(cli: &Cli, args: &cli::MarkArgs) -> anyhow::Result<()> {
    let range = parse_range(&args.range)?;
    let project_dir = PathBuf::from(".");

    let marker = ar_edit_core::marker::add_marker(
        &project_dir,
        &args.source_id,
        range,
        &args.label,
        args.note.as_deref(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&marker)?);
    } else {
        let note_part = marker
            .note
            .as_deref()
            .map(|n| format!("  note: {n}"))
            .unwrap_or_default();
        println!(
            "Created {} on {} [{}] label={}{}",
            marker.id,
            args.source_id,
            range_summary(&marker.range),
            marker.label,
            note_part,
        );
    }
    Ok(())
}

fn cmd_markers(cli: &Cli, source_id: Option<&str>, label: Option<&str>) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");

    // Collect source markers: either one source or all
    let source_docs = if let Some(sid) = source_id {
        let doc = ar_edit_core::marker::list_markers(&project_dir, sid)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        vec![doc]
    } else {
        ar_edit_core::marker::list_all_markers(&project_dir)
            .map_err(|e| anyhow::anyhow!("{e}"))?
    };

    // Resolve all markers and apply label filter
    let mut all_resolved = Vec::new();
    for doc in &source_docs {
        let markers: Vec<_> = if let Some(lbl) = label {
            doc.markers.iter().filter(|m| m.label == lbl).cloned().collect()
        } else {
            doc.markers.clone()
        };

        if markers.is_empty() {
            continue;
        }

        let resolved = ar_edit_core::display::resolve_markers(&markers, &doc.source_id, &project_dir)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        all_resolved.extend(resolved);
    }

    if cli.json {
        let output = serde_json::json!({ "markers": all_resolved });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if all_resolved.is_empty() {
            if let Some(sid) = source_id {
                println!("No markers for {sid}.");
            } else {
                println!("No markers.");
            }
        } else {
            for m in &all_resolved {
                let time_range = format!(
                    "{}-{}",
                    ar_edit_core::display::format_time(m.start_ms),
                    ar_edit_core::display::format_time(m.end_ms),
                );

                let note_part = m
                    .note
                    .as_deref()
                    .map(|n| format!("  \"{n}\""))
                    .unwrap_or_default();

                let preview = m
                    .text_preview
                    .as_deref()
                    .or(m.scene_preview.as_deref())
                    .unwrap_or("");

                println!(
                    "  {}  {}  {:<8} [{}] {}{}",
                    m.id,
                    m.source_id,
                    m.label,
                    time_range,
                    preview,
                    note_part,
                );
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: search (CON-008, REQ-034)
// ---------------------------------------------------------------------------

fn cmd_search(cli: &Cli, args: &cli::SearchArgs) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");

    let type_filter = args.r#type.as_ref().map(|t| match t {
        SearchType::Transcript => ar_edit_core::search::TypeFilter::Transcript,
        SearchType::Scene => ar_edit_core::search::TypeFilter::Scene,
        SearchType::Metadata => ar_edit_core::search::TypeFilter::Metadata,
    });

    let results = ar_edit_core::search::search(
        &project_dir,
        &args.query,
        args.source.as_deref(),
        type_filter.as_ref(),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({ "results": results });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if results.is_empty() {
        println!("No matches.");
    } else {
        for r in &results {
            let type_label = match r.result_type {
                ar_edit_core::search::ResultType::Transcript => "transcript",
                ar_edit_core::search::ResultType::Scene => "scene",
                ar_edit_core::search::ResultType::Metadata => "metadata",
            };
            println!(
                "  [{}] {} {}-{}",
                type_label,
                r.source_id,
                ar_edit_core::display::format_time(r.start_ms),
                ar_edit_core::display::format_time(r.end_ms),
            );
            println!("    {}", r.context);
            println!();
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: index (CON-008, REQ-031)
// ---------------------------------------------------------------------------

fn cmd_index_run(cli: &Cli, args: &cli::IndexRunArgs) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let mut manifest = ar_edit_core::project::read_manifest(&project_dir)?;

    let threshold = 0.3;
    let interval_sec = args
        .interval
        .map(|i| i as u32)
        .unwrap_or(manifest.defaults.thumbnail_interval_sec);

    let sources: Vec<Source> = if args.all {
        manifest
            .sources
            .iter()
            .filter(|s| !s.indexed)
            .cloned()
            .collect()
    } else if let Some(ref id) = args.source_id {
        let source = manifest
            .sources
            .iter()
            .find(|s| s.id == *id)
            .ok_or_else(|| anyhow::anyhow!("source not found: {id}"))?;
        vec![source.clone()]
    } else {
        anyhow::bail!("specify a source ID or --all");
    };

    if sources.is_empty() {
        if cli.json {
            println!("[]");
        } else {
            println!("No sources to index.");
        }
        return Ok(());
    }

    let parallel = args.parallel.unwrap_or(1).max(1);
    let mut indexed = Vec::new();

    if parallel > 1 && sources.len() > 1 {
        for chunk in sources.chunks(parallel) {
            let results: Vec<Result<_, _>> = thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|source| {
                        s.spawn(|| {
                            ar_edit_core::index::build_source_index(
                                &project_dir,
                                source,
                                threshold,
                                interval_sec,
                            )
                        })
                    })
                    .collect();

                handles
                    .into_iter()
                    .map(|h| h.join().unwrap())
                    .collect()
            });

            for result in results {
                let index = result.map_err(|e| anyhow::anyhow!("{e}"))?;
                if let Some(s) = manifest.sources.iter_mut().find(|s| s.id == index.source_id) {
                    s.indexed = true;
                }
                if !cli.json {
                    println!(
                        "Indexed {} ({} scenes, {} thumbnails)",
                        index.source_id, index.scene_count, index.thumbnails.len()
                    );
                }
                indexed.push(index);
            }
        }
    } else {
        for source in &sources {
            let index = ar_edit_core::index::build_source_index(
                &project_dir,
                source,
                threshold,
                interval_sec,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

            if let Some(s) = manifest.sources.iter_mut().find(|s| s.id == source.id) {
                s.indexed = true;
            }
            if !cli.json {
                println!(
                    "Indexed {} ({} scenes, {} thumbnails)",
                    source.id, index.scene_count, index.thumbnails.len()
                );
            }
            indexed.push(index);
        }
    }

    ar_edit_core::project::write_manifest(&project_dir, &manifest)?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&indexed)?);
    }

    Ok(())
}

fn cmd_index_show(cli: &Cli, source_id: &str) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let index =
        ar_edit_core::index::load_index(&project_dir, source_id).map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&index)?);
    } else {
        println!("Source: {}", index.source_id);
        println!(
            "  Duration: {:.1}s  Resolution: {}x{}  Codec: {}  Size: {} bytes",
            index.metadata.duration_ms as f64 / 1000.0,
            index.metadata.resolution.0,
            index.metadata.resolution.1,
            index.metadata.codec,
            index.metadata.file_size_bytes
        );
        println!("  Scenes: {}  Thumbnails: {}", index.scene_count, index.thumbnails.len());
        println!();

        for scene in &index.scenes {
            let desc = scene
                .description
                .as_deref()
                .unwrap_or("(no description)");
            let duration = scene.end_ms - scene.start_ms;
            println!(
                "  Scene {}: {:.1}s - {:.1}s ({:.1}s)  {}",
                scene.index,
                scene.start_ms as f64 / 1000.0,
                scene.end_ms as f64 / 1000.0,
                duration as f64 / 1000.0,
                desc
            );
        }
    }

    Ok(())
}

fn cmd_index_set_description(
    cli: &Cli,
    source_id: &str,
    scene: u32,
    text: &str,
) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let (index, old_description) =
        ar_edit_core::index::set_scene_description(&project_dir, source_id, scene, text)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let mut output = serde_json::to_value(&index.scenes[scene as usize])?;
        output["old_description"] = serde_json::json!(old_description);
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if let Some(ref old) = old_description {
            eprintln!("Replaced description for {} scene {}: {}", source_id, scene, old);
        }
        println!(
            "Set description for {} scene {}: {}",
            source_id, scene, text
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: from-transcript (CON-004, REQ-018)
// ---------------------------------------------------------------------------

fn cmd_from_transcript(cli: &Cli, file: &Path, output: Option<&str>) -> anyhow::Result<()> {
    let doc = ar_edit_core::import::from_transcript(file, output).map_err(|e| {
        match &e {
            ar_edit_core::import::ImportError::ParseErrors(_) => {
                // Parse errors should exit with code 1 (USER_ERROR)
                anyhow::anyhow!("{e}")
            }
            _ => anyhow::anyhow!("{e}"),
        }
    })?;

    let edits_dir = PathBuf::from("edits");
    std::fs::create_dir_all(&edits_dir)?;

    let path = edits_dir.join(format!("{}.edit.json", doc.name));
    let shot_count = doc.snapshot.shots.len();
    let name = doc.name.clone();

    doc.save(&path).map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        let output = serde_json::json!({
            "name": name,
            "path": path.display().to_string(),
            "shot_count": shot_count,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Created edit '{}' with {} shots from {}",
            name,
            shot_count,
            file.display()
        );
    }

    Ok(())
}

/// Convert CLI RangeArgs into a ShotRange.
fn parse_range(range: &RangeArgs) -> anyhow::Result<ShotRange> {
    if let (Some(from), Some(to)) = (range.from_word, range.to_word) {
        Ok(ShotRange::Words { from, to })
    } else if let (Some(from), Some(to)) = (range.from_scene, range.to_scene) {
        Ok(ShotRange::Scenes { from, to })
    } else if let (Some(from_ms), Some(to_ms)) = (range.from_ms, range.to_ms) {
        Ok(ShotRange::Time { from_ms, to_ms })
    } else {
        anyhow::bail!(
            "no range specified (use --from-word/--to-word, --from-scene/--to-scene, or --from-ms/--to-ms)"
        )
    }
}

/// Human-readable summary of a ShotRange.
fn range_summary(range: &ShotRange) -> String {
    match range {
        ShotRange::Words { from, to } => format!("words {from}..{to}"),
        ShotRange::Scenes { from, to } => format!("scenes {from}..{to}"),
        ShotRange::Time { from_ms, to_ms } => format!("{from_ms}ms..{to_ms}ms"),
    }
}

