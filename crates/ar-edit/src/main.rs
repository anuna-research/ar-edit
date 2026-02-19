mod cli;

use std::path::PathBuf;
use std::process;

use ar_edit_core::models::{EditDocument, EditOpKind};
use clap::Parser;
use cli::{
    exit_code, Cli, Commands, EditCommand, IndexCommand, SchemaCommand, TranscriptsCommand,
};

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
            EditCommand::Show { edit } => todo!("edit show: {edit}"),
            EditCommand::History { edit } => cmd_history(cli, edit),
            EditCommand::FromTranscript { file, output } => {
                todo!("edit from-transcript: {file:?}, output={output:?}")
            }
            EditCommand::Note { edit, shot, text } => {
                todo!("edit note: {edit}, shot={shot}, text={text}")
            }
        },
        Commands::Undo { edit } => cmd_undo(cli, edit),
        Commands::Redo { edit } => cmd_redo(cli, edit),
        Commands::Validate { edit } => todo!("validate: {edit}"),
        Commands::Play(args) => todo!("play: target={}", args.target),
        Commands::Render(args) => todo!("render: edit={}, output={:?}", args.edit, args.output),
        Commands::Index(args) => match &args.command {
            Some(IndexCommand::Show { source_id }) => todo!("index show: {source_id}"),
            Some(IndexCommand::SetDescription {
                source_id,
                scene,
                text,
            }) => todo!("index set-description: {source_id}, scene={scene}, text={text}"),
            None => todo!(
                "index run: source_id={:?}, all={}",
                args.run.source_id,
                args.run.all
            ),
        },
        Commands::Search(args) => todo!("search: {}", args.query),
        Commands::Mark(args) => todo!("mark: source={}, label={}", args.source_id, args.label),
        Commands::Markers { source_id } => todo!("markers: {source_id}"),
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

