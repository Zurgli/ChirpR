use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Deserialize;

pub const DEFAULT_PRIMARY_SHORTCUT: &str = "ctrl+shift+space";
pub const DEFAULT_STT_BACKEND: &str = "parakeet";
pub const DEFAULT_PARAKEET_MODEL: &str = "nemo-parakeet-tdt-0.6b-v3";
pub const DEFAULT_ONNX_PROVIDERS: &str = "cpu";
pub const DEFAULT_PASTE_MODE: &str = "ctrl";
pub const DEFAULT_INJECTION_MODE: &str = "paste";
pub const DEFAULT_LANGUAGE: &str = "en";
pub const DEFAULT_CLIPBOARD_CLEAR_DELAY: f32 = 0.75;
pub const DEFAULT_MODEL_TIMEOUT: f32 = 300.0;
pub const DEFAULT_AUDIO_FEEDBACK_VOLUME: f32 = 0.25;
pub const DEFAULT_MAX_RECORDING_DURATION: f32 = 45.0;
pub const MAX_ALLOWED_DURATION: f32 = 7200.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    pub project_root: PathBuf,
    pub config_path: PathBuf,
    pub assets_root: PathBuf,
    pub models_root: PathBuf,
}

impl ProjectPaths {
    pub fn discover() -> Result<Self> {
        let project_root =
            std::env::current_dir().context("failed to resolve current directory")?;
        Ok(Self::from_root(project_root))
    }

    pub fn from_root(project_root: PathBuf) -> Self {
        let assets_root = project_root.join("assets");
        let models_root = assets_root.join("models");
        let config_path = project_root.join("config.toml");
        Self {
            project_root,
            config_path,
            assets_root,
            models_root,
        }
    }

    pub fn with_config_path(mut self, config_path: PathBuf) -> Self {
        self.config_path = config_path;
        self
    }

    pub fn ensure_models_root(&self) -> Result<()> {
        fs::create_dir_all(&self.models_root).with_context(|| {
            format!(
                "failed to create models directory at {}",
                self.models_root.display()
            )
        })
    }

    pub fn model_dir(&self, model_name: &str, quantization: Option<&str>) -> Result<PathBuf> {
        let suffix = match quantization
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "int8" => "-int8",
            _ => "",
        };

        let invalid_chars = Regex::new(r"[^A-Za-z0-9._-]+").expect("valid regex");
        let repeated_dots = Regex::new(r"\.+").expect("valid regex");

        let lowercase_name = model_name.to_ascii_lowercase();
        let sanitized = invalid_chars.replace_all(&lowercase_name, "-");
        let sanitized = repeated_dots.replace_all(&sanitized, ".");
        let sanitized = sanitized.trim_matches(&['-', '.'][..]);
        let safe_name = if sanitized.is_empty() {
            "model"
        } else {
            sanitized
        };

        let model_dir = self.models_root.join(format!("{safe_name}{suffix}"));
        let resolved = normalize_path(&model_dir);
        let models_root = normalize_path(&self.models_root);

        if !resolved.starts_with(&models_root) {
            bail!("invalid model name {model_name:?}: resolved path escapes models directory");
        }

