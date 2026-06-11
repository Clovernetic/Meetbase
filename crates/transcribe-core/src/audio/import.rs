//! Decoding of audio/video files into the pipeline format (16 kHz mono f32).
//!
//! Uses symphonia, which handles the common containers (wav, mp3, m4a/aac,
//! ogg, flac, mp4/mov audio tracks) without external binaries.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{CoreError, Result};

use super::resample::{downmix_to_mono, StreamResampler};

/// Decodes a media file to 16 kHz mono f32 samples.
pub fn decode_to_pipeline_format(path: &Path) -> Result<Vec<f32>> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| CoreError::AudioDecode(format!("unrecognized media format: {e}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| CoreError::AudioDecode("no decodable audio track".into()))?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| CoreError::AudioDecode(format!("decoder init: {e}")))?;

    let mut resampler: Option<StreamResampler> = None;
    let mut channels = 0usize;
    let mut out = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(CoreError::AudioDecode(format!("read packet: {e}"))),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // Skip over corrupt frames rather than failing the whole import.
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(CoreError::AudioDecode(format!("decode: {e}"))),
        };

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            channels = spec.channels.count();
            resampler = Some(StreamResampler::new(spec.rate)?);
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);
        let mono = downmix_to_mono(buf.samples(), channels.max(1));
        out.extend(resampler.as_mut().unwrap().process(&mono)?);
    }

    if let Some(rs) = resampler.as_mut() {
        out.extend(rs.finish()?);
    }
    if out.is_empty() {
        return Err(CoreError::AudioDecode("file contained no audio samples".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::PIPELINE_SAMPLE_RATE;

    /// Writes a WAV with a sine tone and verifies the decoded length/content.
    #[test]
    fn decodes_wav_to_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..44_100 {
            let s = (0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44_100.0).sin()
                * i16::MAX as f32) as i16;
            writer.write_sample(s).unwrap(); // L
            writer.write_sample(s).unwrap(); // R
        }
        writer.finalize().unwrap();

        let samples = decode_to_pipeline_format(&path).unwrap();
        let expected = PIPELINE_SAMPLE_RATE as f32; // 1 second
        assert!(
            (samples.len() as f32 - expected).abs() / expected < 0.02,
            "got {} samples, expected ~{expected}",
            samples.len()
        );
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!(rms > 0.2, "tone lost in decode, rms = {rms}");
    }

    #[test]
    fn rejects_non_media_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-audio.txt");
        std::fs::write(&path, "definitely not audio").unwrap();
        assert!(decode_to_pipeline_format(&path).is_err());
    }
}
