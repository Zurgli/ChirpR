use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ort::{session::Session, value::ValueType};
use reqwest::blocking::Client;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParakeetModelSpec {
    pub model_name: &'static str,
    pub repo_id: &'static str,
    pub quantization: Option<&'static str>,
    pub required_files: &'static [&'static str],
}

#[derive(Debug)]
pub struct ParakeetManager {
    model_dir: PathBuf,
    spec: ParakeetModelSpec,
    sessions: Option<ParakeetSessions>,
    timeout: Option<Duration>,
    last_access: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIoSummary {
    pub label: &'static str,
    pub inputs: Vec<OutletSummary>,
    pub outputs: Vec<OutletSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutletSummary {
    pub name: String,
    pub dtype: String,
}

#[derive(Debug)]
struct ParakeetSessions {
    _encoder_session: Session,
    _decoder_session: Session,
    _feature_session: Session,
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

    pub fn create_manager(&self, model_dir: &Path) -> Result<ParakeetManager> {
        ParakeetManager::new(
            model_dir.to_path_buf(),
            self.clone(),
            Some(Duration::from_secs(300)),
        )
    }

    fn encoder_file_name(&self) -> &'static str {
        match self.quantization {
            Some("int8") => "encoder-model.int8.onnx",
            _ => "encoder-model.onnx",
        }
    }

    fn decoder_file_name(&self) -> &'static str {
        match self.quantization {
            Some("int8") => "decoder_joint-model.int8.onnx",
            _ => "decoder_joint-model.onnx",
        }
    }
}

impl ParakeetManager {
    pub fn new(
        model_dir: PathBuf,
        spec: ParakeetModelSpec,
        timeout: Option<Duration>,
    ) -> Result<Self> {
        let mut manager = Self {
            model_dir,
            spec,
            sessions: None,
            timeout,
            last_access: Instant::now(),
        };
        manager.ensure_loaded()?;
        Ok(manager)
    }

    pub fn is_loaded(&self) -> bool {
        self.sessions.is_some()
    }

    pub fn ensure_prepared(&self) -> Result<()> {
        let missing_files = self.spec.missing_files(&self.model_dir);
        if missing_files.is_empty() {
            Ok(())
        } else {
            bail!(
                "model is not prepared at {}. missing files: {}",
                self.model_dir.display(),
                missing_files.join(", ")
            );
        }
    }

    pub fn ensure_loaded(&mut self) -> Result<()> {
        self.ensure_prepared()?;
        self.last_access = Instant::now();
        if self.sessions.is_none() {
            self.sessions = Some(self.load_sessions()?);
        }
        Ok(())
    }

    pub fn unload(&mut self) {
        self.sessions = None;
    }

    pub fn maybe_unload(&mut self) {
        if let Some(timeout) = self.timeout {
            if self.last_access.elapsed() > timeout {
                self.sessions = None;
            }
        }
    }

    pub fn describe(&self) -> Vec<SessionIoSummary> {
        let Some(sessions) = &self.sessions else {
            return Vec::new();
        };

        vec![
            SessionIoSummary {
                label: "encoder",
                inputs: summarize_outlets(sessions._encoder_session.inputs()),
                outputs: summarize_outlets(sessions._encoder_session.outputs()),
            },
            SessionIoSummary {
                label: "decoder",
                inputs: summarize_outlets(sessions._decoder_session.inputs()),
                outputs: summarize_outlets(sessions._decoder_session.outputs()),
            },
            SessionIoSummary {
                label: "nemo128",
                inputs: summarize_outlets(sessions._feature_session.inputs()),
                outputs: summarize_outlets(sessions._feature_session.outputs()),
            },
        ]
    }

    fn load_sessions(&self) -> Result<ParakeetSessions> {
        let _ = ort::init().commit();

        let encoder_path = self.model_dir.join(self.spec.encoder_file_name());
        let decoder_path = self.model_dir.join(self.spec.decoder_file_name());
        let feature_path = self.model_dir.join("nemo128.onnx");

        let encoder_session = Session::builder()
            .context("failed to create ONNX session builder for encoder")?
            .commit_from_file(&encoder_path)
            .with_context(|| {
                format!(
                    "failed to load encoder session from {}",
                    encoder_path.display()
                )
            })?;

        let decoder_session = Session::builder()
            .context("failed to create ONNX session builder for decoder")?
            .commit_from_file(&decoder_path)
            .with_context(|| {
                format!(
                    "failed to load decoder session from {}",
                    decoder_path.display()
                )
            })?;

        let feature_session = Session::builder()
            .context("failed to create ONNX session builder for nemo128")?
            .commit_from_file(&feature_path)
            .with_context(|| {
                format!(
                    "failed to load nemo128 session from {}",
                    feature_path.display()
                )
            })?;

        Ok(ParakeetSessions {
            _encoder_session: encoder_session,
            _decoder_session: decoder_session,
            _feature_session: feature_session,
        })
    }
}

fn summarize_outlets(outlets: &[ort::value::Outlet]) -> Vec<OutletSummary> {
    outlets
        .iter()
        .map(|outlet| OutletSummary {
            name: outlet.name().to_string(),
            dtype: format_value_type(outlet.dtype()),
        })
        .collect()
}

fn format_value_type(value_type: &ValueType) -> String {
    format!("{value_type:?}")
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
    use std::thread;

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
    fn chooses_expected_session_file_names() {
        let fp = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", None).unwrap();
        assert_eq!(fp.encoder_file_name(), "encoder-model.onnx");
        assert_eq!(fp.decoder_file_name(), "decoder_joint-model.onnx");

        let int8 = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", Some("int8")).unwrap();
        assert_eq!(int8.encoder_file_name(), "encoder-model.int8.onnx");
        assert_eq!(int8.decoder_file_name(), "decoder_joint-model.int8.onnx");
    }

    #[test]
    fn rejects_unknown_model_spec() {
        let error = ParakeetModelSpec::resolve("unknown", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported Parakeet model configuration"));
    }

    #[test]
    fn manager_reports_unprepared_model_dir() {
        let temp_dir = std::env::temp_dir().join("chirp-rust-parakeet-missing");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let spec = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", Some("int8")).unwrap();
        let error = ParakeetManager::new(temp_dir.clone(), spec, Some(Duration::from_secs(1)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("model is not prepared"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn manager_unloads_after_timeout() {
        let spec = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", Some("int8")).unwrap();
        let model_dir = PathBuf::from(
            r"E:\development\chirp\chirp-rust\assets\models\nemo-parakeet-tdt-0.6b-v3-int8",
        );

        if !spec.is_prepared(&model_dir) {
            return;
        }

        let mut manager =
            ParakeetManager::new(model_dir, spec, Some(Duration::from_millis(1))).unwrap();
        assert!(manager.is_loaded());
        thread::sleep(Duration::from_millis(5));
        manager.maybe_unload();
        assert!(!manager.is_loaded());
    }
}
