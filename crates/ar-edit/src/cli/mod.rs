use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

// ---------------------------------------------------------------------------
// Exit codes (CON-009)
// ---------------------------------------------------------------------------

pub mod exit_code {
    pub const SUCCESS: i32 = 0;
    pub const USER_ERROR: i32 = 1;
    pub const SYSTEM_ERROR: i32 = 2;
    pub const VALIDATION_ERROR: i32 = 3;
}

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "ar-edit",
    about = "Transcript-based video editor",
    version,
    after_help = "Repository: https://codeberg.org/anuna/ar-edit"
)]
pub struct Cli {
    /// Output structured JSON instead of human-readable text
    #[arg(long, global = true)]
    pub json: bool,

    /// Show verbose diagnostic output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Preview what would be done without making changes
    #[arg(short = 'n', long, global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Subcommands for an active collaborative session (SPEC-003 CON-012).
#[derive(Subcommand)]
pub enum SessionCommand {
    /// Show session and sync status
    Status,
    /// List connected peers
    Peers,
    /// Leave the current session
    Leave,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Open this project for realtime collaboration; prints a pairing phrase (SPEC-003)
    Share,

    /// Join a collaborative session with a pairing phrase (SPEC-003)
    Pair {
        /// Pairing phrase in the form <num>-<word>-<word>
        phrase: String,
    },

    /// Inspect or leave the current collaborative session (SPEC-003)
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },

    /// Run the collaborative session daemon for this project (SPEC-003)
    Daemon {
        /// Edit document to host (in-memory session for now; persistence is OQ-8)
        #[arg(long)]
        edit: Option<String>,
    },

    /// Create a new project directory
    Init {
        /// Project name (becomes the directory name)
        name: String,
    },

    /// Register source video files
    Add {
        /// Paths to video files
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },

    /// Check runtime dependencies (ffmpeg, whisper, vlc)
    Doctor,

    /// Transcribe source videos using whisper.cpp
    Transcribe(TranscribeArgs),

    /// Transcript query operations
    Transcripts {
        #[command(subcommand)]
        command: TranscriptsCommand,
    },

    /// Edit document operations
    Edit {
        #[command(subcommand)]
        command: EditCommand,
    },

    /// Undo the last operation on an edit document
    Undo {
        /// Edit document name
        edit: String,
    },

    /// Redo the last undone operation on an edit document
    Redo {
        /// Edit document name
        edit: String,
    },

    /// Validate an edit document against project sources
    Validate {
        /// Edit document name
        edit: String,
    },

    /// Play an edit or source in VLC/ffplay
    Play(PlayArgs),

    /// Render an edit document to a video file
    Render(RenderArgs),

    /// Source indexing (scene detection and thumbnails)
    Index(IndexArgs),

    /// Search across transcripts and scene descriptions
    Search(SearchArgs),

    /// Add a marker to a source
    Mark(MarkArgs),

    /// List markers for a source (or all sources)
    Markers {
        /// Source ID (omit to list across all sources)
        source_id: Option<String>,

        /// Filter by label
        #[arg(long)]
        label: Option<String>,
    },

    /// Output JSON Schema for data formats
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },

    /// Launch the interactive terminal UI
    Tui,

    /// Generate shell completions
    Completions {
        /// Target shell
        shell: Shell,
    },
}

impl Cli {
    /// Write shell completions to stdout.
    pub fn print_completions(shell: Shell) {
        clap_complete::generate(
            shell,
            &mut Cli::command(),
            "ar-edit",
            &mut std::io::stdout(),
        );
    }
}

