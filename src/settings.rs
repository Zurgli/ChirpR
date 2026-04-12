use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use std::time::UNIX_EPOCH;

use crate::config::{ChirpConfig, ProjectPaths};
use crate::keyboard::canonicalize_shortcut;
use crate::singleton::terminate_other_app_instances;
use crate::stt::parakeet::ParakeetModelSpec;

const SETTINGS_MUTEX_NAME: &str = "Local\\ChirpRSettingsSingleton";
const SETTINGS_WINDOW_TITLE: &str = "ChirpR Settings";

pub fn run(paths: ProjectPaths) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        return windows_impl::run(paths);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = paths;
        return Err(anyhow::anyhow!("settings window is only available on Windows"));
    }
}

fn build_background_launch_args(paths: &ProjectPaths) -> Result<Vec<OsString>> {
    let config_arg = fs::canonicalize(&paths.config_path)
        .unwrap_or_else(|_| paths.config_path.clone());
    Ok(vec![
        OsString::from("--config"),
        config_arg.into_os_string(),
    ])
}

fn relaunch_background_process(paths: &ProjectPaths) -> Result<()> {
    terminate_other_app_instances()?;
    let current_exe =
        std::env::current_exe().context("failed to resolve current executable for restart")?;
    let args = build_background_launch_args(paths)?;
    let mut command = Command::new(&current_exe);
    command.args(args);
    if paths.project_root.is_dir() {
        command.current_dir(&paths.project_root);
    }
    command
        .spawn()
        .with_context(|| format!("failed to relaunch ChirpR from {}", current_exe.display()))?;
    Ok(())
}

fn persist_structured_config(paths: &ProjectPaths, config: &ChirpConfig) -> Result<ChirpConfig> {
    config.write_merging_into_existing(&paths.config_path)?;
    Ok(config.clone())
}

fn persist_raw_config(paths: &ProjectPaths, raw: &str) -> Result<ChirpConfig> {
    let config = ChirpConfig::validate_raw_toml(raw)?;
    fs::write(&paths.config_path, raw).with_context(|| {
        format!(
            "failed to write config file at {}",
            paths.config_path.display()
        )
    })?;
    Ok(config)
}

