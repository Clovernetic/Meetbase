//! Whisper-based speech-to-text.
//!
//! [`Transcriber`] wraps one loaded whisper.cpp model. It is `Send` (move it
//! to a dedicated blocking thread) but not `Sync`; transcription is CPU/GPU
//! bound and serialized per model instance by design.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::info;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::audio::PIPELINE_SAMPLE_RATE;
use crate::error::{CoreError, Result};

/// One transcribed utterance with absolute timestamps within the recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptSegment {
    pub text: String,
    /// Start offset from the beginning of the recording.
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Transcription options for one chunk or file.
#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// ISO-639-1 code (`"en"`, `"pl"`, …) or `None` for auto-detection.
    pub language: Option<String>,
    /// Translate the output to English instead of transcribing verbatim.
    pub translate: bool,
    /// Worker threads for whisper.cpp; `0` lets the engine decide.
    pub threads: usize,
}

/// whisper.cpp degrades on extremely short inputs; pad anything shorter than
/// this with trailing silence before transcription.
const MIN_AUDIO: Duration = Duration::from_millis(1100);

pub struct Transcriber {
    state: WhisperState,
    model_id: String,
}

impl Transcriber {
    /// Loads a GGML model from disk. Expensive — keep the instance around.
    ///
    /// GPU acceleration follows the crate features the binary was built with
    /// (`metal`, `cuda`, `vulkan`); whisper.cpp falls back to CPU otherwise.
    pub fn load(model_path: &Path, model_id: &str) -> Result<Self> {
        info!(model = model_id, path = %model_path.display(), "loading whisper model");
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or_else(|| CoreError::Transcription("non-UTF8 model path".into()))?,
            params,
        )
        .map_err(|e| CoreError::Transcription(format!("load model: {e}")))?;
        let state = ctx
            .create_state()
            .map_err(|e| CoreError::Transcription(format!("create state: {e}")))?;
        Ok(Self {
            state,
            model_id: model_id.to_string(),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Transcribes 16 kHz mono samples.
    ///
    /// `offset` is the position of `samples[0]` within the recording and is
    /// added to all returned timestamps, so streaming callers can pass chunk
    /// starts and receive absolute times.
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        offset: Duration,
        options: &TranscribeOptions,
    ) -> Result<Vec<TranscriptSegment>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        let min_samples = (MIN_AUDIO.as_secs_f64() * PIPELINE_SAMPLE_RATE as f64) as usize;
        let padded;
        let input: &[f32] = if samples.len() < min_samples {
            padded = {
                let mut v = samples.to_vec();
                v.resize(min_samples, 0.0);
                v
            };
            &padded
        } else {
            samples
        };

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        // Chunks are independent utterances; carrying decoder context across
        // unrelated chunks causes hallucinated repetitions.
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_translate(options.translate);
        let lang = options.language.as_deref();
        params.set_language(lang.or(Some("auto")));
        let threads = if options.threads == 0 {
            (std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4) as i32
                - 2)
            .max(1)
        } else {
            options.threads as i32
        };
        params.set_n_threads(threads);

        self.state
            .full(params, input)
            .map_err(|e| CoreError::Transcription(format!("whisper full: {e}")))?;

        let offset_ms = offset.as_millis() as u64;
        let n = self.state.full_n_segments();
        let mut segments = Vec::with_capacity(n as usize);
        for i in 0..n {
            let Some(seg) = self.state.get_segment(i) else {
                continue;
            };
            let text = seg
                .to_str_lossy()
                .map_err(|e| CoreError::Transcription(format!("segment text: {e}")))?
                .trim()
                .to_string();
            if text.is_empty() || is_non_speech_marker(&text) {
                continue;
            }
            // whisper timestamps are in centiseconds.
            segments.push(TranscriptSegment {
                text,
                start_ms: offset_ms + (seg.start_timestamp().max(0) as u64) * 10,
                end_ms: offset_ms + (seg.end_timestamp().max(0) as u64) * 10,
            });
        }
        Ok(segments)
    }
}

/// Whisper emits bracketed pseudo-events on silence/noise ("[BLANK_AUDIO]",
/// "(music)", "[typing]"); these are noise for meeting notes.
fn is_non_speech_marker(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_speech_markers_are_detected() {
        assert!(is_non_speech_marker("[BLANK_AUDIO]"));
        assert!(is_non_speech_marker("(applause)"));
        assert!(is_non_speech_marker(" [typing] "));
        assert!(!is_non_speech_marker("Hello [sic] world"));
        assert!(!is_non_speech_marker("Hello"));
    }

    #[test]
    fn default_options_are_auto_language() {
        let opts = TranscribeOptions::default();
        assert!(opts.language.is_none());
        assert!(!opts.translate);
    }

    /// End-to-end model test; runs only when a real model is present
    /// (`cargo test -- --ignored` after downloading `tiny`).
    #[test]
    #[ignore = "requires a downloaded whisper model"]
    fn transcribes_synthetic_silence_to_nothing() {
        let manager = crate::models::ModelManager::with_default_dir().unwrap();
        let path = match manager.resolve("tiny") {
            Ok(p) => p,
            Err(_) => return, // model not downloaded — nothing to verify
        };
        let mut t = Transcriber::load(&path, "tiny").unwrap();
        let silence = vec![0.0f32; PIPELINE_SAMPLE_RATE as usize * 2];
        let segments = t
            .transcribe(&silence, Duration::ZERO, &TranscribeOptions::default())
            .unwrap();
        assert!(segments.is_empty(), "silence produced text: {segments:?}");
    }
}
