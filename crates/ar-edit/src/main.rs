mod cli;

use std::path::PathBuf;
use std::process;

use ar_edit_core::models::{EditDocument, EditOpKind, ShotRange, Source};
use clap::Parser;
use cli::{
    exit_code, Cli, Commands, EditCommand, IndexCommand, RangeArgs, SchemaCommand,
    TranscriptsCommand,
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
            TranscriptsCommand::List => todo!("transcripts list"),
            TranscriptsCommand::Read { source_id } => todo!("transcripts read: {source_id}"),
            TranscriptsCommand::Search { query, source } => {
                todo!("transcripts search: {query}, source={source:?}")
            }
            TranscriptsCommand::Export { format, output } => {
                todo!("transcripts export: format={format}, output={output:?}")
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
                todo!("edit from-transcript: {file:?}, output={output:?}")
            }
            EditCommand::Note { edit, shot, text } => cmd_note(cli, edit, shot, text),
        },
        Commands::Undo { edit } => cmd_undo(cli, edit),
        Commands::Redo { edit } => cmd_redo(cli, edit),
        Commands::Validate { edit } => todo!("validate: {edit}"),
        Commands::Play(args) => todo!("play: target={}", args.target),
        Commands::Render(args) => todo!("render: edit={}, output={:?}", args.edit, args.output),
        Commands::Index(args) => match &args.command {
            Some(IndexCommand::Show { source_id }) => cmd_index_show(cli, source_id),
            Some(IndexCommand::SetDescription {
                source_id,
                scene,
                text,
            }) => cmd_index_set_description(cli, source_id, *scene, text),
            None => cmd_index_run(cli, &args.run),
        },
        Commands::Search(args) => todo!("search: {}", args.query),
        Commands::Mark(args) => cmd_mark(cli, args),
        Commands::Markers { source_id } => cmd_markers(cli, source_id),
        Commands::Schema { command } => match command {
            SchemaCommand::Edit => todo!("schema edit"),
        },
        Commands::Tui => todo!("tui"),
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

fn cmd_markers(cli: &Cli, source_id: &str) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");

    let doc = ar_edit_core::marker::list_markers(&project_dir, source_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        if doc.markers.is_empty() {
            println!("No markers for {source_id}.");
        } else {
            for m in &doc.markers {
                let note_part = m
                    .note
                    .as_deref()
                    .map(|n| format!("  \"{n}\""))
                    .unwrap_or_default();
                println!(
                    "  {}  {:<8} [{}]{}",
                    m.id,
                    m.label,
                    range_summary(&m.range),
                    note_part,
                );
            }
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
    let index = ar_edit_core::index::set_scene_description(&project_dir, source_id, scene, text)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&index.scenes[scene as usize])?
        );
    } else {
        println!(
            "Set description for {} scene {}: {}",
            source_id, scene, text
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