        Ok(resolved)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChirpConfig {
    pub primary_shortcut: String,
    pub stt_backend: String,
    pub parakeet_model: String,
    pub parakeet_quantization: Option<String>,
    pub onnx_providers: String,
    pub threads: Option<i32>,
    pub language: Option<String>,
    pub word_overrides: BTreeMap<String, String>,
    pub post_processing: String,
    pub injection_mode: String,
    pub paste_mode: String,
    pub clipboard_behavior: bool,
    pub clipboard_clear_delay: f32,
    pub model_timeout: f32,
    pub audio_feedback: bool,
    pub audio_feedback_volume: f32,
    pub recording_overlay: bool,
    pub start_sound_path: Option<PathBuf>,
    pub stop_sound_path: Option<PathBuf>,
    pub error_sound_path: Option<PathBuf>,
    pub max_recording_duration: f32,
}

impl Default for ChirpConfig {
    fn default() -> Self {
        Self {
            primary_shortcut: DEFAULT_PRIMARY_SHORTCUT.to_string(),
            stt_backend: DEFAULT_STT_BACKEND.to_string(),
            parakeet_model: DEFAULT_PARAKEET_MODEL.to_string(),
            parakeet_quantization: None,
            onnx_providers: DEFAULT_ONNX_PROVIDERS.to_string(),
            threads: None,
            language: Some(DEFAULT_LANGUAGE.to_string()),
            word_overrides: BTreeMap::new(),
            post_processing: String::new(),
            injection_mode: DEFAULT_INJECTION_MODE.to_string(),
            paste_mode: DEFAULT_PASTE_MODE.to_string(),
            clipboard_behavior: true,
            clipboard_clear_delay: DEFAULT_CLIPBOARD_CLEAR_DELAY,
            model_timeout: DEFAULT_MODEL_TIMEOUT,
            audio_feedback: true,
            audio_feedback_volume: DEFAULT_AUDIO_FEEDBACK_VOLUME,
            recording_overlay: true,
            start_sound_path: None,
            stop_sound_path: None,
            error_sound_path: None,
            max_recording_duration: DEFAULT_MAX_RECORDING_DURATION,
        }
    }
}

impl ChirpConfig {
    pub fn load(paths: &ProjectPaths) -> Result<Self> {
        let raw = fs::read_to_string(&paths.config_path).with_context(|| {
            format!(
                "failed to read config file at {}",
                paths.config_path.display()
            )
        })?;
        let parsed: RawConfig = toml::from_str(&raw).with_context(|| {
            format!("failed to parse TOML from {}", paths.config_path.display())
        })?;
        let config = Self::from_raw(parsed);
        config.validate()?;
        Ok(config)
    }

