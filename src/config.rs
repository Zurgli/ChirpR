use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, Value, value};

pub const DEFAULT_PRIMARY_SHORTCUT: &str = "ctrl+shift+space";
pub const DEFAULT_RECORDING_MODE: &str = "toggle";
pub const DEFAULT_STT_BACKEND: &str = "parakeet";
pub const DEFAULT_PARAKEET_MODEL: &str = "nemo-parakeet-tdt-0.6b-v3";
pub const DEFAULT_ONNX_PROVIDERS: &str = "cpu";
pub const DEFAULT_PASTE_MODE: &str = "ctrl";
pub const DEFAULT_INJECTION_MODE: &str = "paste";
pub const DEFAULT_LANGUAGE: &str = "en";
pub const DEFAULT_CLIPBOARD_CLEAR_DELAY: f32 = 0.75;
pub const DEFAULT_MODEL_TIMEOUT: f32 = 10.0;
pub const DEFAULT_AUDIO_FEEDBACK_VOLUME: f32 = 0.25;
pub const DEFAULT_MAX_RECORDING_DURATION: f32 = 45.0;
pub const DEFAULT_OVERLAY_INDICATOR: &str = "sine_eye_double";
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
        let current_dir = std::env::current_dir().context("failed to resolve current directory")?;
        let current_exe =
            std::env::current_exe().context("failed to resolve current executable")?;
        Ok(Self::discover_from_paths(current_dir, current_exe))
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

    fn discover_from_paths(current_dir: PathBuf, current_exe: PathBuf) -> Self {
        for candidate in candidate_roots(&current_dir, &current_exe) {
            if looks_like_project_root(&candidate) {
                return Self::from_root(candidate);
            }
        }

        Self::from_root(current_dir)
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
    pub recording_mode: String,
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
    pub overlay_indicator: String,
    pub start_sound_path: Option<PathBuf>,
    pub stop_sound_path: Option<PathBuf>,
    pub error_sound_path: Option<PathBuf>,
    pub max_recording_duration: f32,
}

