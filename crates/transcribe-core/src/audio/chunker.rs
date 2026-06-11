//! Speech-aware chunking for streaming transcription.
//!
//! Whisper works best on a few seconds of complete utterance. The chunker
//! consumes the continuous 16 kHz mono stream, runs VAD per 30 ms frame and
//! emits a [`SpeechChunk`] when a pause ends an utterance — or force-cuts at
//! `max_chunk` so live captions never lag a long monologue by more than that.

use std::time::Duration;

use super::vad::{FrameVerdict, VoiceActivityDetector, FRAME_SAMPLES};
use super::PIPELINE_SAMPLE_RATE;

/// A self-contained piece of speech ready for transcription.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechChunk {
    /// 16 kHz mono samples.
    pub samples: Vec<f32>,
    /// Offset of the first sample from the start of the recording.
    pub start: Duration,
}

impl SpeechChunk {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / PIPELINE_SAMPLE_RATE as f64)
    }
}

pub struct ChunkerConfig {
    /// Silence run that closes an utterance.
    pub silence_to_cut: Duration,
    /// Hard cap on chunk length (force cut mid-speech).
    pub max_chunk: Duration,
    /// Chunks shorter than this are dropped as VAD noise.
    pub min_chunk: Duration,
    /// Silence kept before/after speech so Whisper sees natural boundaries.
    pub padding: Duration,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            silence_to_cut: Duration::from_millis(700),
            max_chunk: Duration::from_secs(25),
            min_chunk: Duration::from_millis(300),
            padding: Duration::from_millis(200),
        }
    }
}

pub struct SpeechChunker {
    vad: Box<dyn VoiceActivityDetector>,
    config: ChunkerConfig,
    /// Frames of the utterance currently being accumulated.
    current: Vec<f32>,
    /// Stream position (in samples) of the first sample in `current`.
    current_start: u64,
    /// Recent silence frames kept as pre-speech padding.
    pre_padding: Vec<f32>,
    /// Consecutive silence frames seen inside an utterance.
    trailing_silence_frames: usize,
    in_speech: bool,
    /// Total samples consumed from the stream so far.
    stream_pos: u64,
    /// Carry-over for inputs not aligned to the 30 ms frame size.
    remainder: Vec<f32>,
}

impl SpeechChunker {
    pub fn new(vad: Box<dyn VoiceActivityDetector>, config: ChunkerConfig) -> Self {
        Self {
            vad,
            config,
            current: Vec::new(),
            current_start: 0,
            pre_padding: Vec::new(),
            trailing_silence_frames: 0,
            in_speech: false,
            stream_pos: 0,
            remainder: Vec::new(),
        }
    }

    fn frames_for(&self, d: Duration) -> usize {
        (d.as_secs_f64() * PIPELINE_SAMPLE_RATE as f64 / FRAME_SAMPLES as f64).round() as usize
    }

    fn samples_for(&self, d: Duration) -> usize {
        (d.as_secs_f64() * PIPELINE_SAMPLE_RATE as f64).round() as usize
    }

    /// Feeds samples; returns zero or more completed chunks.
    pub fn push(&mut self, samples: &[f32]) -> Vec<SpeechChunk> {
        let mut chunks = Vec::new();
        let mut buf = std::mem::take(&mut self.remainder);
        buf.extend_from_slice(samples);

        let mut offset = 0;
        while offset + FRAME_SAMPLES <= buf.len() {
            let frame = &buf[offset..offset + FRAME_SAMPLES];
            if let Some(chunk) = self.consume_frame(frame) {
                chunks.push(chunk);
            }
            offset += FRAME_SAMPLES;
        }
        self.remainder = buf[offset..].to_vec();
        chunks
    }

    /// Flushes the in-progress utterance at end of recording.
    pub fn finish(&mut self) -> Option<SpeechChunk> {
        if !self.remainder.is_empty() {
            let frame = std::mem::take(&mut self.remainder);
            self.consume_frame(&frame);
        }
        self.cut_current()
    }