// ---------------------------------------------------------------------------
// Shared: range flags (word / scene / time)
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct RangeArgs {
    /// Start word index
    #[arg(long, allow_hyphen_values = true, requires = "to_word", conflicts_with_all = ["from_scene", "to_scene", "from_ms", "to_ms"])]
    pub from_word: Option<i32>,

    /// End word index
    #[arg(long, allow_hyphen_values = true, requires = "from_word", conflicts_with_all = ["from_scene", "to_scene", "from_ms", "to_ms"])]
    pub to_word: Option<i32>,

    /// Start scene index
    #[arg(long, allow_hyphen_values = true, requires = "to_scene", conflicts_with_all = ["from_word", "to_word", "from_ms", "to_ms"])]
    pub from_scene: Option<i32>,

    /// End scene index
    #[arg(long, allow_hyphen_values = true, requires = "from_scene", conflicts_with_all = ["from_word", "to_word", "from_ms", "to_ms"])]
    pub to_scene: Option<i32>,

    /// Start time in milliseconds
    #[arg(long, allow_hyphen_values = true, requires = "to_ms", conflicts_with_all = ["from_word", "to_word", "from_scene", "to_scene"])]
    pub from_ms: Option<i64>,

    /// End time in milliseconds
    #[arg(long, allow_hyphen_values = true, requires = "from_ms", conflicts_with_all = ["from_word", "to_word", "from_scene", "to_scene"])]
    pub to_ms: Option<i64>,
}

