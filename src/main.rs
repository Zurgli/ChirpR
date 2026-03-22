use anyhow::Result;
use chirp_rust::cli::{Cli, Command};
use chirp_rust::config::{ChirpConfig, ProjectPaths};
use chirp_rust::text_processing::TextProcessor;
use clap::Parser;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut paths = ProjectPaths::discover()?;
    if let Some(config_path) = cli.config {
        paths = paths.with_config_path(config_path);
    }

    match cli.command.unwrap_or(Command::Check) {
        Command::Setup => {
            paths.ensure_models_root()?;
            println!("models directory ready at {}", paths.models_root.display());
        }
        Command::Check => {
            let config = ChirpConfig::load(&paths)?;
            let model_dir = paths.model_dir(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let processor =
                TextProcessor::new(config.word_overrides.clone(), &config.post_processing);
            let processed_sample = processor.process("test");
            println!("config OK");
            println!("model dir: {}", model_dir.display());
            println!("processed sample: {processed_sample}");
            if cli.verbose {
                println!("config: {config:#?}");
            }
        }
    }

    Ok(())
}
