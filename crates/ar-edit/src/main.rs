use clap::Parser;

#[derive(Parser)]
#[command(name = "ar-edit", about = "Transcript-based video editor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand)]
enum Commands {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    Ok(())
}
