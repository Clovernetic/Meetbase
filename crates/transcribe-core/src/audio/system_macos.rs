//! macOS system-audio capture via a CoreAudio process tap.
//!
//! Available on macOS 14.4+. A global mono tap captures the post-mix output
//! of every process; an aggregate device built from *only* that tap (adding
//! the physical output device too would duplicate the audio) drives an IO
//! proc that resamples to 16 kHz and forwards into the sink.
//!
//! The first capture triggers the system audio-recording permission prompt
//! (`NSAudioCaptureUsageDescription`). If the user denies it, the tap
//! delivers silence — CoreAudio gives no error — so the UI should tell users
//! to check System Settings when transcripts come out empty with audible
//! meeting sound.

use cidre::{cat, cf, core_audio as ca, ns, os};
use tracing::info;

use crate::error::{CoreError, Result};

use super::capture::CaptureHandle;
use super::resample::{downmix_to_mono, StreamResampler};

type SampleSink = Box<dyn FnMut(&[f32]) + Send>;

/// Everything the IO proc needs; owned by [`TapCaptureGuard`] so it outlives
/// the running device.
struct TapCtx {
    resampler: StreamResampler,
    sink: SampleSink,
}

/// Keeps the capture alive. Field order = drop order: the started device
/// stops first (no more IO proc callbacks), then the tap is destroyed, and
/// only then the context the callbacks referenced.
struct TapCaptureGuard {
    _device: ca::hardware::StartedDevice<ca::AggregateDevice>,
    _tap: ca::TapGuard,
    _ctx: Box<TapCtx>,
}

extern "C" fn io_proc(
    _device: ca::Device,
    _now: &cat::AudioTimeStamp,
    input_data: &cat::AudioBufList<1>,
    _input_time: &cat::AudioTimeStamp,
    _output_data: &mut cat::AudioBufList<1>,
    _output_time: &cat::AudioTimeStamp,
    ctx: Option<&mut TapCtx>,
) -> os::Status {
    let Some(ctx) = ctx else {
        return os::Status::NO_ERR;
    };
    let buf = &input_data.buffers[0];
    let samples = buf.data_bytes_size as usize / std::mem::size_of::<f32>();
    if samples == 0 || buf.data.is_null() {
        return os::Status::NO_ERR;
    }
    let data = unsafe { std::slice::from_raw_parts(buf.data as *const f32, samples) };
    let mono = downmix_to_mono(data, (buf.number_channels as usize).max(1));
    if let Ok(out) = ctx.resampler.process(&mono) {
        if !out.is_empty() {
            (ctx.sink)(&out);
        }
    }
    os::Status::NO_ERR
}

fn supports_process_tap() -> bool {
    ns::ProcessInfo::current().is_os_at_least_version(cidre::api::OsVersion {
        major: 14,
        minor: 4,
        patch: 0,
    })
}

