mod cli;
#[cfg(feature = "tui")]
mod tui;

use std::io::Write;
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

// ---------------------------------------------------------------------------
// CLI error with exit code classification (CON-009)
// ---------------------------------------------------------------------------

/// An error that carries an exit code for the process.
///
/// Wrap core-library errors with one of the constructors (`user`, `system`,
/// `validation`) so that `main()` can extract the correct exit code.
#[derive(Debug)]
struct CliError {
    message: String,
    code: i32,
}

impl CliError {
    fn user(msg: impl std::fmt::Display) -> Self {
        Self {
            message: msg.to_string(),
            code: exit_code::USER_ERROR,
        }
    }

    fn system(msg: impl std::fmt::Display) -> Self {
        Self {
            message: msg.to_string(),
            code: exit_code::SYSTEM_ERROR,
        }
    }

    fn validation(msg: impl std::fmt::Display) -> Self {
        Self {
            message: msg.to_string(),
            code: exit_code::VALIDATION_ERROR,
        }
    }

    fn with_hint(mut self, hint: &str) -> Self {
        self.message = format!("{}\n\n  Hint: {hint}", self.message);
        self
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// Extension trait for classifying Result errors with exit codes.
trait ResultExt<T> {
    fn user_err(self) -> anyhow::Result<T>;
    fn system_err(self) -> anyhow::Result<T>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    fn user_err(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow::Error::new(CliError::user(&e)))
    }
    fn system_err(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow::Error::new(CliError::system(&e)))
    }
}

fn main() {
    let cli = Cli::parse();

    let code = match run(&cli) {
        Ok(()) => exit_code::SUCCESS,
        Err(e) => {
            let code = e
                .downcast_ref::<CliError>()
                .map(|ce| ce.code)
                .unwrap_or(exit_code::USER_ERROR);

            if cli.json {
                let msg = serde_json::json!({ "error": format!("{e:#}") });
                eprintln!("{msg}");
            } else {
                eprintln!("Error: {e:#}");
            }
            code
        }
    };

    process::exit(code);
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Init { name } => {
            let path = PathBuf::from(name);
            let manifest = ar_edit_core::project::init(&path)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            } else {
                println!("Initialized project '{}' at {}", manifest.name, path.display());
            }
            Ok(())
        }
        Commands::Add { files } => {
            let added = ar_edit_core::project::add(Path::new("."), files)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&added)?);
            } else {
                for s in &added {
                    println!("Added source {}: {}", s.id, s.original_filename);
                }
            }
            Ok(())
        }
        Commands::Doctor => {
            let result = ar_edit_core::project::doctor();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let check = |name: &str, dep: &ar_edit_core::project::DepStatus| {
                    if dep.found {
                        println!("  {} {}", name, dep.version.as_deref().unwrap_or("found"));
                    } else if let Some(fb) = &dep.fallback {
                        let hint = dep.install_hint.as_deref()
                            .map(|h| format!("  Install: {h}"))
                            .unwrap_or_default();
                        println!("  {} missing (fallback: {}){}", name, fb, hint);
                    } else {
                        let hint = dep.install_hint.as_deref()
                            .map(|h| format!("  Install: {h}"))
                            .unwrap_or_default();
                        println!("  {} MISSING{}", name, hint);
                    }
                };
                println!("Dependencies:");
                check("ffmpeg", &result.ffmpeg);
                check("ffprobe", &result.ffprobe);
                check("whisper-cli", &result.whisper);
                check("vlc", &result.vlc);
            }
            Ok(())
        }
        Commands::Transcribe(args) => cmd_transcribe(cli, args),
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
            EditCommand::Create { name } => {
                let path = edit_path(name);
                if path.exists() {
                    anyhow::bail!("edit '{}' already exists", name);
                }
                let doc = EditDocument::create(name);
                doc.save(&path)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!("Created edit '{}'", name);
                }
                Ok(())
            }
            EditCommand::AddSegment(args) => {
                let path = edit_path(&args.edit);
                let mut doc = EditDocument::load(&path)?;
                let range = parse_range(&args.range)?;

                // Eager validation: check source exists and range is in bounds
                let project_dir = PathBuf::from(".");
                let manifest = ar_edit_core::project::read_manifest(&project_dir)?;
                let errors = ar_edit_core::validate::validate_shot_source(
                    &args.source, &range, &manifest, &project_dir,
                );
                if !errors.is_empty() {
                    for err in &errors {
                        eprintln!("error: {}", err);
                    }
                    anyhow::bail!("invalid segment: {} error(s)", errors.len());
                }

                let shot = doc.add_shot(&args.source, range)?;
                let shot_id = shot.id.clone();
                doc.save(&path)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!("Added {} to '{}'", shot_id, args.edit);
                }
                Ok(())
            }
            EditCommand::MoveSegment {
                edit,
                shot,
                position,
            } => {
                let path = edit_path(edit);
                let mut doc = EditDocument::load(&path)?;
                doc.move_shot(shot, *position as usize)?;
                doc.save(&path)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!("Moved {} to position {} in '{}'", shot, position, edit);
                }
                Ok(())
            }
            EditCommand::RemoveSegment { edit, shot } => {
                let path = edit_path(edit);
                let mut doc = EditDocument::load(&path)?;
                doc.remove_shot(shot)?;
                doc.save(&path)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!("Removed {} from '{}'", shot, edit);
                }
                Ok(())
            }
            EditCommand::TrimSegment(args) => {
                let path = edit_path(&args.edit);
                let mut doc = EditDocument::load(&path)?;
                let range = parse_range(&args.range)?;

                // Find the shot's source for validation
                let shot_source = doc.snapshot.shots.iter()
                    .find(|s| s.id == args.shot)
                    .map(|s| s.source.clone())
                    .ok_or_else(|| anyhow::anyhow!("shot '{}' not found", args.shot))?;

                // Eager validation: check range is in bounds for the source
                let project_dir = PathBuf::from(".");
                let manifest = ar_edit_core::project::read_manifest(&project_dir)?;
                let errors = ar_edit_core::validate::validate_shot_source(
                    &shot_source, &range, &manifest, &project_dir,
                );
                if !errors.is_empty() {
                    for err in &errors {
                        eprintln!("error: {}", err);
                    }
                    anyhow::bail!("invalid segment: {} error(s)", errors.len());
                }

                doc.trim_shot(&args.shot, range)?;
                doc.save(&path)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!("Trimmed {} in '{}'", args.shot, args.edit);
                }
                Ok(())
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
        Commands::Validate { edit } => cmd_validate(cli, edit),
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

