use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ndarray::{Array1, Array2, Array3, ArrayD};
use ort::{
    session::Session,
    value::{TensorRef, ValueType},
};
use reqwest::blocking::Client;
use serde::Deserialize;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParakeetBundle {
    pub config: ParakeetConfig,
    pub vocabulary: ParakeetVocabulary,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ParakeetConfig {
    pub model_type: String,
    pub features_size: usize,
    pub subsampling_factor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParakeetVocabulary {
    pub tokens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecoderBootstrap {
    pub targets: Array2<i32>,
    pub target_length: Array1<i32>,
    pub input_states_1: Array3<f32>,
    pub input_states_2: Array3<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendPassSummary {
    pub waveform_shape: Vec<usize>,
    pub feature_shape: Vec<usize>,
    pub feature_lengths: Vec<i64>,
    pub encoder_shape: Vec<usize>,
    pub encoder_lengths: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderStepSummary {
    pub logits_shape: Vec<usize>,
    pub prednet_lengths: Vec<i32>,
    pub output_state_1_shape: Vec<usize>,
    pub output_state_2_shape: Vec<usize>,
}

#[derive(Debug)]
struct ParakeetSessions {
    _encoder_session: Session,
    _decoder_session: Session,
    _feature_session: Session,
}

struct FrontendOutputs {
    waveform: Array2<f32>,
    features: ArrayD<f32>,
    feature_lengths: Array1<i64>,
    encoder_outputs: ArrayD<f32>,
    encoder_lengths: Array1<i64>,
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

    pub fn load_bundle(&self) -> Result<ParakeetBundle> {
        ParakeetBundle::load(&self.model_dir)
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

    pub fn run_frontend_dummy(&mut self, sample_count: usize) -> Result<FrontendPassSummary> {
        let frontend = self.run_frontend_outputs(sample_count)?;
        Ok(FrontendPassSummary {
            waveform_shape: frontend.waveform.shape().to_vec(),
            feature_shape: frontend.features.shape().to_vec(),
            feature_lengths: frontend.feature_lengths.iter().copied().collect(),
            encoder_shape: frontend.encoder_outputs.shape().to_vec(),
            encoder_lengths: frontend.encoder_lengths.iter().copied().collect(),
        })
    }

    pub fn run_decoder_dummy_step(&mut self, sample_count: usize) -> Result<DecoderStepSummary> {
        let frontend = self.run_frontend_outputs(sample_count)?;
        let bundle = self.load_bundle()?;
        let bootstrap = bundle.vocabulary.build_decoder_bootstrap(1)?;
        let sessions = self
            .sessions
            .as_mut()
            .context("Parakeet sessions were not loaded")?;

        let decoder_outputs = sessions._decoder_session.run(ort::inputs![
            TensorRef::from_array_view(&frontend.encoder_outputs)?,
            TensorRef::from_array_view(&bootstrap.targets)?,
            TensorRef::from_array_view(&bootstrap.target_length)?,
            TensorRef::from_array_view(&bootstrap.input_states_1)?,
            TensorRef::from_array_view(&bootstrap.input_states_2)?,
        ])?;

        let logits = decoder_outputs["outputs"]
            .try_extract_array::<f32>()
            .context("failed to extract decoder logits tensor")?;
        let prednet_lengths = decoder_outputs["prednet_lengths"]
            .try_extract_array::<i32>()
            .context("failed to extract decoder prednet lengths tensor")?;
        let output_state_1 = decoder_outputs["output_states_1"]
            .try_extract_array::<f32>()
            .context("failed to extract decoder state 1 tensor")?;
        let output_state_2 = decoder_outputs["output_states_2"]
            .try_extract_array::<f32>()
            .context("failed to extract decoder state 2 tensor")?;

        Ok(DecoderStepSummary {
            logits_shape: logits.shape().to_vec(),
            prednet_lengths: prednet_lengths.iter().copied().collect(),
            output_state_1_shape: output_state_1.shape().to_vec(),
            output_state_2_shape: output_state_2.shape().to_vec(),
        })
    }

    fn run_frontend_outputs(&mut self, sample_count: usize) -> Result<FrontendOutputs> {
        self.ensure_loaded()?;
        let sessions = self
            .sessions
            .as_mut()
            .context("Parakeet sessions were not loaded")?;

        let waveform = Array2::<f32>::zeros((1, sample_count));
        let waveform_lens = Array1::<i64>::from_vec(vec![sample_count as i64]);

        let feature_outputs = sessions._feature_session.run(ort::inputs![
            TensorRef::from_array_view(&waveform)?,
            TensorRef::from_array_view(&waveform_lens)?,
        ])?;

        let features = feature_outputs["features"]
            .try_extract_array::<f32>()
            .context("failed to extract nemo128 features tensor")?;
        let feature_lengths = feature_outputs["features_lens"]
            .try_extract_array::<i64>()
            .context("failed to extract nemo128 feature length tensor")?;

        let feature_array = features.to_owned();
        let feature_lengths_vec = feature_lengths.iter().copied().collect::<Vec<_>>();
        drop(feature_outputs);

        let feature_length_input = Array1::<i64>::from_vec(feature_lengths_vec.clone());
        let encoder_outputs = sessions._encoder_session.run(ort::inputs![
            TensorRef::from_array_view(&feature_array)?,
            TensorRef::from_array_view(&feature_length_input)?,
        ])?;

        let encoded = encoder_outputs["outputs"]
            .try_extract_array::<f32>()
            .context("failed to extract encoder output tensor")?;
        let encoded_lengths = encoder_outputs["encoded_lengths"]
            .try_extract_array::<i64>()
            .context("failed to extract encoder lengths tensor")?;

        Ok(FrontendOutputs {
            waveform,
            features: feature_array,
            feature_lengths: feature_length_input,
            encoder_outputs: encoded.to_owned(),
            encoder_lengths: Array1::from_iter(encoded_lengths.iter().copied()),
        })
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

impl ParakeetBundle {
    pub fn load(model_dir: &Path) -> Result<Self> {
        let config_path = model_dir.join("config.json");
        let config_raw = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let config: ParakeetConfig = serde_json::from_str(&config_raw)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;

        let vocab_path = model_dir.join("vocab.txt");
        let vocab_raw = fs::read_to_string(&vocab_path)
            .with_context(|| format!("failed to read {}", vocab_path.display()))?;
        let tokens = vocab_raw
            .lines()
            .filter_map(|line| line.split_once(' ').map(|(token, _)| token.to_string()))
            .collect::<Vec<_>>();

        Ok(Self {
            config,
            vocabulary: ParakeetVocabulary { tokens },
        })
    }
}

impl ParakeetVocabulary {
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn token_id(&self, token: &str) -> Option<usize> {
        self.tokens.iter().position(|value| value == token)
    }

    pub fn start_of_transcript_id(&self) -> Option<usize> {
        self.token_id("<|startoftranscript|>")
    }

    pub fn blank_token_id(&self) -> usize {
        self.tokens.len()
    }

    pub fn build_decoder_bootstrap(&self, batch_size: usize) -> Result<DecoderBootstrap> {
        let start_token =
            self.start_of_transcript_id()
                .context("missing <|startoftranscript|> token in vocabulary")? as i32;

        Ok(DecoderBootstrap {
            targets: Array2::from_elem((batch_size, 1), start_token),
            target_length: Array1::from_elem(batch_size, 1_i32),
            input_states_1: Array3::zeros((2, batch_size, 640)),
            input_states_2: Array3::zeros((2, batch_size, 640)),
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

    #[test]
    fn vocabulary_builds_decoder_bootstrap() {
        let vocabulary = ParakeetVocabulary {
            tokens: vec![
                "<unk>".into(),
                "<|startoftranscript|>".into(),
                "hello".into(),
            ],
        };

        let bootstrap = vocabulary.build_decoder_bootstrap(2).unwrap();
        assert_eq!(bootstrap.targets.shape(), &[2, 1]);
        assert_eq!(bootstrap.target_length.to_vec(), vec![1, 1]);
        assert_eq!(bootstrap.targets[[0, 0]], 1);
        assert_eq!(bootstrap.input_states_1.shape(), &[2, 2, 640]);
        assert_eq!(bootstrap.input_states_2.shape(), &[2, 2, 640]);
        assert_eq!(vocabulary.blank_token_id(), 3);
    }

    #[test]
    fn manager_runs_dummy_frontend_pass() {
        let spec = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", Some("int8")).unwrap();
        let model_dir = PathBuf::from(
            r"E:\development\chirp\chirp-rust\assets\models\nemo-parakeet-tdt-0.6b-v3-int8",
        );

        if !spec.is_prepared(&model_dir) {
            return;
        }

        let mut manager =
            ParakeetManager::new(model_dir, spec, Some(Duration::from_secs(300))).unwrap();
        let summary = manager.run_frontend_dummy(1600).unwrap();
        assert_eq!(summary.waveform_shape, vec![1, 1600]);
        assert_eq!(summary.feature_shape[0], 1);
        assert_eq!(summary.feature_shape[1], 128);
        assert_eq!(summary.encoder_shape[0], 1);
        assert_eq!(summary.encoder_shape[1], 1024);
        assert_eq!(summary.feature_lengths.len(), 1);
        assert_eq!(summary.encoder_lengths.len(), 1);
    }

    #[test]
    fn manager_runs_dummy_decoder_step() {
        let spec = ParakeetModelSpec::resolve("nemo-parakeet-tdt-0.6b-v3", Some("int8")).unwrap();
        let model_dir = PathBuf::from(
            r"E:\development\chirp\chirp-rust\assets\models\nemo-parakeet-tdt-0.6b-v3-int8",
        );

        if !spec.is_prepared(&model_dir) {
            return;
        }

        let mut manager =
            ParakeetManager::new(model_dir, spec, Some(Duration::from_secs(300))).unwrap();
        let summary = manager.run_decoder_dummy_step(1600).unwrap();
        assert_eq!(summary.logits_shape[0], 1);
        assert_eq!(summary.output_state_1_shape, vec![2, 1, 640]);
        assert_eq!(summary.output_state_2_shape, vec![2, 1, 640]);
        assert_eq!(summary.prednet_lengths, vec![1]);
    }
}