// ---------------------------------------------------------------------------
// Transcribe (CON-002)
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct TranscribeArgs {
    /// Source ID to transcribe (omit when using --all)
    pub source_id: Option<String>,

    /// Transcribe all un-transcribed sources
    #[arg(long)]
    pub all: bool,

    /// Re-transcribe even if already transcribed
    #[arg(long)]
    pub force: bool,

    /// Whisper model name (e.g. tiny, base, small, medium, large)
    #[arg(long)]
    pub model: Option<String>,

    /// Number of parallel transcription workers
    #[arg(long)]
    pub parallel: Option<usize>,

    /// Import an existing SRT, VTT, or whisper JSON transcript
    #[arg(long, value_name = "FILE")]
    pub import: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Transcripts (CON-003)
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
pub enum TranscriptsCommand {
    /// List all transcripts
    List,

    /// Read a transcript as plain text
    Read {
        /// Source ID
        source_id: String,

        /// Interleave markers at their timestamp positions
        #[arg(long)]
        with_markers: bool,
    },

    /// Search within transcripts
    Search {
        /// Search query
        query: String,

        /// Restrict search to a specific source
        #[arg(long)]
        source: Option<String>,
    },

    /// Export transcript in an editable format
    Export {
        /// Output format
        #[arg(long, default_value = "editable")]
        format: String,

        /// Output file (stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Edit (CON-004)
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
pub enum EditCommand {
    /// Create a new empty edit document
    Create {
        /// Edit document name
        name: String,
    },

    /// Add a segment (shot) to an edit
    AddSegment(AddSegmentArgs),

    /// Move a segment to a new position
    MoveSegment {
        /// Edit document name
        edit: String,

        /// Shot ID to move
        #[arg(long)]
        shot: String,

        /// Target position (0-based)
        #[arg(long)]
        position: u32,
    },

    /// Remove a segment from an edit
    RemoveSegment {
        /// Edit document name
        edit: String,

        /// Shot ID to remove
        #[arg(long)]
        shot: String,
    },

    /// Trim a segment's range
    TrimSegment(TrimSegmentArgs),

    /// Show the current state of an edit document
    Show {
        /// Edit document name
        edit: String,
    },

    /// Show the operation history of an edit
    History {
        /// Edit document name
        edit: String,
    },

    /// Create an edit from an annotated transcript file
    FromTranscript {
        /// Path to annotated markdown file
        file: PathBuf,

        /// Output edit name (derived from filename if omitted)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Add a note to a shot
    Note {
        /// Edit document name
        edit: String,

        /// Shot ID
        #[arg(long)]
        shot: String,

        /// Note text
        #[arg(long)]
        text: String,
    },
}

#[derive(Args)]
pub struct AddSegmentArgs {
    /// Edit document name
    pub edit: String,

    /// Source ID
    #[arg(long)]
    pub source: String,

    #[command(flatten)]
    pub range: RangeArgs,
}

#[derive(Args)]
pub struct TrimSegmentArgs {
    /// Edit document name
    pub edit: String,

    /// Shot ID to trim
    #[arg(long)]
    pub shot: String,

    #[command(flatten)]
    pub range: RangeArgs,
}

// ---------------------------------------------------------------------------
// Play (CON-006)
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct PlayArgs {
    /// Edit name or source ID to play
    pub target: String,

    /// Show overlay (optionally "minimal" for reduced info)
    #[arg(long, num_args = 0..=1, default_missing_value = "full")]
    pub overlay: Option<String>,

    /// Play only a specific shot (edit playback)
    #[arg(long)]
    pub shot: Option<String>,

    /// Start at timecode (source playback, e.g. 00:01:30)
    #[arg(long)]
    pub at: Option<String>,

    /// Start at word index (source playback)
    #[arg(long)]
    pub at_word: Option<u32>,

    /// Start at scene index (source playback)
    #[arg(long)]
    pub at_scene: Option<u32>,

    /// Preview resolution (e.g. "1280x720"); defaults to 720p
    #[arg(long)]
    pub resolution: Option<String>,
}

// ---------------------------------------------------------------------------
// Render (CON-007)
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct RenderArgs {
    /// Edit document name
    pub edit: String,

    /// Output file path
    #[arg(short, long)]
    pub output: PathBuf,

    /// Embed subtitles from transcripts
    #[arg(long)]
    pub subtitles: bool,

    /// Burn overlay into the video
    #[arg(long)]
    pub burn_overlay: bool,

    /// Video codec (e.g. h264, h265)
    #[arg(long)]
    pub codec: Option<String>,

    /// Output resolution (e.g. 1920x1080)
    #[arg(long)]
    pub resolution: Option<String>,
}

// ---------------------------------------------------------------------------
// Index (CON-008)
// ---------------------------------------------------------------------------

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub struct IndexArgs {
    #[command(subcommand)]
    pub command: Option<IndexCommand>,

    #[command(flatten)]
    pub run: IndexRunArgs,
}

#[derive(Args)]
pub struct IndexRunArgs {
    /// Source ID to index
    pub source_id: Option<String>,

    /// Index all un-indexed sources
    #[arg(long)]
    pub all: bool,

    /// Generate description placeholders
    #[arg(long)]
    pub describe: bool,

    /// Thumbnail extraction interval in seconds
    #[arg(long)]
    pub interval: Option<f64>,

    /// Number of parallel workers
    #[arg(long)]
    pub parallel: Option<usize>,
}

#[derive(Subcommand)]
pub enum IndexCommand {
    /// Display the scene index for a source
    Show {
        /// Source ID
        source_id: String,
    },

    /// Set a scene description
    SetDescription {
        /// Source ID
        source_id: String,

        /// Scene index (0-based)
        #[arg(long)]
        scene: u32,

        /// Description text
        #[arg(long)]
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Search (CON-008)
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Restrict to a specific source
    #[arg(long)]
    pub source: Option<String>,

    /// Filter by match type
    #[arg(long, value_name = "TYPE")]
    pub r#type: Option<SearchType>,
}

#[derive(Clone, ValueEnum)]
pub enum SearchType {
    Transcript,
    Scene,
    Metadata,
}

// ---------------------------------------------------------------------------
// Mark / Markers
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct MarkArgs {
    /// Source ID
    pub source_id: String,

    /// Marker label (e.g. select, avoid, review)
    #[arg(long)]
    pub label: String,

    /// Optional note
    #[arg(long)]
    pub note: Option<String>,

    #[command(flatten)]
    pub range: RangeArgs,
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
pub enum SchemaCommand {
    /// Output the JSON Schema for edit documents
    Edit,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_init() {
        let cli = Cli::try_parse_from(["ar-edit", "init", "my-project"]).unwrap();
        assert!(!cli.json);
        assert!(matches!(cli.command, Commands::Init { ref name } if name == "my-project"));
    }

    #[test]
    fn cli_parses_global_json_flag() {
        let cli = Cli::try_parse_from(["ar-edit", "--json", "doctor"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Commands::Doctor));
    }

    #[test]
    fn cli_parses_add_multiple_files() {
        let cli = Cli::try_parse_from(["ar-edit", "add", "a.mp4", "b.mp4"]).unwrap();
        match cli.command {
            Commands::Add { ref files } => assert_eq!(files.len(), 2),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn cli_parses_transcribe_single() {
        let cli =
            Cli::try_parse_from(["ar-edit", "transcribe", "src-001", "--model", "base"]).unwrap();
        match cli.command {
            Commands::Transcribe(ref args) => {
                assert_eq!(args.source_id.as_deref(), Some("src-001"));
                assert_eq!(args.model.as_deref(), Some("base"));
                assert!(!args.all);
            }
            _ => panic!("expected Transcribe"),
        }
    }

    #[test]
    fn cli_parses_transcribe_all() {
        let cli =
            Cli::try_parse_from(["ar-edit", "transcribe", "--all", "--parallel", "4"]).unwrap();
        match cli.command {
            Commands::Transcribe(ref args) => {
                assert!(args.all);
                assert_eq!(args.parallel, Some(4));
                assert!(args.source_id.is_none());
            }
            _ => panic!("expected Transcribe"),
        }
    }

    #[test]
    fn cli_parses_transcripts_read_with_markers() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "transcripts",
            "read",
            "src-001",
            "--with-markers",
        ])
        .unwrap();
        match cli.command {
            Commands::Transcripts {
                command:
                    TranscriptsCommand::Read {
                        ref source_id,
                        with_markers,
                    },
            } => {
                assert_eq!(source_id, "src-001");
                assert!(with_markers);
            }
            _ => panic!("expected Transcripts Read"),
        }
    }

    #[test]
    fn cli_parses_transcripts_read_without_markers() {
        let cli = Cli::try_parse_from(["ar-edit", "transcripts", "read", "src-001"]).unwrap();
        match cli.command {
            Commands::Transcripts {
                command:
                    TranscriptsCommand::Read {
                        ref source_id,
                        with_markers,
                    },
            } => {
                assert_eq!(source_id, "src-001");
                assert!(!with_markers);
            }
            _ => panic!("expected Transcripts Read"),
        }
    }

    #[test]
    fn cli_parses_transcripts_search() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "transcripts",
            "search",
            "climate policy",
            "--source",
            "src-001",
        ])
        .unwrap();
        match cli.command {
            Commands::Transcripts {
                command:
                    TranscriptsCommand::Search {
                        ref query,
                        ref source,
                    },
            } => {
                assert_eq!(query, "climate policy");
                assert_eq!(source.as_deref(), Some("src-001"));
            }
            _ => panic!("expected Transcripts Search"),
        }
    }

    #[test]
    fn cli_parses_edit_create() {
        let cli = Cli::try_parse_from(["ar-edit", "edit", "create", "rough-cut"]).unwrap();
        match cli.command {
            Commands::Edit {
                command: EditCommand::Create { ref name },
            } => assert_eq!(name, "rough-cut"),
            _ => panic!("expected Edit Create"),
        }
    }

    #[test]
    fn cli_parses_edit_add_segment_words() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "add-segment",
            "rough-cut",
            "--source",
            "src-001",
            "--from-word",
            "0",
            "--to-word",
            "52",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command: EditCommand::AddSegment(ref args),
            } => {
                assert_eq!(args.edit, "rough-cut");
                assert_eq!(args.source, "src-001");
                assert_eq!(args.range.from_word, Some(0));
                assert_eq!(args.range.to_word, Some(52));
                assert!(args.range.from_scene.is_none());
                assert!(args.range.from_ms.is_none());
            }
            _ => panic!("expected Edit AddSegment"),
        }
    }

    #[test]
    fn cli_parses_edit_move_segment() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "move-segment",
            "rough-cut",
            "--shot",
            "shot-003",
            "--position",
            "0",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command:
                    EditCommand::MoveSegment {
                        ref edit,
                        ref shot,
                        position,
                    },
            } => {
                assert_eq!(edit, "rough-cut");
                assert_eq!(shot, "shot-003");
                assert_eq!(position, 0);
            }
            _ => panic!("expected Edit MoveSegment"),
        }
    }

    #[test]
    fn cli_parses_edit_trim_segment_scenes() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "trim-segment",
            "rough-cut",
            "--shot",
            "shot-001",
            "--from-scene",
            "1",
            "--to-scene",
            "3",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command: EditCommand::TrimSegment(ref args),
            } => {
                assert_eq!(args.shot, "shot-001");
                assert_eq!(args.range.from_scene, Some(1));
                assert_eq!(args.range.to_scene, Some(3));
            }
            _ => panic!("expected Edit TrimSegment"),
        }
    }

    #[test]
    fn cli_rejects_mixed_range_types() {
        let result = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "add-segment",
            "rough-cut",
            "--source",
            "src-001",
            "--from-word",
            "0",
            "--to-word",
            "52",
            "--from-scene",
            "1",
            "--to-scene",
            "3",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_parses_undo_redo() {
        let cli = Cli::try_parse_from(["ar-edit", "undo", "rough-cut"]).unwrap();
        assert!(matches!(cli.command, Commands::Undo { ref edit } if edit == "rough-cut"));

        let cli = Cli::try_parse_from(["ar-edit", "redo", "rough-cut"]).unwrap();
        assert!(matches!(cli.command, Commands::Redo { ref edit } if edit == "rough-cut"));
    }

    #[test]
    fn cli_parses_validate() {
        let cli = Cli::try_parse_from(["ar-edit", "validate", "rough-cut"]).unwrap();
        assert!(matches!(cli.command, Commands::Validate { ref edit } if edit == "rough-cut"));
    }

    #[test]
    fn cli_parses_play_edit() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "play",
            "rough-cut",
            "--overlay",
            "minimal",
            "--shot",
            "shot-003",
        ])
        .unwrap();
        match cli.command {
            Commands::Play(ref args) => {
                assert_eq!(args.target, "rough-cut");
                assert_eq!(args.overlay.as_deref(), Some("minimal"));
                assert_eq!(args.shot.as_deref(), Some("shot-003"));
            }
            _ => panic!("expected Play"),
        }
    }

    #[test]
    fn cli_parses_play_source_at_word() {
        let cli = Cli::try_parse_from(["ar-edit", "play", "src-001", "--at-word", "85"]).unwrap();
        match cli.command {
            Commands::Play(ref args) => {
                assert_eq!(args.target, "src-001");
                assert_eq!(args.at_word, Some(85));
            }
            _ => panic!("expected Play"),
        }
    }

    #[test]
    fn cli_parses_render() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "render",
            "rough-cut",
            "-o",
            "final.mp4",
            "--subtitles",
            "--codec",
            "h265",
            "--resolution",
            "1920x1080",
        ])
        .unwrap();
        match cli.command {
            Commands::Render(ref args) => {
                assert_eq!(args.edit, "rough-cut");
                assert_eq!(args.output, PathBuf::from("final.mp4"));
                assert!(args.subtitles);
                assert_eq!(args.codec.as_deref(), Some("h265"));
                assert_eq!(args.resolution.as_deref(), Some("1920x1080"));
            }
            _ => panic!("expected Render"),
        }
    }

    #[test]
    fn cli_parses_render_minimal() {
        let cli =
            Cli::try_parse_from(["ar-edit", "render", "rough-cut", "-o", "output.mp4"]).unwrap();
        match cli.command {
            Commands::Render(ref args) => {
                assert_eq!(args.edit, "rough-cut");
                assert_eq!(args.output, PathBuf::from("output.mp4"));
                assert!(!args.subtitles);
                assert!(!args.burn_overlay);
                assert!(args.codec.is_none());
                assert!(args.resolution.is_none());
            }
            _ => panic!("expected Render"),
        }
    }

    #[test]
    fn cli_parses_render_burn_overlay() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "render",
            "rough-cut",
            "-o",
            "output.mp4",
            "--burn-overlay",
        ])
        .unwrap();
        match cli.command {
            Commands::Render(ref args) => {
                assert!(args.burn_overlay);
            }
            _ => panic!("expected Render"),
        }
    }

    #[test]
    fn cli_parses_index_source() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "index",
            "src-001",
            "--describe",
            "--interval",
            "5.0",
        ])
        .unwrap();
        match cli.command {
            Commands::Index(ref args) => {
                assert!(args.command.is_none());
                assert_eq!(args.run.source_id.as_deref(), Some("src-001"));
                assert!(args.run.describe);
                assert_eq!(args.run.interval, Some(5.0));
            }
            _ => panic!("expected Index"),
        }
    }

    #[test]
    fn cli_parses_index_show() {
        let cli = Cli::try_parse_from(["ar-edit", "index", "show", "src-001"]).unwrap();
        match cli.command {
            Commands::Index(ref args) => {
                assert!(matches!(
                    args.command,
                    Some(IndexCommand::Show { ref source_id }) if source_id == "src-001"
                ));
            }
            _ => panic!("expected Index Show"),
        }
    }

    #[test]
    fn cli_parses_index_set_description() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "index",
            "set-description",
            "src-001",
            "--scene",
            "2",
            "--text",
            "Aerial shot of coastline",
        ])
        .unwrap();
        match cli.command {
            Commands::Index(ref args) => match args.command {
                Some(IndexCommand::SetDescription {
                    ref source_id,
                    scene,
                    ref text,
                }) => {
                    assert_eq!(source_id, "src-001");
                    assert_eq!(scene, 2);
                    assert_eq!(text, "Aerial shot of coastline");
                }
                _ => panic!("expected SetDescription"),
            },
            _ => panic!("expected Index"),
        }
    }

    #[test]
    fn cli_parses_search() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "search",
            "climate policy",
            "--source",
            "src-001",
            "--type",
            "transcript",
        ])
        .unwrap();
        match cli.command {
            Commands::Search(ref args) => {
                assert_eq!(args.query, "climate policy");
                assert_eq!(args.source.as_deref(), Some("src-001"));
                assert!(matches!(args.r#type, Some(SearchType::Transcript)));
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn cli_parses_mark() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "mark",
            "src-001",
            "--label",
            "select",
            "--from-word",
            "45",
            "--to-word",
            "120",
            "--note",
            "Best take",
        ])
        .unwrap();
        match cli.command {
            Commands::Mark(ref args) => {
                assert_eq!(args.source_id, "src-001");
                assert_eq!(args.label, "select");
                assert_eq!(args.range.from_word, Some(45));
                assert_eq!(args.range.to_word, Some(120));
                assert_eq!(args.note.as_deref(), Some("Best take"));
            }
            _ => panic!("expected Mark"),
        }
    }

    #[test]
    fn cli_parses_markers_with_source() {
        let cli = Cli::try_parse_from(["ar-edit", "markers", "src-001"]).unwrap();
        match cli.command {
            Commands::Markers {
                ref source_id,
                ref label,
            } => {
                assert_eq!(source_id.as_deref(), Some("src-001"));
                assert!(label.is_none());
            }
            _ => panic!("expected Markers"),
        }
    }

    #[test]
    fn cli_parses_markers_all_sources() {
        let cli = Cli::try_parse_from(["ar-edit", "markers"]).unwrap();
        match cli.command {
            Commands::Markers {
                ref source_id,
                ref label,
            } => {
                assert!(source_id.is_none());
                assert!(label.is_none());
            }
            _ => panic!("expected Markers"),
        }
    }

    #[test]
    fn cli_parses_markers_with_label_filter() {
        let cli =
            Cli::try_parse_from(["ar-edit", "markers", "src-001", "--label", "select"]).unwrap();
        match cli.command {
            Commands::Markers {
                ref source_id,
                ref label,
            } => {
                assert_eq!(source_id.as_deref(), Some("src-001"));
                assert_eq!(label.as_deref(), Some("select"));
            }
            _ => panic!("expected Markers"),
        }
    }

    #[test]
    fn cli_parses_edit_note() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "note",
            "rough-cut",
            "--shot",
            "shot-001",
            "--text",
            "Too long, trim the first half",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command:
                    EditCommand::Note {
                        ref edit,
                        ref shot,
                        ref text,
                    },
            } => {
                assert_eq!(edit, "rough-cut");
                assert_eq!(shot, "shot-001");
                assert_eq!(text, "Too long, trim the first half");
            }
            _ => panic!("expected Edit Note"),
        }
    }

    #[test]
    fn cli_parses_schema_edit() {
        let cli = Cli::try_parse_from(["ar-edit", "schema", "edit"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Schema {
                command: SchemaCommand::Edit
            }
        ));
    }

    #[test]
    fn cli_parses_tui() {
        let cli = Cli::try_parse_from(["ar-edit", "tui"]).unwrap();
        assert!(matches!(cli.command, Commands::Tui));
    }

    #[test]
    fn cli_parses_completions() {
        let cli = Cli::try_parse_from(["ar-edit", "completions", "bash"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Completions { shell: Shell::Bash }
        ));

        let cli = Cli::try_parse_from(["ar-edit", "completions", "zsh"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Completions { shell: Shell::Zsh }
        ));

        let cli = Cli::try_parse_from(["ar-edit", "completions", "fish"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Completions { shell: Shell::Fish }
        ));
    }

    #[test]
    fn cli_parses_transcripts_export_default() {
        let cli = Cli::try_parse_from(["ar-edit", "transcripts", "export"]).unwrap();
        match cli.command {
            Commands::Transcripts {
                command:
                    TranscriptsCommand::Export {
                        ref format,
                        ref output,
                    },
            } => {
                assert_eq!(format, "editable");
                assert!(output.is_none());
            }
            _ => panic!("expected Transcripts Export"),
        }
    }

    #[test]
    fn cli_parses_transcripts_export_with_output() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "transcripts",
            "export",
            "--format",
            "editable",
            "-o",
            "draft.md",
        ])
        .unwrap();
        match cli.command {
            Commands::Transcripts {
                command:
                    TranscriptsCommand::Export {
                        ref format,
                        ref output,
                    },
            } => {
                assert_eq!(format, "editable");
                assert_eq!(output.as_deref(), Some(std::path::Path::new("draft.md")));
            }
            _ => panic!("expected Transcripts Export"),
        }
    }

    #[test]
    fn exit_codes_match_con_009() {
        assert_eq!(exit_code::SUCCESS, 0);
        assert_eq!(exit_code::USER_ERROR, 1);
        assert_eq!(exit_code::SYSTEM_ERROR, 2);
        assert_eq!(exit_code::VALIDATION_ERROR, 3);
    }

    #[test]
    fn cli_accepts_negative_ms_values() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "add-segment",
            "rough-cut",
            "--source",
            "src-001",
            "--from-ms",
            "-100",
            "--to-ms",
            "5000",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command: EditCommand::AddSegment(ref args),
            } => {
                assert_eq!(args.range.from_ms, Some(-100));
                assert_eq!(args.range.to_ms, Some(5000));
            }
            _ => panic!("expected Edit AddSegment"),
        }
    }

    #[test]
    fn cli_accepts_negative_word_values() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "add-segment",
            "rough-cut",
            "--source",
            "src-001",
            "--from-word",
            "-5",
            "--to-word",
            "10",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command: EditCommand::AddSegment(ref args),
            } => {
                assert_eq!(args.range.from_word, Some(-5));
                assert_eq!(args.range.to_word, Some(10));
            }
            _ => panic!("expected Edit AddSegment"),
        }
    }

    #[test]
    fn cli_accepts_negative_scene_values() {
        let cli = Cli::try_parse_from([
            "ar-edit",
            "edit",
            "add-segment",
            "rough-cut",
            "--source",
            "src-001",
            "--from-scene",
            "-1",
            "--to-scene",
            "3",
        ])
        .unwrap();
        match cli.command {
            Commands::Edit {
                command: EditCommand::AddSegment(ref args),
            } => {
                assert_eq!(args.range.from_scene, Some(-1));
                assert_eq!(args.range.to_scene, Some(3));
            }
            _ => panic!("expected Edit AddSegment"),
        }
    }
}
