//! Conversion of arbitrary capture formats to the pipeline's 16 kHz mono f32.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::error::{CoreError, Result};

use super::PIPELINE_SAMPLE_RATE;

/// Downmix interleaved multi-channel samples to mono by averaging channels.
///
/// A no-op (copy) for mono input. Panics are avoided for ragged trailing
/// frames by truncating to whole frames.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    assert!(channels > 0, "channels must be >= 1");
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Streaming resampler from an arbitrary input rate to 16 kHz mono.
///
/// Wraps rubato's sinc resampler with internal buffering so callers can feed
/// capture callbacks of any size and receive whatever output is ready.
pub struct StreamResampler {
    inner: Option<SincFixedIn<f32>>,
    /// Samples not yet consumed because the resampler works in fixed blocks.
    pending: Vec<f32>,
    block_size: usize,
}

impl StreamResampler {
    /// Creates a resampler from `input_rate` to [`PIPELINE_SAMPLE_RATE`].
    ///
    /// When the input is already 16 kHz, resampling is bypassed entirely.
    pub fn new(input_rate: u32) -> Result<Self> {
        if input_rate == PIPELINE_SAMPLE_RATE {
            return Ok(Self {
                inner: None,
                pending: Vec::new(),
                block_size: 0,
            });
        }
        let block_size = 1024;
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };
        let inner = SincFixedIn::<f32>::new(
            PIPELINE_SAMPLE_RATE as f64 / input_rate as f64,
            2.0,
            params,
            block_size,
            1,
        )
        .map_err(|e| CoreError::AudioDecode(format!("resampler init: {e}")))?;
        Ok(Self {
            inner: Some(inner),
            pending: Vec::with_capacity(block_size * 2),
            block_size,
        })
    }

    /// Feeds mono samples at the input rate; returns any 16 kHz output ready.
    pub fn process(&mut self, mono: &[f32]) -> Result<Vec<f32>> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(mono.to_vec());
        };
        self.pending.extend_from_slice(mono);
        let mut out = Vec::new();
        while self.pending.len() >= self.block_size {
            let block: Vec<f32> = self.pending.drain(..self.block_size).collect();
            let processed = inner
                .process(&[block], None)
                .map_err(|e| CoreError::AudioDecode(format!("resample: {e}")))?;
            out.extend_from_slice(&processed[0]);
        }
        Ok(out)
    }

    /// Flushes remaining buffered samples (pad with silence to a full block).
    pub fn finish(&mut self) -> Result<Vec<f32>> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(Vec::new());
        };
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut block: Vec<f32> = self.pending.drain(..).collect();
        let real_len = block.len();
        block.resize(self.block_size, 0.0);
        let processed = inner
            .process(&[block], None)
            .map_err(|e| CoreError::AudioDecode(format!("resample flush: {e}")))?;
        // Trim the tail produced by the silence padding.
        let ratio = PIPELINE_SAMPLE_RATE as f64 / self.input_block_rate();
        let useful = (real_len as f64 * ratio).round() as usize;
        let mut out = processed[0].clone();
        out.truncate(useful.min(out.len()));
        Ok(out)
    }

    fn input_block_rate(&self) -> f64 {
        // Derive the input rate back from the configured ratio.
        match &self.inner {
            Some(r) => {
                self.block_size as f64 / r.output_frames_next() as f64 * PIPELINE_SAMPLE_RATE as f64
            }
            None => PIPELINE_SAMPLE_RATE as f64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_stereo_averages_channels() {
        let interleaved = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        let mono = downmix_to_mono(&interleaved, 2);
        assert_eq!(mono, vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        let samples = [0.1, 0.2, 0.3];
        assert_eq!(downmix_to_mono(&samples, 1), samples.to_vec());
    }

    #[test]
    fn passthrough_at_pipeline_rate() {
        let mut rs = StreamResampler::new(PIPELINE_SAMPLE_RATE).unwrap();
        let input: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let out = rs.process(&input).unwrap();
        assert_eq!(out, input);
        assert!(rs.finish().unwrap().is_empty());
    }

    #[test]
    fn resamples_48k_to_16k_with_correct_length() {
        let mut rs = StreamResampler::new(48_000).unwrap();
        // 1 second of a 440 Hz sine at 48 kHz.
        let input: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        let mut out = rs.process(&input).unwrap();
        out.extend(rs.finish().unwrap());
        // Expect ~16000 samples (±2% for filter edges).
        let expected = 16_000.0;
        assert!(
            (out.len() as f32 - expected).abs() / expected < 0.02,
            "got {} samples, expected ~{expected}",
            out.len()
        );
        // Energy should be preserved in rough terms (sine survives resampling).
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms > 0.5, "rms too low: {rms}");
    }
}
