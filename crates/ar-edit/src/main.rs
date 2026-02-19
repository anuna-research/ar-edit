mod cli;

use clap::Parser;
use cli::{
    exit_code, Cli, Commands, EditCommand, IndexCommand, SchemaCommand, TranscriptsCommand,
};
use std::process;

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
            EditCommand::History { edit } => todo!("edit history: {edit}"),
            EditCommand::FromTranscript { file, output } => {
                todo!("edit from-transcript: {file:?}, output={output:?}")
            }
            EditCommand::Note { edit, shot, text } => {
                todo!("edit note: {edit}, shot={shot}, text={text}")
            }
        },
        Commands::Undo { edit } => todo!("undo: {edit}"),
        Commands::Redo { edit } => todo!("redo: {edit}"),
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
    }
}