    fn from_raw(raw: RawConfig) -> Self {
        let defaults = Self::default();
        let word_overrides = raw
            .word_overrides
            .unwrap_or_default()
            .into_iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect();

        Self {
            primary_shortcut: raw
                .primary_shortcut
                .unwrap_or(defaults.primary_shortcut)
                .to_ascii_lowercase(),
            stt_backend: raw.stt_backend.unwrap_or(defaults.stt_backend),
            parakeet_model: raw.parakeet_model.unwrap_or(defaults.parakeet_model),
            parakeet_quantization: normalize_optional_string(raw.parakeet_quantization)
                .map(|value| value.to_ascii_lowercase()),
            onnx_providers: raw
                .onnx_providers
                .unwrap_or(defaults.onnx_providers)
                .to_ascii_lowercase(),
            threads: raw.threads.map(|value| value as i32),
            language: normalize_optional_string(raw.language).or(defaults.language),
            word_overrides,
            post_processing: raw.post_processing.unwrap_or(defaults.post_processing),
            injection_mode: raw
                .injection_mode
                .unwrap_or(defaults.injection_mode)
                .to_ascii_lowercase(),
            paste_mode: raw
                .paste_mode
                .unwrap_or(defaults.paste_mode)
                .to_ascii_lowercase(),
            clipboard_behavior: raw
                .clipboard_behavior
                .unwrap_or(defaults.clipboard_behavior),
            clipboard_clear_delay: raw
                .clipboard_clear_delay
                .unwrap_or(defaults.clipboard_clear_delay),
            model_timeout: raw.model_timeout.unwrap_or(defaults.model_timeout),
            audio_feedback: raw.audio_feedback.unwrap_or(defaults.audio_feedback),
            audio_feedback_volume: raw
                .audio_feedback_volume
                .unwrap_or(defaults.audio_feedback_volume),
            recording_overlay: raw.recording_overlay.unwrap_or(defaults.recording_overlay),
            start_sound_path: normalize_optional_string(raw.start_sound_path).map(PathBuf::from),
            stop_sound_path: normalize_optional_string(raw.stop_sound_path).map(PathBuf::from),
            error_sound_path: normalize_optional_string(raw.error_sound_path).map(PathBuf::from),
            max_recording_duration: raw
                .max_recording_duration
                .unwrap_or(defaults.max_recording_duration),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if matches!(self.threads, Some(value) if value < 0) {
            bail!(
                "threads must be non-negative, got {}",
                self.threads.unwrap()
            );
        }

        if matches!(self.parakeet_quantization.as_deref(), Some(value) if value != "int8" && !value.is_empty())
        {
            bail!(
                "parakeet_quantization must be empty or 'int8', got {:?}",
                self.parakeet_quantization
            );
        }

        if self.clipboard_clear_delay <= 0.0 {
            bail!(
                "clipboard_clear_delay must be positive, got {}",
                self.clipboard_clear_delay
            );
        }

        if self.injection_mode != "type" && self.injection_mode != "paste" {
            bail!(
                "injection_mode must be 'type' or 'paste', got {:?}",
                self.injection_mode
            );
        }

        if self.paste_mode != "ctrl" && self.paste_mode != "ctrl+shift" {
            bail!(
                "paste_mode must be 'ctrl' or 'ctrl+shift', got {:?}",
                self.paste_mode
            );
        }

        if self.model_timeout < 0.0 {
            bail!(
                "model_timeout must be non-negative, got {}",
                self.model_timeout
            );
        }

        if self.max_recording_duration < 0.0 {
            bail!(
                "max_recording_duration must be non-negative, got {}",
                self.max_recording_duration
            );
        }

        if self.max_recording_duration > MAX_ALLOWED_DURATION {
            bail!(
                "max_recording_duration must be <= {}, got {}",
                MAX_ALLOWED_DURATION,
                self.max_recording_duration
            );
        }

        if !(0.0..=1.0).contains(&self.audio_feedback_volume) {
            bail!(
                "audio_feedback_volume must be between 0.0 and 1.0, got {}",
                self.audio_feedback_volume
            );
        }

        validate_optional_path("start_sound_path", self.start_sound_path.as_deref())?;
        validate_optional_path("stop_sound_path", self.stop_sound_path.as_deref())?;
        validate_optional_path("error_sound_path", self.error_sound_path.as_deref())?;

        Ok(())
    }
}

fn validate_optional_path(name: &str, path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if !path.is_file() {
            bail!("{name} does not exist: {}", path.display());
        }
    }
    Ok(())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    primary_shortcut: Option<String>,
    stt_backend: Option<String>,
    parakeet_model: Option<String>,
    parakeet_quantization: Option<String>,
    onnx_providers: Option<String>,
    threads: Option<i64>,
    language: Option<String>,
    word_overrides: Option<BTreeMap<String, String>>,
    post_processing: Option<String>,
    injection_mode: Option<String>,
    paste_mode: Option<String>,
    clipboard_behavior: Option<bool>,
    clipboard_clear_delay: Option<f32>,
    model_timeout: Option<f32>,
    audio_feedback: Option<bool>,
    audio_feedback_volume: Option<f32>,
    recording_overlay: Option<bool>,
    start_sound_path: Option<String>,
    stop_sound_path: Option<String>,
    error_sound_path: Option<String>,
    max_recording_duration: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_paths() -> ProjectPaths {
        ProjectPaths::from_root(PathBuf::from(r"E:\development\chirp\chirp-rust"))
    }