impl Default for ChirpConfig {
    fn default() -> Self {
        Self {
            primary_shortcut: DEFAULT_PRIMARY_SHORTCUT.to_string(),
            recording_mode: DEFAULT_RECORDING_MODE.to_string(),
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
            overlay_indicator: DEFAULT_OVERLAY_INDICATOR.to_string(),
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
        Self::from_toml_str(&raw)
            .with_context(|| format!("failed to parse TOML from {}", paths.config_path.display()))
    }

    pub fn from_toml_str(raw: &str) -> Result<Self> {
        let parsed: RawConfig = toml::from_str(raw)?;
        let config = Self::from_raw(parsed);
        config.validate()?;
        Ok(config)
    }

    pub fn validate_raw_toml(raw: &str) -> Result<Self> {
        Self::from_toml_str(raw)
    }

    pub fn to_canonical_toml(&self) -> Result<String> {
        self.validate()?;
        let mut toml = toml::to_string_pretty(&SerializableConfig::from(self))
            .context("failed to serialize config as TOML")?;
        if !toml.ends_with('\n') {
            toml.push('\n');
        }
        Ok(toml)
    }

    pub fn write_canonical(&self, path: &Path) -> Result<()> {
        let toml = self.to_canonical_toml()?;
        fs::write(path, toml)
            .with_context(|| format!("failed to write config file at {}", path.display()))
    }

    /// Writes this config by updating values in the existing file so inline and header comments
    /// stay attached to their keys. Falls back to [`Self::write_canonical`] when the file is
    /// missing, empty, or not valid TOML.
    pub fn write_merging_into_existing(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if !path.is_file() {
            return self.write_canonical(path);
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file at {}", path.display()))?;
        if raw.trim().is_empty() {
            return self.write_canonical(path);
        }
        let mut doc: DocumentMut = raw
            .parse()
            .with_context(|| format!("invalid TOML in {}", path.display()))?;
        merge_chirp_config_into_document(&mut doc, self)?;
        fs::write(path, doc.to_string()).with_context(|| {
            format!(
                "failed to write merged config file at {}",
                path.display()
            )
        })?;
        Ok(())
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
            recording_mode: raw
                .recording_mode
                .unwrap_or(defaults.recording_mode)
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
            overlay_indicator: raw
                .overlay_indicator
                .unwrap_or(defaults.overlay_indicator)
                .to_ascii_lowercase(),
            start_sound_path: normalize_optional_string(raw.start_sound_path).map(PathBuf::from),
            stop_sound_path: normalize_optional_string(raw.stop_sound_path).map(PathBuf::from),
            error_sound_path: normalize_optional_string(raw.error_sound_path).map(PathBuf::from),
            max_recording_duration: raw
                .max_recording_duration
                .unwrap_or(defaults.max_recording_duration),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.stt_backend != "parakeet" {
            bail!("stt_backend must be 'parakeet', got {:?}", self.stt_backend);
        }

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

        if self.recording_mode != "toggle" && self.recording_mode != "hold" {
            bail!(
                "recording_mode must be 'toggle' or 'hold', got {:?}",
                self.recording_mode
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

        if self.overlay_indicator != "dot"
            && self.overlay_indicator != "halo_soft"
            && self.overlay_indicator != "sine_eye_double"
        {
            bail!(
                "overlay_indicator must be 'dot', 'halo_soft', or 'sine_eye_double', got {:?}",
                self.overlay_indicator
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

        crate::stt::parakeet::ParakeetModelSpec::resolve(
            &self.parakeet_model,
            self.parakeet_quantization.as_deref(),
        )?;

        validate_optional_path("start_sound_path", self.start_sound_path.as_deref())?;
        validate_optional_path("stop_sound_path", self.stop_sound_path.as_deref())?;
        validate_optional_path("error_sound_path", self.error_sound_path.as_deref())?;

        Ok(())
    }
}

fn merge_chirp_config_into_document(doc: &mut DocumentMut, config: &ChirpConfig) -> Result<()> {
    set_root_value(doc, "primary_shortcut", config.primary_shortcut.as_str().into());
    set_root_value(doc, "recording_mode", config.recording_mode.as_str().into());
    set_root_value(doc, "stt_backend", config.stt_backend.as_str().into());
    set_root_value(doc, "parakeet_model", config.parakeet_model.as_str().into());
    set_root_value(
        doc,
        "parakeet_quantization",
        config
            .parakeet_quantization
            .as_deref()
            .unwrap_or_default()
            .into(),
    );
    set_root_value(doc, "onnx_providers", config.onnx_providers.as_str().into());
    match config.threads {
        Some(t) => set_root_value(doc, "threads", i64::from(t).into()),
        None => {
            doc.remove("threads");
        }
    }
    match config.language.as_deref() {
        Some(lang) => set_root_value(doc, "language", lang.into()),
        None => {
            doc.remove("language");
        }
    }
    set_root_value(doc, "post_processing", config.post_processing.as_str().into());
    set_root_value(doc, "injection_mode", config.injection_mode.as_str().into());
    set_root_value(doc, "paste_mode", config.paste_mode.as_str().into());
    set_root_value(doc, "clipboard_behavior", config.clipboard_behavior.into());
    set_root_value(
        doc,
        "clipboard_clear_delay",
        f64::from(config.clipboard_clear_delay).into(),
    );
    set_root_value(doc, "model_timeout", f64::from(config.model_timeout).into());
    set_root_value(doc, "audio_feedback", config.audio_feedback.into());
    set_root_value(
        doc,
        "audio_feedback_volume",
        f64::from(config.audio_feedback_volume).into(),
    );
    set_root_value(doc, "recording_overlay", config.recording_overlay.into());
    set_root_value(doc, "overlay_indicator", config.overlay_indicator.as_str().into());
    set_root_value(
        doc,
        "start_sound_path",
        opt_path_toml(config.start_sound_path.as_deref()).into(),
    );
    set_root_value(
        doc,
        "stop_sound_path",
        opt_path_toml(config.stop_sound_path.as_deref()).into(),
    );
    set_root_value(
        doc,
        "error_sound_path",
        opt_path_toml(config.error_sound_path.as_deref()).into(),
    );
    set_root_value(
        doc,
        "max_recording_duration",
        f64::from(config.max_recording_duration).into(),
    );

    merge_word_overrides_table(doc, &config.word_overrides)?;
    Ok(())
}

/// Updates a root key’s logical value while keeping the existing value’s decoration (including
/// end-of-line comments). Inserts the key if missing or if the existing entry is not a value.
fn set_root_value(doc: &mut DocumentMut, key: &str, new_val: Value) {
    if let Some(item) = doc.get_mut(key) {
        if let Some(old_v) = item.as_value_mut() {
            let decor = old_v.decor().clone();
            *old_v = new_val;
            *old_v.decor_mut() = decor;
            return;
        }
    }
    doc[key] = value(new_val);
}

/// Same as [`set_root_value`] for a normal TOML table (e.g. `[word_overrides]`).
fn set_table_value(table: &mut Table, key: &str, new_val: Value) {
    if let Some(item) = table.get_mut(key) {
        if let Some(old_v) = item.as_value_mut() {
            let decor = old_v.decor().clone();
            *old_v = new_val;
            *old_v.decor_mut() = decor;
            return;
        }
    }
    table.insert(key, value(new_val));
}

fn opt_path_toml(path: Option<&Path>) -> String {
    path.map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn merge_word_overrides_table(
    doc: &mut DocumentMut,
    map: &BTreeMap<String, String>,
) -> Result<()> {
    if !doc.contains_table("word_overrides") {
        doc["word_overrides"] = Item::Table(Table::new());
    }
    let table = doc["word_overrides"]
        .as_table_mut()
        .with_context(|| "word_overrides must be a TOML table")?;

    let stale_keys: Vec<String> = table
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| !map.contains_key(key))
        .collect();
    for key in stale_keys {
        table.remove(&key);
    }

    for (key, replacement) in map {
        set_table_value(table, key.as_str(), replacement.as_str().into());
    }

    Ok(())
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

fn candidate_roots(current_dir: &Path, current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_ancestors(current_exe.parent(), &mut candidates);
    push_ancestors(Some(current_dir), &mut candidates);
    candidates
}

fn push_ancestors(start: Option<&Path>, candidates: &mut Vec<PathBuf>) {
    if let Some(start) = start {
        for ancestor in start.ancestors() {
            let candidate = ancestor.to_path_buf();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
}

fn looks_like_project_root(path: &Path) -> bool {
    path.join("config.toml").is_file() && path.join("assets").is_dir()
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    primary_shortcut: Option<String>,
    recording_mode: Option<String>,
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
    overlay_indicator: Option<String>,
    start_sound_path: Option<String>,
    stop_sound_path: Option<String>,
    error_sound_path: Option<String>,
    max_recording_duration: Option<f32>,
}

#[derive(Debug, Serialize)]
struct SerializableConfig<'a> {
    primary_shortcut: &'a str,
    recording_mode: &'a str,
    stt_backend: &'a str,
    parakeet_model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parakeet_quantization: Option<&'a str>,
    onnx_providers: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    threads: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    word_overrides: &'a BTreeMap<String, String>,
    post_processing: &'a str,
    injection_mode: &'a str,
    paste_mode: &'a str,
    clipboard_behavior: bool,
    clipboard_clear_delay: f32,
    model_timeout: f32,
    audio_feedback: bool,
    audio_feedback_volume: f32,
    recording_overlay: bool,
    overlay_indicator: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_sound_path: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sound_path: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_sound_path: Option<&'a Path>,
    max_recording_duration: f32,
}

impl<'a> From<&'a ChirpConfig> for SerializableConfig<'a> {
    fn from(config: &'a ChirpConfig) -> Self {
        Self {
            primary_shortcut: &config.primary_shortcut,
            recording_mode: &config.recording_mode,
            stt_backend: &config.stt_backend,
            parakeet_model: &config.parakeet_model,
            parakeet_quantization: config.parakeet_quantization.as_deref(),
            onnx_providers: &config.onnx_providers,
            threads: config.threads,
            language: config.language.as_deref(),
            word_overrides: &config.word_overrides,
            post_processing: &config.post_processing,
            injection_mode: &config.injection_mode,
            paste_mode: &config.paste_mode,
            clipboard_behavior: config.clipboard_behavior,
            clipboard_clear_delay: config.clipboard_clear_delay,
            model_timeout: config.model_timeout,
            audio_feedback: config.audio_feedback,
            audio_feedback_volume: config.audio_feedback_volume,
            recording_overlay: config.recording_overlay,
            overlay_indicator: &config.overlay_indicator,
            start_sound_path: config.start_sound_path.as_deref(),
            stop_sound_path: config.stop_sound_path.as_deref(),
            error_sound_path: config.error_sound_path.as_deref(),
            max_recording_duration: config.max_recording_duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_paths() -> ProjectPaths {
        ProjectPaths::from_root(PathBuf::from(r"E:\development\chirp\chirp-rust"))
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid system time")
            .as_nanos();
        std::env::temp_dir().join(format!("chirpr-{name}-{nanos}"))
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
    fn invalid_recording_mode_fails() {
        let config = ChirpConfig {
            recording_mode: "pulse".into(),
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("recording_mode must be 'toggle' or 'hold'"));
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
    fn invalid_overlay_indicator_fails() {
        let config = ChirpConfig {
            overlay_indicator: "blob".into(),
            ..ChirpConfig::default()
        };
        let error = config.validate().unwrap_err().to_string();
        assert!(
            error.contains("overlay_indicator must be 'dot', 'halo_soft', or 'sine_eye_double'")
        );
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
            recording_mode: Some("Hold".into()),
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
            overlay_indicator: Some("Halo_Soft".into()),
            start_sound_path: None,
            stop_sound_path: None,
            error_sound_path: None,
            max_recording_duration: None,
        };

        let config = ChirpConfig::from_raw(raw);
        assert_eq!(config.primary_shortcut, "ctrl+shift+space");
        assert_eq!(config.recording_mode, "hold");
        assert_eq!(config.parakeet_quantization.as_deref(), Some("int8"));
        assert_eq!(config.onnx_providers, "cpu");
        assert_eq!(config.injection_mode, "paste");
        assert_eq!(config.paste_mode, "ctrl+shift");
        assert_eq!(config.overlay_indicator, "halo_soft");
        assert!(config.word_overrides.contains_key("parra keat"));
    }

    #[test]
    fn raw_config_preserves_negative_threads_for_validation() {
        let raw = RawConfig {
            primary_shortcut: None,
            recording_mode: None,
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
            overlay_indicator: None,
            start_sound_path: None,
            stop_sound_path: None,
            error_sound_path: None,
            max_recording_duration: None,
        };

        let config = ChirpConfig::from_raw(raw);
        assert_eq!(config.threads, Some(-5));
        assert!(config.validate().is_err());
    }

    #[test]
    fn canonical_toml_round_trips_common_settings() {
        let mut config = ChirpConfig {
            primary_shortcut: "rightctrl".into(),
            recording_mode: "hold".into(),
            audio_feedback: false,
            overlay_indicator: "dot".into(),
            max_recording_duration: 12.5,
            ..ChirpConfig::default()
        };
        config
            .word_overrides
            .insert("parra keat".into(), "parakeet".into());

        let raw = config.to_canonical_toml().unwrap();
        let reparsed = ChirpConfig::from_toml_str(&raw).unwrap();

        assert_eq!(reparsed, config);
        assert!(raw.contains("primary_shortcut = \"rightctrl\""));
        assert!(raw.contains("recording_mode = \"hold\""));
    }

    #[test]
    fn raw_toml_validation_reports_parse_errors() {
        let error = ChirpConfig::validate_raw_toml("primary_shortcut = [")
            .unwrap_err()
            .to_string();
        assert!(error.contains("TOML parse error"));
    }

    #[test]
    fn canonical_toml_skips_empty_optional_paths() {
        let config = ChirpConfig::default();
        let raw = config.to_canonical_toml().unwrap();
        assert!(!raw.contains("start_sound_path"));
        assert!(!raw.contains("stop_sound_path"));
        assert!(!raw.contains("error_sound_path"));
    }

    #[test]
    fn canonical_toml_serializes_sound_paths() {
        let root = unique_temp_dir("config-sound");
        fs::create_dir_all(&root).unwrap();
        let sound_path = root.join("ding.wav");
        fs::write(&sound_path, "tone").unwrap();

        let config = ChirpConfig {
            start_sound_path: Some(sound_path.clone()),
            ..ChirpConfig::default()
        };

        let raw = config.to_canonical_toml().unwrap();
        assert!(raw.contains("start_sound_path"));
        assert!(raw.contains(&sound_path.display().to_string()));
    }

    #[test]
    fn canonical_write_persists_word_overrides() {
        let root = unique_temp_dir("config-write");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.toml");

        let mut config = ChirpConfig::default();
        config
            .word_overrides
            .insert("parra keat".into(), "parakeet".into());
        config.write_canonical(&config_path).unwrap();

        let written = fs::read_to_string(&config_path).unwrap();
        assert!(written.contains("[word_overrides]"));
        assert!(written.contains("\"parra keat\" = \"parakeet\""));
    }

    #[test]
    fn discover_prefers_executable_ancestor_when_cwd_is_wrong() {
        let root = unique_temp_dir("discover-exe");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("config.toml"),
            "primary_shortcut = \"ctrl+shift+space\"\nrecording_mode = \"toggle\"\n",
        )
        .unwrap();

        let exe_path = root.join("target").join("debug").join("chirpr.exe");
        fs::create_dir_all(exe_path.parent().unwrap()).unwrap();
        fs::write(&exe_path, "").unwrap();

        let discovered = ProjectPaths::discover_from_paths(root.join("assets"), exe_path);

        assert_eq!(discovered.project_root, root);
    }

    #[test]
    fn discover_falls_back_to_current_dir_when_it_is_valid() {
        let root = unique_temp_dir("discover-cwd");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("config.toml"),
            "primary_shortcut = \"ctrl+shift+space\"\nrecording_mode = \"toggle\"\n",
        )
        .unwrap();

        let discovered = ProjectPaths::discover_from_paths(
            root.clone(),
            PathBuf::from(r"C:\Windows\System32\chirpr.exe"),
        );

        assert_eq!(discovered.project_root, root);
    }

    #[test]
    fn merge_write_preserves_inline_comments_in_repo_config() {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        if !src.is_file() {
            return;
        }
        let raw = fs::read_to_string(&src).unwrap();
        let hashes_before = raw.matches('#').count();
        let inline_marker = "Global recording shortcut";
        let block_marker = "Word overrides map spoken";
        assert!(
            raw.contains(inline_marker),
            "repo config.toml should contain `{inline_marker}` for this regression test"
        );
        assert!(
            raw.contains(block_marker),
            "repo config.toml should contain `{block_marker}` for this regression test"
        );

        let config = ChirpConfig::from_toml_str(&raw).unwrap();
        let dir = unique_temp_dir("merge-comments");
        fs::create_dir_all(&dir).unwrap();
        let dst = dir.join("config.toml");
        fs::write(&dst, &raw).unwrap();
        config.write_merging_into_existing(&dst).unwrap();
        let out = fs::read_to_string(&dst).unwrap();
        let hashes_after = out.matches('#').count();
        assert!(
            out.contains(inline_marker),
            "expected inline comment text preserved; output:\n{out}"
        );
        assert!(
            out.contains(block_marker),
            "expected full-line comment before [word_overrides] preserved; output:\n{out}"
        );
        assert_eq!(
            hashes_before, hashes_after,
            "hash/comment count changed: before {hashes_before} after {hashes_after}"
        );
    }
}