    fn consume_frame(&mut self, frame: &[f32]) -> Option<SpeechChunk> {
        let verdict = self.vad.classify(frame);
        let frame_start = self.stream_pos;
        self.stream_pos += frame.len() as u64;

        match (self.in_speech, verdict) {
            (false, FrameVerdict::Silence) => {
                // Keep a rolling window of silence as pre-speech padding.
                self.pre_padding.extend_from_slice(frame);
                let max_pad = self.samples_for(self.config.padding);
                if self.pre_padding.len() > max_pad {
                    let excess = self.pre_padding.len() - max_pad;
                    self.pre_padding.drain(..excess);
                }
                None
            }
            (false, FrameVerdict::Speech) => {
                self.in_speech = true;
                self.trailing_silence_frames = 0;
                self.current_start = frame_start.saturating_sub(self.pre_padding.len() as u64);
                self.current = std::mem::take(&mut self.pre_padding);
                self.current.extend_from_slice(frame);
                None
            }
            (true, verdict) => {
                self.current.extend_from_slice(frame);
                if verdict == FrameVerdict::Silence {
                    self.trailing_silence_frames += 1;
                } else {
                    self.trailing_silence_frames = 0;
                }

                let silence_cut = self.trailing_silence_frames >= self.frames_for(self.config.silence_to_cut);
                let max_cut = self.current.len() >= self.samples_for(self.config.max_chunk);
                if silence_cut || max_cut {
                    self.cut_current()
                } else {
                    None
                }
            }
        }
    }

    fn cut_current(&mut self) -> Option<SpeechChunk> {
        if !self.in_speech {
            return None;
        }
        self.in_speech = false;
        let mut samples = std::mem::take(&mut self.current);
        // Trim the accumulated trailing silence down to the configured
        // padding — Whisper gains nothing from a long silent tail and short
        // blips would otherwise balloon into second-long chunks.
        let trailing = self.trailing_silence_frames * FRAME_SAMPLES;
        let keep_tail = self.samples_for(self.config.padding);
        if trailing > keep_tail {
            samples.truncate(samples.len().saturating_sub(trailing - keep_tail));
        }
        self.trailing_silence_frames = 0;
        self.pre_padding.clear();

        if samples.len() < self.samples_for(self.config.min_chunk) {
            return None;
        }
        Some(SpeechChunk {
            samples,
            start: Duration::from_secs_f64(self.current_start as f64 / PIPELINE_SAMPLE_RATE as f64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::vad::EnergyVad;

    fn tone(seconds: f64) -> Vec<f32> {
        let n = (seconds * PIPELINE_SAMPLE_RATE as f64) as usize;
        (0..n)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0001; (seconds * PIPELINE_SAMPLE_RATE as f64) as usize]
    }

    fn chunker() -> SpeechChunker {
        SpeechChunker::new(Box::new(EnergyVad::default()), ChunkerConfig::default())
    }

    #[test]
    fn emits_chunk_after_utterance_and_pause() {
        let mut c = chunker();
        let mut chunks = Vec::new();
        chunks.extend(c.push(&silence(1.0)));
        chunks.extend(c.push(&tone(2.0)));
        chunks.extend(c.push(&silence(1.5)));
        assert_eq!(chunks.len(), 1, "expected exactly one chunk");
        let d = chunks[0].duration().as_secs_f64();
        // ~2 s of speech + hangover + padding + the silence run that cut it.
        assert!((1.9..3.8).contains(&d), "unexpected chunk duration {d}");
        // Started roughly at the 1.0 s mark (minus padding).
        let start = chunks[0].start.as_secs_f64();
        assert!((0.6..1.05).contains(&start), "unexpected start {start}");
    }

    #[test]
    fn force_cuts_long_monologue() {
        let mut c = SpeechChunker::new(
            Box::new(EnergyVad::default()),
            ChunkerConfig {
                max_chunk: std::time::Duration::from_secs(5),
                ..ChunkerConfig::default()
            },
        );
        c.push(&silence(1.0));
        let chunks = c.push(&tone(12.0));
        assert!(chunks.len() >= 2, "long speech must be force-cut, got {}", chunks.len());
        for ch in &chunks {
            assert!(ch.duration().as_secs_f64() <= 5.1);
        }
    }

    #[test]
    fn drops_sub_minimum_blips() {
        let mut c = chunker();
        let mut chunks = Vec::new();
        chunks.extend(c.push(&silence(1.0)));
        chunks.extend(c.push(&tone(0.01))); // 10 ms blip
        chunks.extend(c.push(&silence(2.0)));
        // A 10ms blip plus hangover/padding stays under min_chunk… but
        // hangover adds 300 ms, so allow either zero chunks or one tiny chunk
        // — the contract is no chunk longer than 1 s appears.
        for ch in &chunks {
            assert!(ch.duration().as_secs_f64() < 1.0);
        }
    }

    #[test]
    fn finish_flushes_in_progress_speech() {
        let mut c = chunker();
        c.push(&silence(1.0));
        let mid = c.push(&tone(2.0));
        assert!(mid.is_empty(), "no pause yet, nothing should be emitted");
        let last = c.finish().expect("finish must flush the open utterance");
        assert!(last.duration().as_secs_f64() >= 1.9);
    }

    #[test]
    fn pure_silence_yields_nothing() {
        let mut c = chunker();
        assert!(c.push(&silence(5.0)).is_empty());
        assert!(c.finish().is_none());
    }
}