    #[test]
    fn default_config_is_valid() {
        let config = ChirpConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn invalid_negative_clipboard_delay_fails() {
        let config = ChirpConfig {
            clipboard_clear_delay: -1.0,
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("clipboard_clear_delay must be positive"));
    }

    #[test]
    fn invalid_negative_threads_fail() {
        let config = ChirpConfig {
            threads: Some(-1),
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("threads must be non-negative"));
    }

    #[test]
    fn invalid_injection_mode_fails() {
        let config = ChirpConfig {
            injection_mode: "magic".into(),
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("injection_mode must be 'type' or 'paste'"));
    }

    #[test]
    fn invalid_paste_mode_fails() {
        let config = ChirpConfig {
            paste_mode: "hack".into(),
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("paste_mode must be 'ctrl' or 'ctrl+shift'"));
    }

    #[test]
    fn invalid_max_recording_duration_fails() {
        let config = ChirpConfig {
            max_recording_duration: MAX_ALLOWED_DURATION + 1.0,
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("max_recording_duration must be <="));
    }

    #[test]
    fn invalid_audio_feedback_volume_fails() {
        let config = ChirpConfig {
            audio_feedback_volume: 1.5,
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("audio_feedback_volume must be between 0.0 and 1.0"));
    }

    #[test]
    fn model_dir_adds_int8_suffix() {
        let paths = sample_paths();
        let model_dir = paths
            .model_dir("nemo-parakeet-tdt-0.6b-v3", Some("int8"))
            .unwrap();
        assert!(
            model_dir.ends_with(
                Path::new("assets")
                    .join("models")
                    .join("nemo-parakeet-tdt-0.6b-v3-int8")
            )
        );
    }

    #[test]
    fn model_dir_blocks_path_traversal() {
        let paths = sample_paths();
        let model_dir = paths.model_dir("../../escape", None).unwrap();
        assert!(model_dir.starts_with(paths.models_root));
    }

    #[test]
    fn raw_config_normalizes_keys() {
        let raw = RawConfig {
            primary_shortcut: Some("CTRL+SHIFT+SPACE".into()),
            stt_backend: None,
            parakeet_model: None,
            parakeet_quantization: Some("INT8".into()),
            onnx_providers: Some("CPU".into()),
            threads: Some(3),
            language: Some("en".into()),
            word_overrides: Some(BTreeMap::from([("Parra Keat".into(), "parakeet".into())])),
            post_processing: None,
            injection_mode: Some("Paste".into()),
            paste_mode: Some("CTRL+SHIFT".into()),
            clipboard_behavior: None,
            clipboard_clear_delay: None,
            model_timeout: None,
            audio_feedback: None,
            audio_feedback_volume: None,
            recording_overlay: None,
            start_sound_path: None,
            stop_sound_path: None,
            error_sound_path: None,
            max_recording_duration: None,
        };

        let config = ChirpConfig::from_raw(raw);
        assert_eq!(config.primary_shortcut, "ctrl+shift+space");
        assert_eq!(config.parakeet_quantization.as_deref(), Some("int8"));
        assert_eq!(config.onnx_providers, "cpu");
        assert_eq!(config.injection_mode, "paste");
        assert_eq!(config.paste_mode, "ctrl+shift");
        assert!(config.word_overrides.contains_key("parra keat"));
    }

    #[test]
    fn raw_config_preserves_negative_threads_for_validation() {
        let raw = RawConfig {
            primary_shortcut: None,
            stt_backend: None,
            parakeet_model: None,
            parakeet_quantization: None,
            onnx_providers: None,
            threads: Some(-5),
            language: None,
            word_overrides: None,
            post_processing: None,
            injection_mode: None,
            paste_mode: None,
            clipboard_behavior: None,
            clipboard_clear_delay: None,
            model_timeout: None,
            audio_feedback: None,
            audio_feedback_volume: None,
            recording_overlay: None,
            start_sound_path: None,
            stop_sound_path: None,
            error_sound_path: None,
            max_recording_duration: None,
        };

        let config = ChirpConfig::from_raw(raw);
        assert_eq!(config.threads, Some(-5));
        assert!(config.validate().is_err());
    }
}
