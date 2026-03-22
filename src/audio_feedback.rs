use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::AudioBuffer;

#[derive(Debug, Clone)]
pub struct AudioFeedback {
    enabled: bool,
    volume: f32,
    sounds_root: PathBuf,
}

impl AudioFeedback {
    pub fn new(enabled: bool, volume: f32, sounds_root: PathBuf) -> Self {
        Self {
            enabled,
            volume: volume.clamp(0.0, 1.0),
            sounds_root,
        }
    }

    pub fn play_start(&self, override_path: Option<&Path>) {
        self.play_optional_sound("ping-up.wav", override_path);
    }

    pub fn play_stop(&self, override_path: Option<&Path>) {
        self.play_optional_sound("ping-down.wav", override_path);
    }

    pub fn play_error(&self, override_path: Option<&Path>) {
        if !self.enabled {
            return;
        }

        if self.try_play_path(override_path).is_some() {
            return;
        }

        #[cfg(target_os = "windows")]
        unsafe {
            let _ = windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(
                windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONHAND,
            );
        }
    }

    fn play_optional_sound(&self, asset_name: &str, override_path: Option<&Path>) {
        if !self.enabled {
            return;
        }

        if self.try_play_path(override_path).is_some() {
            return;
        }

        let asset_path = self.sounds_root.join(asset_name);
        let _ = self.try_play_path(Some(asset_path.as_path()));
    }

    fn try_play_path(&self, path: Option<&Path>) -> Option<()> {
        let path = path?;
        if !path.is_file() {
            return None;
        }

        #[cfg(target_os = "windows")]
        if try_play_windows_file(path).is_some() {
            return Some(());
        }

        let audio = AudioBuffer::load_wav(path).ok()?;
        if audio.mono_samples.is_empty() {
            return None;
        }

        spawn_playback(audio, self.volume).ok()
    }
}

#[cfg(target_os = "windows")]
fn try_play_windows_file(path: &Path) -> Option<()> {
    use std::os::windows::ffi::OsStrExt;

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();

    let played = unsafe {
        windows_sys::Win32::Media::Audio::PlaySoundW(
            wide_path.as_ptr(),
            std::ptr::null_mut(),
            windows_sys::Win32::Media::Audio::SND_ASYNC
                | windows_sys::Win32::Media::Audio::SND_FILENAME
                | windows_sys::Win32::Media::Audio::SND_NODEFAULT,
        )
    };

    if played != 0 { Some(()) } else { None }
}

fn spawn_playback(audio: AudioBuffer, volume: f32) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("failed to find default output device"))?;
    let supported_config = device.default_output_config()?;
    let stream_config: cpal::StreamConfig = supported_config.clone().into();
    let audio = audio.resample_to(stream_config.sample_rate.0)?;
    let channels = stream_config.channels as usize;
    let frame_count = audio.mono_samples.len();
    let scaled_samples = audio
        .mono_samples
        .into_iter()
        .map(|sample| sample.clamp(-1.0, 1.0) * volume)
        .collect::<Vec<f32>>();

    let stream = match supported_config.sample_format() {
        cpal::SampleFormat::F32 => {
            build_output_stream::<f32>(&device, &stream_config, channels, scaled_samples)?
        }
        cpal::SampleFormat::I16 => {
            build_output_stream::<i16>(&device, &stream_config, channels, scaled_samples)?
        }
        cpal::SampleFormat::U16 => {
            build_output_stream::<u16>(&device, &stream_config, channels, scaled_samples)?
        }
        sample_format => {
            return Err(anyhow::anyhow!(
                "unsupported output sample format: {sample_format:?}"
            ));
        }
    };

    thread::spawn(move || {
        let _keep_stream_alive = stream;
        let total_frames = frame_count as f32;
        let rate = stream_config.sample_rate.0 as f32;
        let playback_seconds = if rate > 0.0 { total_frames / rate } else { 0.0 };
        thread::sleep(Duration::from_secs_f32(playback_seconds + 0.05));
    });

    Ok(())
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    samples: Vec<f32>,
) -> anyhow::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut index = 0usize;
    let total_samples = samples.len();

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            for frame in data.chunks_mut(channels) {
                let value = if index < total_samples {
                    let sample = samples[index];
                    index += 1;
                    sample
                } else {
                    0.0
                };

                for output in frame {
                    *output = T::from_sample(value);
                }
            }
        },
        move |_error| {},
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_stop_methods_are_safe_when_assets_missing() {
        let feedback = AudioFeedback::new(false, 0.25, PathBuf::from("."));
        feedback.play_start(None);
        feedback.play_stop(None);
        feedback.play_error(None);
    }
}
