//! Live audio capture (microphone and, where supported, system audio).
//!
//! Built on cpal. Each source runs its own OS callback thread; samples are
//! downmixed to mono, resampled to 16 kHz and forwarded through the provided
//! sink closure (typically a [`super::mixer::MixerInput`]).
//!
//! System audio:
//! - **Windows**: WASAPI loopback — capture an *output* device as input,
//!   supported natively by cpal.
//! - **macOS**: CoreAudio process tap (macOS 14.4+), see
//!   [`super::system_macos`]. On older macOS the call fails gracefully and
//!   recording continues with the microphone only.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tracing::{info, warn};

use crate::error::{CoreError, Result};

use super::resample::{downmix_to_mono, StreamResampler};

/// A running capture stream. Dropping it stops capture.
///
/// The guard owns whatever keeps the OS callback alive (a `cpal::Stream`,
/// a CoreAudio tap + aggregate device, …). Handles are deliberately not
/// `Send`: they live and die on the capture thread that created them.
pub struct CaptureHandle {
    _guard: Box<dyn std::any::Any>,
    pub device_name: String,
    pub sample_rate: u32,
}

impl CaptureHandle {
    pub(crate) fn new(guard: impl std::any::Any, device_name: String, sample_rate: u32) -> Self {
        Self {
            _guard: Box::new(guard),
            device_name,
            sample_rate,
        }
    }
}

/// Lists input device names, default first.
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .input_devices()
        .map_err(|e| CoreError::AudioDevice(e.to_string()))?
        .filter_map(|d| d.name().ok())
        .collect();
    if let Some(def) = default_name {
        names.retain(|n| n != &def);
        names.insert(0, def);
    }
    Ok(names)
}

/// Starts microphone capture, feeding 16 kHz mono samples into `sink`.
///
/// `device_name: None` selects the system default input.
pub fn start_microphone(
    device_name: Option<&str>,
    mut sink: impl FnMut(&[f32]) + Send + 'static,
) -> Result<CaptureHandle> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .input_devices()
            .map_err(|e| CoreError::AudioDevice(e.to_string()))?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| CoreError::AudioDevice(format!("input device `{name}` not found")))?,
        None => host
            .default_input_device()
            .ok_or_else(|| CoreError::AudioDevice("no default input device".into()))?,
    };
    let resolved_name = device.name().unwrap_or_else(|_| "unknown".into());

    let config = device
        .default_input_config()
        .map_err(|e| CoreError::AudioDevice(format!("no input config: {e}")))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    info!(device = %resolved_name, sample_rate, channels, "starting microphone capture");

    let mut resampler = StreamResampler::new(sample_rate)?;
    let err_fn = |e| warn!("capture stream error: {e}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                let mono = downmix_to_mono(data, channels);
                if let Ok(out) = resampler.process(&mono) {
                    if !out.is_empty() {
                        sink(&out);
                    }
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _| {
                let f32s: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                let mono = downmix_to_mono(&f32s, channels);
                if let Ok(out) = resampler.process(&mono) {
                    if !out.is_empty() {
                        sink(&out);
                    }
                }
            },
            err_fn,
            None,
        ),
        other => {
            return Err(CoreError::AudioDevice(format!(
                "unsupported sample format {other:?}"
            )))
        }
    }
    .map_err(|e| CoreError::AudioDevice(format!("build stream: {e}")))?;

    stream
        .play()
        .map_err(|e| CoreError::AudioDevice(format!("start stream: {e}")))?;

    Ok(CaptureHandle::new(stream, resolved_name, sample_rate))
}

/// Starts system-audio (loopback) capture where the platform supports it.
#[cfg(target_os = "windows")]
pub fn start_system_audio(mut sink: impl FnMut(&[f32]) + Send + 'static) -> Result<CaptureHandle> {
    // WASAPI loopback: open the default *output* device as an input stream.
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| CoreError::AudioDevice("no default output device".into()))?;
    let resolved_name = device.name().unwrap_or_else(|_| "unknown".into());
    let config = device
        .default_output_config()
        .map_err(|e| CoreError::AudioDevice(format!("no output config: {e}")))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    info!(device = %resolved_name, sample_rate, "starting WASAPI loopback capture");

    let mut resampler = StreamResampler::new(sample_rate)?;
    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                let mono = downmix_to_mono(data, channels);
                if let Ok(out) = resampler.process(&mono) {
                    if !out.is_empty() {
                        sink(&out);
                    }
                }
            },
            |e| warn!("loopback stream error: {e}"),
            None,
        )
        .map_err(|e| CoreError::AudioDevice(format!("build loopback stream: {e}")))?;
    stream
        .play()
        .map_err(|e| CoreError::AudioDevice(format!("start loopback stream: {e}")))?;
    Ok(CaptureHandle::new(stream, resolved_name, sample_rate))
}

/// Starts system-audio capture via a CoreAudio process tap (macOS 14.4+).
#[cfg(target_os = "macos")]
pub fn start_system_audio(sink: impl FnMut(&[f32]) + Send + 'static) -> Result<CaptureHandle> {
    super::system_macos::start_system_audio(sink)
}

/// System-audio capture is not yet wired on this platform; see module docs.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn start_system_audio(_sink: impl FnMut(&[f32]) + Send + 'static) -> Result<CaptureHandle> {
    Err(CoreError::AudioDevice(
        "system-audio capture is not yet supported on this platform".into(),
    ))
}
