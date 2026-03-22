use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParakeetModelSpec {
    pub model_name: &'static str,
    pub repo_id: &'static str,
    pub quantization: Option<&'static str>,
    pub required_files: &'static [&'static str],
}

impl ParakeetModelSpec {
    pub fn resolve(model_name: &str, quantization: Option<&str>) -> Result<Self> {
        let normalized_quantization = quantization.and_then(|value| {
            let trimmed = value.trim().to_ascii_lowercase();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        match (model_name, normalized_quantization.as_deref()) {
            ("nemo-parakeet-tdt-0.6b-v3", None) => Ok(Self {
                model_name: "nemo-parakeet-tdt-0.6b-v3",
                repo_id: "istupakov/parakeet-tdt-0.6b-v3-onnx",
                quantization: None,
                required_files: &[
                    "config.json",
                    "decoder_joint-model.onnx",
                    "encoder-model.onnx",
                    "encoder-model.onnx.data",
                    "nemo128.onnx",
                    "vocab.txt",
                ],
            }),
            ("nemo-parakeet-tdt-0.6b-v3", Some("int8")) => Ok(Self {
                model_name: "nemo-parakeet-tdt-0.6b-v3",
                repo_id: "smcleod/parakeet-tdt-0.6b-v3-int8",
                quantization: Some("int8"),
                required_files: &[
                    "config.json",
                    "decoder_joint-model.int8.onnx",
                    "encoder-model.int8.onnx",
                    "nemo128.onnx",
                    "vocab.txt",
                ],
            }),
            _ => bail!(
                "unsupported Parakeet model configuration: model={model_name:?}, quantization={quantization:?}"
            ),
        }
    }

    pub fn is_prepared(&self, model_dir: &Path) -> bool {
        self.missing_files(model_dir).is_empty()
    }

    pub fn missing_files(&self, model_dir: &Path) -> Vec<String> {
        self.required_files
            .iter()
            .filter_map(|file_name| {
                let path = model_dir.join(file_name);
                if path.is_file() {
                    None
                } else {
                    Some((*file_name).to_string())
                }
            })
            .collect()
    }

    pub fn ensure_downloaded(&self, model_dir: &Path) -> Result<Vec<PathBuf>> {
        fs::create_dir_all(model_dir)
            .with_context(|| format!("failed to create model directory {}", model_dir.display()))?;

        let client = Client::builder()
            .user_agent("chirp-rust")
            .build()
            .context("failed to initialize HTTP client")?;
        let mut materialized = Vec::with_capacity(self.required_files.len());

        for file_name in self.required_files {
            let target = model_dir.join(file_name);
            download_if_needed(&client, self.repo_id, file_name, &target)?;
            materialized.push(target);
        }

        Ok(materialized)
    }
}

fn download_if_needed(
    client: &Client,
    repo_id: &str,
    file_name: &str,
    target: &Path,
) -> Result<()> {
    if target.is_file() {
        return Ok(());
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create parent directory for {}", target.display())
        })?;
    }

    let url = format!("https://huggingface.co/{repo_id}/resolve/main/{file_name}?download=true");
    let mut response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to request {url}"))?
        .error_for_status()
        .with_context(|| format!("failed to download {file_name} from {repo_id}"))?;

    let temp_path = target.with_extension("part");
    let mut temp_file = fs::File::create(&temp_path)
        .with_context(|| format!("failed to create temporary file {}", temp_path.display()))?;
    response
        .copy_to(&mut temp_file)
        .with_context(|| format!("failed to stream {file_name} into {}", temp_path.display()))?;
    temp_file
        .flush()
        .with_context(|| format!("failed to flush {}", temp_path.display()))?;
    fs::rename(&temp_path, target).with_context(|| {
        format!(
            "failed to move downloaded file from {} to {}",
            temp_path.display(),
            target.display()
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_fp_model_spec() {
        let spec = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", None).unwrap();
        assert_eq!(spec.repo_id, "istupakov/parakeet-tdt-0.6b-v3-onnx");
        assert!(spec.required_files.contains(&"encoder-model.onnx.data"));
    }

    #[test]
    fn resolves_int8_model_spec() {
        let spec = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", Some("int8")).unwrap();
        assert_eq!(spec.repo_id, "smcleod/parakeet-tdt-0.6b-v3-int8");
        assert!(spec.required_files.contains(&"encoder-model.int8.onnx"));
    }

    #[test]
    fn rejects_unknown_model_spec() {
        let error = ParakeetModelSpec::resolve("unknown", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported Parakeet model configuration"));
    }
}