/// Starts the tap, feeding 16 kHz mono samples into `sink`.
pub fn start_system_audio(sink: impl FnMut(&[f32]) + Send + 'static) -> Result<CaptureHandle> {
    if !supports_process_tap() {
        return Err(CoreError::AudioDevice(
            "system-audio capture needs macOS 14.4 or newer; recording microphone only".into(),
        ));
    }

    // The aggregate device is anchored to the default output device's UID so
    // CoreAudio clocks it sensibly.
    let output_device = ca::System::default_output_device()
        .map_err(|e| CoreError::AudioDevice(format!("no default output device: {e:?}")))?;
    let output_uid = output_device
        .uid()
        .map_err(|e| CoreError::AudioDevice(format!("output device uid: {e:?}")))?;
    let output_name = output_device
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "system audio".into());

    // Global mono tap of all processes (excluding none). This is the point
    // where macOS prompts for audio-recording permission on first use.
    let tap_desc = ca::TapDesc::with_mono_global_tap_excluding_processes(&ns::Array::new());
    let tap = tap_desc
        .create_process_tap()
        .map_err(|e| CoreError::AudioDevice(format!("create process tap: {e:?}")))?;
    let tap_uid = tap
        .uid()
        .map_err(|e| CoreError::AudioDevice(format!("tap uid: {e:?}")))?;
    let asbd = tap
        .asbd()
        .map_err(|e| CoreError::AudioDevice(format!("tap format: {e:?}")))?;
    let sample_rate = asbd.sample_rate as u32;
    info!(
        sample_rate,
        channels = asbd.channels_per_frame,
        output = %output_name,
        "created CoreAudio process tap"
    );

    let sub_tap = cf::DictionaryOf::with_keys_values(
        &[ca::hardware::sub_tap_keys::uid()],
        &[tap_uid.as_type_ref()],
    );
    // Tap only — adding the output device as a sub-device too would capture
    // the same audio twice (echo).
    let agg_desc = cf::DictionaryOf::with_keys_values(
        &[
            ca::aggregate_device_keys::is_private(),
            ca::aggregate_device_keys::is_stacked(),
            ca::aggregate_device_keys::tap_auto_start(),
            ca::aggregate_device_keys::name(),
            ca::aggregate_device_keys::main_sub_device(),
            ca::aggregate_device_keys::uid(),
            ca::aggregate_device_keys::tap_list(),
        ],
        &[
            cf::Boolean::value_true().as_type_ref(),
            cf::Boolean::value_false(),
            cf::Boolean::value_true(),
            cf::str!(c"meetbase-system-tap").as_type_ref(),
            &output_uid,
            &cf::Uuid::new().to_cf_string(),
            &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
        ],
    );

    let agg_device = ca::AggregateDevice::with_desc(&agg_desc)
        .map_err(|e| CoreError::AudioDevice(format!("create aggregate device: {e:?}")))?;

    let mut ctx = Box::new(TapCtx {
        resampler: StreamResampler::new(sample_rate)?,
        sink: Box::new(sink),
    });

    let proc_id = agg_device
        .create_io_proc_id(io_proc, Some(ctx.as_mut()))
        .map_err(|e| CoreError::AudioDevice(format!("create io proc: {e:?}")))?;
    let started = ca::device_start(agg_device, Some(proc_id))
        .map_err(|e| CoreError::AudioDevice(format!("start aggregate device: {e:?}")))?;
    info!("system-audio tap running");

    Ok(CaptureHandle::new(
        TapCaptureGuard {
            _device: started,
            _tap: tap,
            _ctx: ctx,
        },
        output_name,
        sample_rate,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual hardware test: captures 3 s of system audio while `say` speaks
    /// through the default output, and asserts non-silent samples arrived.
    /// Requires macOS 14.4+ and the audio-recording permission for the
    /// terminal running the test.
    #[test]
    #[ignore = "requires audio hardware + recording permission"]
    fn captures_played_audio() {
        if !supports_process_tap() {
            eprintln!("skipping: macOS < 14.4");
            return;
        }
        let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
        let sink_buf = collected.clone();
        let _handle = start_system_audio(move |s| {
            sink_buf.lock().unwrap().extend_from_slice(s);
        })
        .expect("start tap");

        // Speak through the speakers while the tap runs.
        let speaker = std::thread::spawn(|| {
            let _ = std::process::Command::new("say")
                .arg("Testing system audio capture for meetbase.")
                .status();
        });
        std::thread::sleep(std::time::Duration::from_secs(3));
        speaker.join().ok();

        let samples = collected.lock().unwrap();
        // ~3 s at 16 kHz minus startup latency; be generous.
        assert!(
            samples.len() > 16_000,
            "too few samples captured: {}",
            samples.len()
        );
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!(
            rms > 1e-4,
            "captured only silence (rms {rms:e}) — permission denied or no audio played"
        );
        println!("captured {} samples, rms {rms:.4}", samples.len());
    }
}