/// Load an edit document, producing a user-error with hint on failure.
fn load_edit(edit: &str) -> anyhow::Result<EditDocument> {
    let path = edit_path(edit);
    EditDocument::load(&path).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                &format!("check that edit '{edit}' exists in the edits/ directory"),
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// Command handlers: undo / redo / history
// ---------------------------------------------------------------------------

fn cmd_undo(cli: &Cli, edit: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let mut doc = load_edit(edit)?;

    let undone = doc
        .undo()
        .map_err(|e| anyhow::Error::new(CliError::user(e)))?
        .clone();
    doc.save(&path).system_err()?;

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
    let mut doc = load_edit(edit)?;

    let redone = doc
        .redo()
        .map_err(|e| anyhow::Error::new(CliError::user(e)))?
        .clone();
    doc.save(&path).system_err()?;

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
    let doc = load_edit(edit)?;

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
// Command handler: transcribe (CON-002)
// ---------------------------------------------------------------------------

fn cmd_transcribe(cli: &Cli, args: &cli::TranscribeArgs) -> anyhow::Result<()> {
    use ar_edit_core::{project, transcript};

    let project_dir = PathBuf::from(".");
    let manifest = project::read_manifest(&project_dir)?;

    // Handle --import: import an existing SRT/VTT/JSON file
    if let Some(ref import_path) = args.import {
        let source_id = args
            .source_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--import requires a source ID argument"))?;
        let t = transcript::import_transcript(import_path, source_id)?;
        let out_path = project_dir
            .join("transcripts")
            .join(format!("{source_id}.transcript.json"));
        let json = serde_json::to_string_pretty(&t)?;
        std::fs::write(&out_path, json)?;

        // Mark source as transcribed in manifest
        let mut manifest = manifest;
        if let Some(src) = manifest.sources.iter_mut().find(|s| s.id == source_id) {
            src.transcribed = true;
            project::write_manifest(&project_dir, &manifest)?;
        }

        if cli.json {
            println!("{}", serde_json::to_string_pretty(&t)?);
        } else {
            println!(
                "Imported transcript for {} ({} words)",
                source_id, t.word_count
            );
        }
        return Ok(());
    }

    let mut manifest = manifest;

    // Collect source IDs to transcribe
    let source_ids: Vec<String> = if args.all {
        manifest
            .sources
            .iter()
            .filter(|s| !s.transcribed)
            .map(|s| s.id.clone())
            .collect()
    } else if let Some(ref id) = args.source_id {
        if !manifest.sources.iter().any(|s| s.id == *id) {
            anyhow::bail!("source '{}' not found in manifest", id);
        }
        vec![id.clone()]
    } else {
        anyhow::bail!("provide a source ID or use --all");
    };

    if source_ids.is_empty() {
        if cli.json {
            println!("{{\"transcribed\":[]}}");
        } else {
            println!("All sources already transcribed.");
        }
        return Ok(());
    }

    let model_name = args
        .model
        .as_deref()
        .unwrap_or(&manifest.defaults.whisper_model);
    let model_path = transcript::find_model(model_name)?;
    let mut results = Vec::new();

    for source_id in &source_ids {
        let source = manifest
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .unwrap();

        if !cli.json {
            eprintln!("Transcribing {}...", source_id);
        }

        // Extract audio
        let audio_path = project_dir
            .join("sources")
            .join(format!("{source_id}.wav"));
        let source_path = project_dir.join(&source.path);
        transcript::extract_audio(&source_path, &audio_path)?;

        // Run whisper
        let (t, _progress) =
            transcript::invoke_whisper(&audio_path, &model_path, source_id)?;

        // Save transcript
        let out_path = project_dir
            .join("transcripts")
            .join(format!("{source_id}.transcript.json"));
        let json = serde_json::to_string_pretty(&t)?;
        std::fs::write(&out_path, &json)?;

        // Clean up audio
        let _ = std::fs::remove_file(&audio_path);

        // Mark transcribed
        if let Some(src) = manifest.sources.iter_mut().find(|s| &s.id == source_id) {
            src.transcribed = true;
        }

        if !cli.json {
            eprintln!(
                "  {} words, {}",
                t.word_count,
                ar_edit_core::display::format_time(t.duration_ms)
            );
        }
        results.push(t);
    }

    project::write_manifest(&project_dir, &manifest)?;

    if cli.json {
        let output = serde_json::json!({ "transcribed": results });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Transcribed {} source{}.",
            results.len(),
            if results.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: transcripts (CON-003, REQ-008, REQ-009, REQ-010)
// ---------------------------------------------------------------------------

fn cmd_transcripts_list(cli: &Cli) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");
    let transcripts = ar_edit_core::transcript_ops::list(&project_dir).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                "ensure you are inside an ar-edit project directory, or run `ar-edit init <name>` to create one",
            ),
        )
    })?;

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
    let transcript = ar_edit_core::transcript_ops::read(&project_dir, source_id).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                &format!("run `ar-edit transcribe {source_id}` to generate a transcript, or check source ID with `ar-edit transcripts list`"),
            ),
        )
    })?;

    if with_markers {
        // Load and resolve markers for this source
        let source_markers = ar_edit_core::marker::list_markers(&project_dir, source_id)
            .user_err()?;
        let resolved = ar_edit_core::display::resolve_markers(
            &source_markers.markers,
            source_id,
            &project_dir,
        )
        .user_err()?;

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
        .user_err()?;

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
        anyhow::bail!("unsupported export format '{format}': only 'editable' is currently supported");
    }

    let project_dir = PathBuf::from(".");
    let markdown = ar_edit_core::export::export_editable(&project_dir)
        .user_err()?;

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
    let doc = load_edit(edit)?;
    let project_dir = PathBuf::from(".");

    let resolved = ar_edit_core::display::resolve_edit(&doc, &project_dir)
        .user_err()?;

    if cli.json {
        let total_duration_ms: u64 = resolved.iter().map(|s| s.duration_ms).sum();
        let output = serde_json::json!({
            "name": doc.name,
            "head": doc.head,
            "shot_count": resolved.len(),
            "total_duration_ms": total_duration_ms,
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

            let total_ms: u64 = resolved.iter().map(|s| s.duration_ms).sum();
            println!("  {}", "-".repeat(78));
            println!(
                "  Total: {} shots, {}",
                resolved.len(),
                ar_edit_core::display::format_time_hms(total_ms),
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: note (REQ-051, REQ-053)
// ---------------------------------------------------------------------------

fn cmd_note(cli: &Cli, edit: &str, shot: &str, text: &str) -> anyhow::Result<()> {
    let path = edit_path(edit);
    let mut doc = load_edit(edit)?;

    let note = doc.add_note(shot, text).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                &format!("run `ar-edit edit show {edit}` to list available shots"),
            ),
        )
    })?.clone();
    doc.save(&path).system_err()?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&note)?);
    } else {
        println!("Added note to {shot}: {text}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: validate (CON-005, REQ-017)
// ---------------------------------------------------------------------------

fn cmd_validate(cli: &Cli, edit: &str) -> anyhow::Result<()> {
    let doc = load_edit(edit)?;
    let project_dir = PathBuf::from(".");
    let manifest = ar_edit_core::project::read_manifest(&project_dir).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                "ensure you are inside an ar-edit project directory",
            ),
        )
    })?;

    let result = ar_edit_core::validate::validate(&doc, &manifest, &project_dir);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if result.valid {
        println!("Edit '{edit}' is valid.");
    } else {
        eprintln!("Edit '{edit}' has {} validation error{}:",
            result.errors.len(),
            if result.errors.len() == 1 { "" } else { "s" }
        );
        for err in &result.errors {
            eprintln!("  {}: {}", err.shot_id, err.error);
        }
    }

    if !result.valid {
        return Err(anyhow::Error::new(
            CliError::validation(format!(
                "edit '{edit}' failed validation with {} error{}",
                result.errors.len(),
                if result.errors.len() == 1 { "" } else { "s" }
            ))
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Command handlers: play (REQ-021, REQ-024)
// ---------------------------------------------------------------------------

fn cmd_play(cli: &Cli, args: &PlayArgs) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(".");

    let player = playback::detect_player().map_err(|e| {
        anyhow::Error::new(
            CliError::system(e).with_hint("run `ar-edit doctor` to check dependencies"),
        )
    })?;

    let is_source = args.target.starts_with("src-");
    let overlay_mode = ar_edit_core::overlay::OverlayMode::from_flag(args.overlay.as_deref());

    if !is_source && args.shot.is_none() {
        // Full edit playback (REQ-022): render all shots concatenated, then play
        return cmd_play_full(cli, &project_dir, &args.target, &player, overlay_mode, args.resolution.as_deref());
    }

    // Capture shot/source context for feedback before building the request
    let (shot_id, source_id) = if is_source {
        (None, args.target.clone())
    } else {
        // Safe: clap guarantees --shot is required for edit playback when this
        // branch is reached, and the `if !is_source && args.shot.is_none()`
        // guard above redirects to full-edit playback.
        let shot_id_str = args.shot.as_deref().ok_or_else(|| {
            anyhow::Error::new(CliError::user("--shot is required for edit playback"))
        })?;
        let doc = load_edit(&args.target)?;
        let shot = doc
            .snapshot
            .shots
            .iter()
            .find(|s| s.id == shot_id_str)
            .ok_or_else(|| {
                anyhow::Error::new(
                    CliError::user(format!("shot not found: {shot_id_str}"))
                        .with_hint(&format!(
                            "run `ar-edit edit show {}` to list available shots",
                            args.target
                        )),
                )
            })?;
        (Some(shot_id_str.to_string()), shot.source.clone())
    };

    let req = if is_source {
        build_source_play_request(&project_dir, &args.target, args)?
    } else {
        // Safe: same reasoning as above — shot is guaranteed to be Some here.
        let shot_ref = args.shot.as_deref().ok_or_else(|| {
            anyhow::Error::new(CliError::user("--shot is required for edit playback"))
        })?;
        build_edit_play_request(&project_dir, &args.target, shot_ref)?
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

    let mut child = playback::launch_player(&player, &req).system_err()?;
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
    resolution_flag: Option<&str>,
) -> anyhow::Result<()> {
    let doc = load_edit(edit_name)?;

    // Default preview to 720p; override with --resolution if provided.
    let resolution = match resolution_flag {
        Some(s) => Some(ar_edit_core::render::parse_resolution(s).map_err(|e| {
            anyhow::Error::new(CliError::user(e).with_hint("expected format: WIDTHxHEIGHT, e.g. 1920x1080"))
        })?),
        None => Some((1280, 720)),
    };
    let render_options = ar_edit_core::render::RenderOptions {
        resolution,
        ..Default::default()
    };

    let shot_count = doc.snapshot.shots.len();
    if !cli.json {
        let overlay_label = match overlay_mode {
            ar_edit_core::overlay::OverlayMode::Clean => "",
            ar_edit_core::overlay::OverlayMode::Full => " [overlay: full]",
            ar_edit_core::overlay::OverlayMode::Minimal => " [overlay: minimal]",
        };
        let res = resolution.unwrap();
        println!("Rendering full preview of \"{edit_name}\" ({shot_count} shots) [{}x{}]{overlay_label}...", res.0, res.1);
    }

    // Resolve shots for feedback timings before rendering
    let resolved = ar_edit_core::display::resolve_edit(&doc, project_dir)
        .user_err()?;

    let preview_path = ar_edit_core::render::render_preview(&doc, project_dir, overlay_mode, &render_options)
        .map_err(|e| {
            anyhow::Error::new(
                CliError::system(e).with_hint("run `ar-edit doctor` to check dependencies"),
            )
        })?;

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

    let mut child = playback::launch_player(player, &req).system_err()?;
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
    let doc = load_edit(&args.edit)?;

    let shot_count = doc.snapshot.shots.len();
    if shot_count == 0 {
        return Err(anyhow::Error::new(
            CliError::user(format!("edit '{}' has no shots", args.edit))
                .with_hint("add shots with `ar-edit edit add-segment`"),
        ));
    }

    // Auto-validate before rendering
    let manifest = ar_edit_core::project::read_manifest(&project_dir).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                "ensure you are inside an ar-edit project directory",
            ),
        )
    })?;
    let validation = ar_edit_core::validate::validate(&doc, &manifest, &project_dir);
    if !validation.valid {
        if !cli.json {
            eprintln!("Edit '{}' has {} validation error{}:",
                args.edit,
                validation.errors.len(),
                if validation.errors.len() == 1 { "" } else { "s" }
            );
            for err in &validation.errors {
                eprintln!("  {}: {}", err.shot_id, err.error);
            }
        }
        return Err(anyhow::Error::new(
            CliError::validation(format!(
                "edit '{}' failed validation with {} error{}",
                args.edit,
                validation.errors.len(),
                if validation.errors.len() == 1 { "" } else { "s" }
            ))
            .with_hint("run `ar-edit validate` to see details"),
        ));
    }

    let overlay_mode = if args.burn_overlay {
        ar_edit_core::overlay::OverlayMode::Full
    } else {
        ar_edit_core::overlay::OverlayMode::Clean
    };

    // Parse render format options (REQ-026)
    let resolution = args
        .resolution
        .as_deref()
        .map(ar_edit_core::render::parse_resolution)
        .transpose()
        .user_err()?;

    let render_options = ar_edit_core::render::RenderOptions {
        video_codec: args.codec.clone(),
        resolution,
        subtitles: args.subtitles,
    };

    if !cli.json {
        let overlay_label = if args.burn_overlay { " [overlay: full]" } else { "" };
        let codec_label = args.codec.as_deref().map(|c| format!(" [codec: {c}]")).unwrap_or_default();
        let res_label = args.resolution.as_deref().map(|r| format!(" [resolution: {r}]")).unwrap_or_default();
        let subs_label = if args.subtitles { " [subtitles]" } else { "" };
        eprintln!(
            "Rendering '{}' ({} shots) to {}{}{}{}{}...",
            args.edit, shot_count, args.output.display(), overlay_label, codec_label, res_label, subs_label
        );
    }

    // Ensure parent directory exists for the output file
    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let render_err = |e: ar_edit_core::render::RenderError| -> anyhow::Error {
        anyhow::Error::new(
            CliError::system(e).with_hint("run `ar-edit doctor` to check dependencies"),
        )
    };

    if cli.json {
        // --json mode: streaming JSON lines for progress (CON-007, REQ-027)
        ar_edit_core::render::render_to_file_with_progress(
            &doc,
            &project_dir,
            &args.output,
            overlay_mode,
            &render_options,
            move |rp| {
                let line = serde_json::json!({
                    "progress": (rp.progress * 100.0).round() / 100.0,
                    "current_shot": rp.current_shot,
                    "eta_seconds": rp.eta_seconds.unwrap_or(0),
                });
                let _ = writeln!(std::io::stderr(), "{}", line);
            },
        )
        .map_err(render_err)?;

        // Final success line on stdout
        let output = serde_json::json!({
            "success": true,
            "edit": args.edit,
            "output": args.output.display().to_string(),
            "shot_count": shot_count,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Human mode: indicatif progress bar (REQ-027)
        use indicatif::{ProgressBar, ProgressStyle};

        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:26}] {pos}%  {prefix}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("##-"),
        );
        pb.set_message("Rendering");

        ar_edit_core::render::render_to_file_with_progress(
            &doc,
            &project_dir,
            &args.output,
            overlay_mode,
            &render_options,
            move |rp| {
                let pct = (rp.progress * 100.0).round() as u64;
                pb.set_position(pct);
                let shot_label = format!("shot {}/{}", rp.shot_index, rp.shot_count);
                let eta_label = match rp.eta_seconds {
                    Some(secs) => {
                        let mins = secs / 60;
                        let s = secs % 60;
                        format!("  ETA {mins:02}:{s:02}")
                    }
                    None => String::new(),
                };
                pb.set_prefix(format!("{shot_label}{eta_label}"));
                if rp.progress >= 1.0 {
                    pb.finish_with_message("Done");
                }
            },
        )
        .map_err(render_err)?;

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
        playback::resolve_source_path(source_id, project_dir).user_err()?;

    let start_ms = if let Some(ref tc) = args.at {
        playback::parse_timecode(tc).user_err()?
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
        .user_err()?;
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
        .user_err()?;
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
    let doc = load_edit(edit_name)?;

    let shot = doc
        .snapshot
        .shots
        .iter()
        .find(|s| s.id == shot_id)
        .ok_or_else(|| {
            anyhow::Error::new(
                CliError::user(format!("shot not found: {shot_id}"))
                    .with_hint(&format!(
                        "run `ar-edit edit show {edit_name}` to list available shots",
                    )),
            )
        })?;

    let source_id = &shot.source;
    let (file, _source) =
        playback::resolve_source_path(source_id, project_dir).user_err()?;

    // Resolve shot range to timestamps
    let range = &shot.range;
    let dir = match range {
        ShotRange::Words { .. } => project_dir.join("transcripts"),
        ShotRange::Scenes { .. } => project_dir.join("index"),
        ShotRange::Time { .. } => project_dir.to_path_buf(),
    };
    let (start_ms, end_ms) =
        ar_edit_core::resolve::resolve_range_from_dir(range, source_id, &dir)
            .user_err()?;

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
    .user_err()?;

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
            .user_err()?;
        vec![doc]
    } else {
        ar_edit_core::marker::list_all_markers(&project_dir)
            .user_err()?
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
            .user_err()?;
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
    .user_err()?;

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
    let mut manifest = ar_edit_core::project::read_manifest(&project_dir).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                "ensure you are inside an ar-edit project directory, or run `ar-edit init <name>` to create one",
            ),
        )
    })?;

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
            .ok_or_else(|| {
                anyhow::Error::new(
                    CliError::user(format!("source not found: {id}"))
                        .with_hint("check available sources in manifest.json"),
                )
            })?;
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

    let index_err = |e: ar_edit_core::index::IndexError| -> anyhow::Error {
        anyhow::Error::new(
            CliError::system(e).with_hint("run `ar-edit doctor` to check dependencies"),
        )
    };

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
                    .map(|h| match h.join() {
                        Ok(result) => result,
                        Err(_) => Err(ar_edit_core::index::IndexError::FfmpegFailed(
                            "indexing thread panicked unexpectedly".to_string(),
                        )),
                    })
                    .collect()
            });

            for result in results {
                let index = result.map_err(index_err)?;
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
            .map_err(index_err)?;

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
    let index = ar_edit_core::index::load_index(&project_dir, source_id).map_err(|e| {
        anyhow::Error::new(
            CliError::user(e).with_hint(
                &format!("run `ar-edit index {source_id}` to index this source"),
            ),
        )
    })?;

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
            .user_err()?;

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
    let doc = ar_edit_core::import::from_transcript(file, output)
        .user_err()?;

    let edits_dir = PathBuf::from("edits");
    std::fs::create_dir_all(&edits_dir)?;

    let path = edits_dir.join(format!("{}.edit.json", doc.name));
    let shot_count = doc.snapshot.shots.len();
    let name = doc.name.clone();

    doc.save(&path).system_err()?;

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