fn model_status_message(paths: &ProjectPaths, config: &ChirpConfig) -> Result<String> {
    let model_dir = paths.model_dir(
        &config.parakeet_model,
        config.parakeet_quantization.as_deref(),
    )?;
    let spec = ParakeetModelSpec::resolve(
        &config.parakeet_model,
        config.parakeet_quantization.as_deref(),
    )?;
    let missing_files = spec.missing_files(&model_dir);

    if missing_files.is_empty() {
        Ok(format!("Model ready at {}", model_dir.display()))
    } else {
        Ok(format!(
            "Model files missing at {}: {}. Run chirpr-cli setup to prepare this model.",
            model_dir.display(),
            missing_files.join(", ")
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WordOverrideEntry {
    spoken: String,
    replacement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct StructuredSettings {
    primary_shortcut: String,
    recording_mode: String,
    injection_mode: String,
    paste_mode: String,
    clipboard_behavior: bool,
    clipboard_clear_delay: String,
    audio_feedback: bool,
    audio_feedback_volume: String,
    recording_overlay: bool,
    overlay_indicator: String,
    max_recording_duration: String,
    language: String,
    post_processing: String,
    start_sound_path: String,
    stop_sound_path: String,
    error_sound_path: String,
    stt_backend: String,
    parakeet_model: String,
    parakeet_quantization: String,
    onnx_providers: String,
    threads: String,
    model_timeout: String,
    word_overrides: Vec<WordOverrideEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitialState {
    form: StructuredSettings,
    raw_toml: String,
    model_status: String,
    load_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsUiAssets {
    html: String,
    css: String,
    js: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingsUiSnapshot {
    assets: SettingsUiAssets,
    newest_write: SystemTime,
}

impl StructuredSettings {
    fn from_config(config: &ChirpConfig) -> Self {
        Self {
            primary_shortcut: config.primary_shortcut.clone(),
            recording_mode: config.recording_mode.clone(),
            injection_mode: config.injection_mode.clone(),
            paste_mode: config.paste_mode.clone(),
            clipboard_behavior: config.clipboard_behavior,
            clipboard_clear_delay: config.clipboard_clear_delay.to_string(),
            audio_feedback: config.audio_feedback,
            audio_feedback_volume: config.audio_feedback_volume.to_string(),
            recording_overlay: config.recording_overlay,
            overlay_indicator: config.overlay_indicator.clone(),
            max_recording_duration: config.max_recording_duration.to_string(),
            language: config.language.clone().unwrap_or_default(),
            post_processing: config.post_processing.clone(),
            start_sound_path: path_to_string(config.start_sound_path.as_deref()),
            stop_sound_path: path_to_string(config.stop_sound_path.as_deref()),
            error_sound_path: path_to_string(config.error_sound_path.as_deref()),
            stt_backend: config.stt_backend.clone(),
            parakeet_model: config.parakeet_model.clone(),
            parakeet_quantization: config.parakeet_quantization.clone().unwrap_or_default(),
            onnx_providers: config.onnx_providers.clone(),
            threads: config
                .threads
                .map(|value| value.to_string())
                .unwrap_or_default(),
            model_timeout: config.model_timeout.to_string(),
            word_overrides: config
                .word_overrides
                .iter()
                .map(|(spoken, replacement)| WordOverrideEntry {
                    spoken: spoken.clone(),
                    replacement: replacement.clone(),
                })
                .collect(),
        }
    }

    fn into_config(self) -> Result<ChirpConfig> {
        let mut word_overrides = BTreeMap::new();
        for entry in self.word_overrides.into_iter() {
            let spoken = entry.spoken.trim();
            if spoken.is_empty() {
                continue;
            }
            let replacement = entry.replacement.trim();
            word_overrides.insert(spoken.to_ascii_lowercase(), replacement.to_string());
        }

        let config = ChirpConfig {
            primary_shortcut: canonicalize_shortcut(&self.primary_shortcut)?,
            recording_mode: self.recording_mode.trim().to_ascii_lowercase(),
            stt_backend: self.stt_backend.trim().to_ascii_lowercase(),
            parakeet_model: self.parakeet_model.trim().to_string(),
            parakeet_quantization: normalize_string(self.parakeet_quantization),
            onnx_providers: self.onnx_providers.trim().to_ascii_lowercase(),
            threads: parse_optional_i32(&self.threads, "threads")?,
            language: normalize_string(self.language),
            word_overrides,
            post_processing: self.post_processing.trim().to_string(),
            injection_mode: self.injection_mode.trim().to_ascii_lowercase(),
            paste_mode: self.paste_mode.trim().to_ascii_lowercase(),
            clipboard_behavior: self.clipboard_behavior,
            clipboard_clear_delay: parse_required_f32(
                &self.clipboard_clear_delay,
                "clipboard_clear_delay",
            )?,
            model_timeout: parse_required_f32(&self.model_timeout, "model_timeout")?,
            audio_feedback: self.audio_feedback,
            audio_feedback_volume: parse_required_f32(
                &self.audio_feedback_volume,
                "audio_feedback_volume",
            )?,
            recording_overlay: self.recording_overlay,
            overlay_indicator: self.overlay_indicator.trim().to_ascii_lowercase(),
            start_sound_path: normalize_path_string(&self.start_sound_path),
            stop_sound_path: normalize_path_string(&self.stop_sound_path),
            error_sound_path: normalize_path_string(&self.error_sound_path),
            max_recording_duration: parse_required_f32(
                &self.max_recording_duration,
                "max_recording_duration",
            )?,
        };
        config.validate()?;
        Ok(config)
    }
}

fn normalize_string(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn normalize_path_string(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn path_to_string(path: Option<&Path>) -> String {
    path.map(|value| value.display().to_string())
        .unwrap_or_default()
}

fn parse_required_f32(raw: &str, field_name: &str) -> Result<f32> {
    raw.trim()
        .parse::<f32>()
        .with_context(|| format!("{field_name} must be a number"))
}

fn parse_optional_i32(raw: &str, field_name: &str) -> Result<Option<i32>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        trimmed
            .parse::<i32>()
            .with_context(|| format!("{field_name} must be an integer"))
            .map(Some)
    }
}

fn load_initial_state(paths: &ProjectPaths) -> Result<InitialState> {
    let raw_toml = match fs::read_to_string(&paths.config_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ChirpConfig::default().to_canonical_toml()?
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to read config file at {}",
                    paths.config_path.display()
                )
            });
        }
    };

    match ChirpConfig::from_toml_str(&raw_toml) {
        Ok(config) => Ok(InitialState {
            form: StructuredSettings::from_config(&config),
            model_status: model_status_message(paths, &config)?,
            raw_toml,
            load_warning: None,
        }),
        Err(error) => {
            let default_config = ChirpConfig::default();
            Ok(InitialState {
                form: StructuredSettings::from_config(&default_config),
                model_status: model_status_message(paths, &default_config)?,
                raw_toml,
                load_warning: Some(format!(
                    "The current config could not be loaded into the structured editor. The Raw TOML tab still shows the live file.\n\n{error:#}"
                )),
            })
        }
    }
}

fn embedded_ui_assets() -> SettingsUiAssets {
    SettingsUiAssets {
        html: include_str!("settings_ui/index.html").to_string(),
        css: include_str!("settings_ui/settings.css").to_string(),
        js: include_str!("settings_ui/settings.js").to_string(),
    }
}

fn dev_settings_ui_dir(paths: &ProjectPaths) -> Option<PathBuf> {
    let candidate = paths.project_root.join("src").join("settings_ui");
    if candidate.join("index.html").is_file()
        && candidate.join("settings.css").is_file()
        && candidate.join("settings.js").is_file()
    {
        Some(candidate)
    } else {
        None
    }
}

fn load_settings_ui_snapshot(paths: &ProjectPaths) -> Result<SettingsUiSnapshot> {
    let Some(ui_dir) = dev_settings_ui_dir(paths) else {
        return Ok(SettingsUiSnapshot {
            assets: embedded_ui_assets(),
            newest_write: SystemTime::UNIX_EPOCH,
        });
    };

    let html_path = ui_dir.join("index.html");
    let css_path = ui_dir.join("settings.css");
    let js_path = ui_dir.join("settings.js");

    let html = fs::read_to_string(&html_path)
        .with_context(|| format!("failed to read {}", html_path.display()))?;
    let css = fs::read_to_string(&css_path)
        .with_context(|| format!("failed to read {}", css_path.display()))?;
    let js = fs::read_to_string(&js_path)
        .with_context(|| format!("failed to read {}", js_path.display()))?;

    let newest_write = [html_path, css_path, js_path]
        .into_iter()
        .map(|path| {
            fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .with_context(|| format!("failed to read modified time for {}", path.display()))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH);

    Ok(SettingsUiSnapshot {
        assets: SettingsUiAssets { html, css, js },
        newest_write,
    })
}

#[cfg(target_os = "windows")]
mod windows_impl {
    #![allow(unsafe_op_in_unsafe_fn)]

    use std::mem::size_of;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime};

    use anyhow::{Context, Result};
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::{Window, WindowBuilder};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONASTERISK, MB_ICONERROR, MB_OK, MessageBoxW,
    };
    use wry::http::Request;
    use wry::{WebView, WebViewBuilder};

    use crate::audio_feedback::AudioFeedback;
    use crate::config::{ChirpConfig, ProjectPaths};
    use crate::recording_overlay::enable_dpi_awareness;
    use crate::singleton::{WindowsMutexGuard, focus_window_by_title, try_acquire_named_mutex};

    use super::{
        InitialState, SETTINGS_MUTEX_NAME, SETTINGS_WINDOW_TITLE, SettingsUiSnapshot,
        StructuredSettings, dev_settings_ui_dir, load_initial_state, load_settings_ui_snapshot,
        model_status_message, persist_raw_config, persist_structured_config,
        relaunch_background_process,
    };

    #[derive(Debug, serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum IpcRequestPayload {
        SaveStructured { form: StructuredSettings },
        SaveRaw { raw: String },
        BrowseSound { field: String },
        PreviewSound { role: String, path: String },
        RefreshModelStatus { form: StructuredSettings },
        CloseWindow,
    }

    enum SettingsEvent {
        Ipc(IpcRequestPayload),
        ReloadAssets,
    }

    #[derive(serde::Serialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum UiResponse {
        ReplaceSoundPath { field: String, value: String },
        UpdateModelStatus { message: String },
        SetStructuredError { message: String },
        SetRawError { message: String },
        ReloadUi { state: super::InitialState },
        SaveSucceeded,
    }

    struct SettingsRuntime {
        paths: ProjectPaths,
        window: Window,
        webview: Option<WebView>,
        ui_snapshot: SettingsUiSnapshot,
        proxy: tao::event_loop::EventLoopProxy<SettingsEvent>,
        _mutex: WindowsMutexGuard,
    }

    pub fn run(paths: ProjectPaths) -> Result<()> {
        let Some(settings_mutex) = try_acquire_named_mutex(SETTINGS_MUTEX_NAME)? else {
            let _ = focus_window_by_title(SETTINGS_WINDOW_TITLE)?;
            return Ok(());
        };

        enable_dpi_awareness();

        let ui_snapshot = load_settings_ui_snapshot(&paths)?;

        let event_loop = EventLoopBuilder::<SettingsEvent>::with_user_event().build();
        let window = WindowBuilder::new()
            .with_title(SETTINGS_WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(1000.0, 700.0))
            .with_min_inner_size(LogicalSize::new(900.0, 700.0))
            .build(&event_loop)
            .context("failed to create settings window")?;

        let proxy = event_loop.create_proxy();
        let webview = build_webview(&window, &paths, &ui_snapshot, proxy.clone())?;

        let mut runtime = SettingsRuntime {
            paths,
            window,
            webview: Some(webview),
            ui_snapshot,
            proxy: proxy.clone(),
            _mutex: settings_mutex,
        };

        if dev_settings_ui_dir(&runtime.paths).is_some() {
            spawn_hot_reload_watcher(runtime.paths.clone(), proxy);
        }

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    runtime.webview.take();
                    *control_flow = ControlFlow::Exit;
                }
                Event::UserEvent(SettingsEvent::Ipc(payload)) => {
                    if let Err(error) = handle_ipc_event(&mut runtime, payload, control_flow) {
                        runtime.show_error(&format!("{error:#}"));
                    }
                }
                Event::UserEvent(SettingsEvent::ReloadAssets) => {
                    if let Err(error) = runtime.reload_webview_if_needed() {
                        runtime.show_error(&format!("{error:#}"));
                    }
                }
                _ => {}
            }
        })
    }

    fn build_ipc_handler(
        proxy: tao::event_loop::EventLoopProxy<SettingsEvent>,
    ) -> impl Fn(Request<String>) {
        move |request: Request<String>| {
            let body = request.into_body();
            match serde_json::from_str::<IpcRequestPayload>(&body) {
                Ok(payload) => {
                    let _ = proxy.send_event(SettingsEvent::Ipc(payload));
                }
                Err(_) => {
                    #[cfg(debug_assertions)]
                    eprintln!("ChirpR settings: IPC message ignored (JSON): {body}");
                }
            }
        }
    }

    fn build_webview(
        window: &Window,
        paths: &ProjectPaths,
        ui_snapshot: &SettingsUiSnapshot,
        proxy: tao::event_loop::EventLoopProxy<SettingsEvent>,
    ) -> Result<WebView> {
        let initial_state = load_initial_state(paths)?;
        let initial_script = build_initialization_script(&initial_state)?;
        let html = build_html(&ui_snapshot.assets);

        let builder = WebViewBuilder::new()
            .with_html(html)
            .with_initialization_script(initial_script)
            .with_ipc_handler(build_ipc_handler(proxy))
            .with_accept_first_mouse(true);

        builder
            .build(window)
            .context("failed to create settings webview")
    }

    fn spawn_hot_reload_watcher(
        paths: ProjectPaths,
        proxy: tao::event_loop::EventLoopProxy<SettingsEvent>,
    ) {
        thread::spawn(move || {
            let mut last_seen = load_settings_ui_snapshot(&paths)
                .map(|snapshot| snapshot.newest_write)
                .unwrap_or(SystemTime::UNIX_EPOCH);

            loop {
                thread::sleep(Duration::from_millis(350));
                let snapshot = match load_settings_ui_snapshot(&paths) {
                    Ok(snapshot) => snapshot,
                    Err(_) => continue,
                };

                if snapshot.newest_write > last_seen {
                    last_seen = snapshot.newest_write;
                    if proxy.send_event(SettingsEvent::ReloadAssets).is_err() {
                        break;
                    }
                }
            }
        });
    }

    fn handle_ipc_event(
        runtime: &mut SettingsRuntime,
        payload: IpcRequestPayload,
        control_flow: &mut ControlFlow,
    ) -> Result<()> {
        match payload {
            IpcRequestPayload::SaveStructured { form } => match form.into_config() {
                Ok(config) => {
                    persist_structured_config(&runtime.paths, &config)?;
                    let state = load_initial_state(&runtime.paths)?;
                    runtime.send(UiResponse::ReloadUi { state })?;
                    relaunch_background_process(&runtime.paths)?;
                    runtime.send(UiResponse::SaveSucceeded)?;
                    Ok(())
                }
                Err(error) => runtime.send(UiResponse::SetStructuredError {
                    message: format!("{error:#}"),
                }),
            },
            IpcRequestPayload::SaveRaw { raw } => match persist_raw_config(&runtime.paths, &raw) {
                Ok(_) => {
                    let state = load_initial_state(&runtime.paths)?;
                    runtime.send(UiResponse::ReloadUi { state })?;
                    relaunch_background_process(&runtime.paths)?;
                    runtime.send(UiResponse::SaveSucceeded)?;
                    Ok(())
                }
                Err(error) => runtime.send(UiResponse::SetRawError {
                    message: format!("{error:#}"),
                }),
            },
            IpcRequestPayload::BrowseSound { field } => {
                if let Some(path) = choose_sound_file()? {
                    runtime.send(UiResponse::ReplaceSoundPath {
                        field,
                        value: path.display().to_string(),
                    })?;
                }
                Ok(())
            }
            IpcRequestPayload::PreviewSound { role, path } => {
                preview_feedback_sound(&runtime.paths, &role, &path)?;
                Ok(())
            }
            IpcRequestPayload::RefreshModelStatus { form } => {
                let message = match form.into_config() {
                    Ok(config) => model_status_message(&runtime.paths, &config)?,
                    Err(error) => format!("{error:#}"),
                };
                runtime.send(UiResponse::UpdateModelStatus { message })
            }
            IpcRequestPayload::CloseWindow => {
                runtime.webview.take();
                *control_flow = ControlFlow::Exit;
                Ok(())
            }
        }
    }

    impl SettingsRuntime {
        fn send(&self, response: UiResponse) -> Result<()> {
            let Some(webview) = &self.webview else {
                return Ok(());
            };
            // Same pattern as `build_initialization_script`: embed a JSON string literal so the
            // WebView script source never contains raw `</script>` (or other sequences) from
            // `raw_toml`, which would truncate the injected script and corrupt reload payloads.
            let inner =
                serde_json::to_string(&response).context("failed to serialize UI response")?;
            let literal =
                serde_json::to_string(&inner).context("failed to escape UI response for JS")?;
            webview
                .evaluate_script(&format!("window.__chirprReceive(JSON.parse({literal}));"))
                .context("failed to send response to settings webview")
        }

        fn show_error(&self, message: &str) {
            show_message_box(message, MB_OK | MB_ICONERROR);
        }

        fn reload_webview_if_needed(&mut self) -> Result<()> {
            let snapshot = load_settings_ui_snapshot(&self.paths)?;
            if snapshot.newest_write <= self.ui_snapshot.newest_write {
                return Ok(());
            }

            self.webview.take();
            let webview = build_webview(&self.window, &self.paths, &snapshot, self.proxy.clone())?;
            self.webview = Some(webview);
            self.ui_snapshot = snapshot;
            Ok(())
        }
    }

    fn build_html(assets: &super::SettingsUiAssets) -> String {
        assets
            .html
            .replace("__CHIRPR_SETTINGS_CSS__", &assets.css)
            .replace("__CHIRPR_SETTINGS_JS__", &assets.js)
    }

    fn build_initialization_script(initial_state: &InitialState) -> Result<String> {
        let json =
            serde_json::to_string(initial_state).context("failed to serialize initial settings")?;
        let json_literal =
            serde_json::to_string(&json).context("failed to prepare settings state for JS")?;
        Ok(format!(
            "window.__CHIRPR_INITIAL_STATE = JSON.parse({json_literal});"
        ))
    }

    fn show_message_box(message: &str, style: u32) {
        let title = wide(SETTINGS_WINDOW_TITLE);
        let message = wide(message);
        unsafe {
            MessageBoxW(HWND::default(), message.as_ptr(), title.as_ptr(), style);
        }
    }

    fn preview_feedback_sound(paths: &ProjectPaths, role: &str, path: &str) -> Result<()> {
        let sounds_root = paths.assets_root.join("sounds");
        let volume = ChirpConfig::load(paths)
            .map(|cfg| cfg.audio_feedback_volume)
            .unwrap_or_else(|_| ChirpConfig::default().audio_feedback_volume)
            .max(0.2);
        let feedback = AudioFeedback::new(true, volume, sounds_root);
        let trimmed = path.trim();
        let override_path = if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        };
        let played = match role {
            "start" => feedback.try_play_start(override_path.as_deref()),
            "stop" => feedback.try_play_stop(override_path.as_deref()),
            "error" => feedback.try_play_error(override_path.as_deref()),
            _ => false,
        };
        if !played {
            unsafe {
                let _ = MessageBeep(MB_ICONASTERISK);
            }
        }
        Ok(())
    }

    fn choose_sound_file() -> Result<Option<PathBuf>> {
        let mut file_buffer = vec![0_u16; 1024];
        let filter = wide("Wave Files (*.wav)\0*.wav\0All Files (*.*)\0*.*\0\0");
        let mut ofn = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            lpstrFilter: filter.as_ptr(),
            lpstrFile: file_buffer.as_mut_ptr(),
            nMaxFile: file_buffer.len() as u32,
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY,
            ..unsafe { std::mem::zeroed() }
        };

        let opened = unsafe { GetOpenFileNameW(&mut ofn) };
        if opened == 0 {
            return Ok(None);
        }

        let len = file_buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(file_buffer.len());
        Ok(Some(PathBuf::from(String::from_utf16_lossy(
            &file_buffer[..len],
        ))))
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid system time")
            .as_nanos();
        std::env::temp_dir().join(format!("chirpr-settings-{name}-{nanos}"))
    }

    fn sample_paths(root: PathBuf) -> ProjectPaths {
        ProjectPaths {
            config_path: root.join("config.toml"),
            assets_root: root.join("assets"),
            models_root: root.join("assets").join("models"),
            project_root: root,
        }
    }

    #[test]
    fn background_launch_args_include_config_path() {
        let root = unique_temp_dir("launch-args");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("config.toml"),
            "primary_shortcut = \"ctrl+shift+space\"\n",
        )
        .unwrap();
        let paths = sample_paths(root.clone());
        let args = build_background_launch_args(&paths).unwrap();
        assert_eq!(args[0], OsString::from("--config"));
        assert_eq!(args[1], fs::canonicalize(&paths.config_path).unwrap());
    }

    #[test]
    fn structured_form_round_trips_config() {
        let mut config = ChirpConfig {
            primary_shortcut: "rightctrl".into(),
            recording_mode: "hold".into(),
            language: Some("en-US".into()),
            threads: Some(4),
            ..ChirpConfig::default()
        };
        config
            .word_overrides
            .insert("parra keat".into(), "parakeet".into());

        let form = StructuredSettings::from_config(&config);
        let rebuilt = form.into_config().unwrap();

        assert_eq!(rebuilt.primary_shortcut, "rightctrl");
        assert_eq!(rebuilt.recording_mode, "hold");
        assert_eq!(rebuilt.threads, Some(4));
        assert_eq!(
            rebuilt.word_overrides.get("parra keat"),
            Some(&"parakeet".to_string())
        );
    }

    #[test]
    fn structured_form_accepts_empty_replacement() {
        let form = StructuredSettings {
            word_overrides: vec![WordOverrideEntry {
                spoken: "um".into(),
                replacement: "".into(),
            }],
            ..StructuredSettings::from_config(&ChirpConfig::default())
        };
        let rebuilt = form.into_config().unwrap();
        assert_eq!(rebuilt.word_overrides.get("um"), Some(&String::new()));
    }

    #[test]
    fn raw_save_preserves_user_toml_text() {
        let root = unique_temp_dir("raw-preserve");
        fs::create_dir_all(root.join("assets")).unwrap();
        let paths = sample_paths(root);
        let raw = "primary_shortcut = \"rightctrl\"\nrecording_mode = \"hold\"\n";

        let config = persist_raw_config(&paths, raw).unwrap();
        let written = fs::read_to_string(&paths.config_path).unwrap();

        assert_eq!(written, raw);
        assert_eq!(config.primary_shortcut, "rightctrl");
        assert_eq!(config.recording_mode, "hold");
    }

    #[test]
    fn raw_save_rejects_invalid_toml() {
        let root = unique_temp_dir("raw-invalid");
        fs::create_dir_all(root.join("assets")).unwrap();
        let paths = sample_paths(root);

        let error = persist_raw_config(&paths, "primary_shortcut = [")
            .unwrap_err()
            .to_string();

        assert!(error.contains("TOML parse error"));
    }

    #[test]
    fn structured_save_writes_canonical_config() {
        let root = unique_temp_dir("structured");
        fs::create_dir_all(root.join("assets")).unwrap();
        let sound = root.join("ding.wav");
        fs::write(&sound, "tone").unwrap();
        let paths = sample_paths(root);
        let config = ChirpConfig {
            primary_shortcut: "rightctrl".into(),
            recording_mode: "hold".into(),
            start_sound_path: Some(sound),
            ..ChirpConfig::default()
        };

        persist_structured_config(&paths, &config).unwrap();
        let written = fs::read_to_string(&paths.config_path).unwrap();

        assert!(written.contains("primary_shortcut = \"rightctrl\""));
        assert!(written.contains("recording_mode = \"hold\""));
    }

    #[test]
    fn structured_save_preserves_end_of_line_comments() {
        let root = unique_temp_dir("structured-comments");
        fs::create_dir_all(root.join("assets")).unwrap();
        let paths = sample_paths(root);
        let initial = r#"primary_shortcut = "ctrl+shift+space"  # shortcut comment
recording_mode = "toggle"  # recording comment
"#;
        fs::write(&paths.config_path, initial).unwrap();

        let mut config = ChirpConfig::from_toml_str(initial).unwrap();
        config.recording_mode = "hold".into();

        persist_structured_config(&paths, &config).unwrap();
        let written = fs::read_to_string(&paths.config_path).unwrap();

        assert!(
            written.contains("# shortcut comment"),
            "expected primary_shortcut comment preserved:\n{written}"
        );
        assert!(
            written.contains("# recording comment"),
            "expected recording_mode comment preserved:\n{written}"
        );
        assert!(written.contains("recording_mode = \"hold\""), "{written}");
    }

    #[test]
    fn model_status_reports_missing_files_without_failing() {
        let root = unique_temp_dir("model-status");
        fs::create_dir_all(root.join("assets").join("models")).unwrap();
        let paths = sample_paths(root);
        let message = model_status_message(&paths, &ChirpConfig::default()).unwrap();
        assert!(message.contains("missing"));
        assert!(message.contains("chirpr-cli setup"));
    }
}
