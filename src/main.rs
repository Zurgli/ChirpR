use anyhow::Result;
use chirp_rust::cli::{Cli, Command};
use chirp_rust::config::{ChirpConfig, ProjectPaths};
use chirp_rust::stt::parakeet::ParakeetModelSpec;
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
            let config = ChirpConfig::load(&paths)?;
            paths.ensure_models_root()?;
            let model_dir = paths.model_dir(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let spec = ParakeetModelSpec::resolve(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let downloaded_files = spec.ensure_downloaded(&model_dir)?;
            println!("model ready at {}", model_dir.display());
            println!("downloaded {} required files", downloaded_files.len());
        }
        Command::Check => {
            let config = ChirpConfig::load(&paths)?;
            let model_dir = paths.model_dir(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let spec = ParakeetModelSpec::resolve(
                &config.parakeet_model,
                config.parakeet_quantization.as_deref(),
            )?;
            let missing_files = spec.missing_files(&model_dir);
            let processor =
                TextProcessor::new(config.word_overrides.clone(), &config.post_processing);
            let processed_sample = processor.process("test");
            println!("config OK");
            println!("model dir: {}", model_dir.display());
            if missing_files.is_empty() {
                println!("model files: ready");
                spec.create_manager(&model_dir)?;
                println!("onnx sessions: ready");
            } else {
                println!("model files missing: {}", missing_files.join(", "));
            }
            println!("processed sample: {processed_sample}");
            if cli.verbose {
                println!("config: {config:#?}");
            }
        }
    }

    Ok(())
}
