use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "chirp-rust",
    version,
    about = "Rust migration of Chirp local dictation"
)]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Setup,
    Check,
    Run,
    Listen,
    Record {
        #[arg(long)]
        seconds: Option<f32>,

        #[arg(long)]
        wav: Option<PathBuf>,
    },
    Transcribe {
        #[arg(long)]
        wav: PathBuf,
    },
}
