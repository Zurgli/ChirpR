use std::path::Path;

use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavReader};

#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub mono_samples: Vec<f32>,
}

impl AudioBuffer {
    pub fn load_wav(path: &Path) -> Result<Self> {
        let mut reader = WavReader::open(path)
            .with_context(|| format!("failed to open wav file {}", path.display()))?;
        let spec = reader.spec();

        if spec.channels == 0 {
            bail!("wav file {} has zero channels", path.display());
        }

        let interleaved = match (spec.sample_format, spec.bits_per_sample) {
            (SampleFormat::Int, 8) => read_i8_samples(&mut reader)?,
            (SampleFormat::Int, 16) => read_i16_samples(&mut reader)?,
            (SampleFormat::Int, 24) | (SampleFormat::Int, 32) => read_i32_samples(&mut reader)?,
            (SampleFormat::Float, 32) => read_float_samples(&mut reader)?,
            _ => bail!(
                "unsupported wav format in {}: {:?} {}-bit",
                path.display(),
                spec.sample_format,
                spec.bits_per_sample
            ),
        };

        let mono_samples = mixdown_to_mono(&interleaved, spec.channels as usize);

        Ok(Self {
            sample_rate_hz: spec.sample_rate,
            channels: spec.channels,
            mono_samples,
        })
    }

    pub fn require_sample_rate(&self, expected_hz: u32) -> Result<()> {
        if self.sample_rate_hz == expected_hz {
            Ok(())
        } else {
            bail!(
                "expected {} Hz audio but got {} Hz",
                expected_hz,
                self.sample_rate_hz
            )
        }
    }
}

fn read_i8_samples(reader: &mut WavReader<std::io::BufReader<std::fs::File>>) -> Result<Vec<f32>> {
    reader
        .samples::<i8>()
        .map(|sample| {
            sample
                .map(|value| value as f32 / i8::MAX as f32)
                .context("failed to read integer wav sample")
        })
        .collect()
}

fn read_i16_samples(reader: &mut WavReader<std::io::BufReader<std::fs::File>>) -> Result<Vec<f32>> {
    reader
        .samples::<i16>()
        .map(|sample| {
            sample
                .map(|value| value as f32 / i16::MAX as f32)
                .context("failed to read integer wav sample")
        })
        .collect()
}

fn read_i32_samples(reader: &mut WavReader<std::io::BufReader<std::fs::File>>) -> Result<Vec<f32>> {
    reader
        .samples::<i32>()
        .map(|sample| {
            sample
                .map(|value| value as f32 / i32::MAX as f32)
                .context("failed to read integer wav sample")
        })
        .collect()
}

fn read_float_samples(
    reader: &mut WavReader<std::io::BufReader<std::fs::File>>,
) -> Result<Vec<f32>> {
    reader
        .samples::<f32>()
        .map(|sample| sample.context("failed to read float wav sample"))
        .collect()
}

fn mixdown_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }

    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_and_mixdowns_stereo_wav() {
        let path = temp_wav_path("stereo");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        {
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            writer.write_sample::<i16>(i16::MAX).unwrap();
            writer.write_sample::<i16>(0).unwrap();
            writer.write_sample::<i16>(0).unwrap();
            writer.write_sample::<i16>(i16::MAX).unwrap();
            writer.finalize().unwrap();
        }

        let audio = AudioBuffer::load_wav(&path).unwrap();
        assert_eq!(audio.sample_rate_hz, 16_000);
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.mono_samples.len(), 2);
        assert!((audio.mono_samples[0] - 0.5).abs() < 0.01);
        assert!((audio.mono_samples[1] - 0.5).abs() < 0.01);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let audio = AudioBuffer {
            sample_rate_hz: 44_100,
            channels: 1,
            mono_samples: vec![0.0, 1.0],
        };

        let error = audio.require_sample_rate(16_000).unwrap_err().to_string();
        assert!(error.contains("expected 16000 Hz audio"));
    }

    fn temp_wav_path(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("chirp-rust-{label}-{unique}.wav"))
    }
}
