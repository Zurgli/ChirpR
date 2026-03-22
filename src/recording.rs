use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cpal::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::AudioBuffer;

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureSummary {
    pub device_name: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub captured_samples: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordingResult {
    pub audio: AudioBuffer,
    pub summary: CaptureSummary,
}

pub struct ActiveRecording {
    stream: cpal::Stream,
    captured: Arc<Mutex<Vec<f32>>>,
    error_slot: Arc<Mutex<Option<String>>>,
    device_name: String,
    sample_rate_hz: u32,
    channels: u16,
}

pub struct MicrophoneRecorder;

impl MicrophoneRecorder {
    pub fn start_default() -> Result<ActiveRecording> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("failed to find default input device")?;
        let device_name = device
            .name()
            .unwrap_or_else(|_| "default-input".to_string());
        let supported_config = device
            .default_input_config()
            .context("failed to get default input config")?;
        let stream_config: cpal::StreamConfig = supported_config.clone().into();
        let sample_rate_hz = stream_config.sample_rate.0;
        let channels = stream_config.channels;

        let captured = Arc::new(Mutex::new(Vec::<f32>::new()));
        let error_slot = Arc::new(Mutex::new(None::<String>));
        let stream = match supported_config.sample_format() {
            cpal::SampleFormat::F32 => build_input_stream::<f32>(
                &device,
                &stream_config,
                channels,
                Arc::clone(&captured),
                Arc::clone(&error_slot),
            )?,
            cpal::SampleFormat::I16 => build_input_stream::<i16>(
                &device,
                &stream_config,
                channels,
                Arc::clone(&captured),
                Arc::clone(&error_slot),
            )?,
            cpal::SampleFormat::U16 => build_input_stream::<u16>(
                &device,
                &stream_config,
                channels,
                Arc::clone(&captured),
                Arc::clone(&error_slot),
            )?,
            sample_format => bail!("unsupported input sample format: {sample_format:?}"),
        };

        stream.play().context("failed to start input stream")?;

        Ok(ActiveRecording {
            stream,
            captured,
            error_slot,
            device_name,
            sample_rate_hz,
            channels,
        })
    }

    pub fn record_for(duration: Duration) -> Result<RecordingResult> {
        if duration.is_zero() {
            bail!("recording duration must be greater than zero");
        }

        let active = Self::start_default()?;
        thread::sleep(duration);
        active.stop()
    }
}

impl ActiveRecording {
    pub fn stop(self) -> Result<RecordingResult> {
        drop(self.stream);

        if let Some(message) = self
            .error_slot
            .lock()
            .map_err(|_| anyhow::anyhow!("audio error state was poisoned"))?
            .take()
        {
            bail!("audio capture failed: {message}");
        }

        let mono_samples = self
            .captured
            .lock()
            .map_err(|_| anyhow::anyhow!("audio buffer was poisoned"))?
            .clone();

        Ok(RecordingResult {
            summary: CaptureSummary {
                device_name: self.device_name,
                sample_rate_hz: self.sample_rate_hz,
                channels: self.channels,
                captured_samples: mono_samples.len(),
            },
            audio: AudioBuffer {
                sample_rate_hz: self.sample_rate_hz,
                channels: 1,
                mono_samples,
            },
        })
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    captured: Arc<Mutex<Vec<f32>>>,
    error_slot: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if let Ok(mut buffer) = captured.lock() {
                    for frame in data.chunks_exact(channels as usize) {
                        let sample = frame
                            .iter()
                            .map(|value| f32::from_sample(*value))
                            .sum::<f32>()
                            / channels as f32;
                        buffer.push(sample);
                    }
                }
            },
            move |error| {
                if let Ok(mut slot) = error_slot.lock() {
                    *slot = Some(error.to_string());
                }
            },
            None,
        )
        .context("failed to build input stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_duration() {
        let error = MicrophoneRecorder::record_for(Duration::from_secs(0))
            .unwrap_err()
            .to_string();
        assert!(error.contains("recording duration must be greater than zero"));
    }
}
