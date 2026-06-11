//! Voice activity detection.
//!
//! The pipeline uses VAD to (a) cut transcription cost by skipping silence and
//! (b) find natural chunk boundaries so Whisper never splits a word in half.
//!
//! The default detector is an adaptive energy gate: cheap, dependency-free and
//! good enough for chunk-boundary detection (Whisper itself is robust to a bit
//! of leading/trailing silence). The trait keeps the door open for a Silero
//! ONNX detector behind a feature flag without touching call sites.

/// Frame size the detectors operate on: 30 ms at 16 kHz.
pub const FRAME_SAMPLES: usize = 480;

/// Verdict for a single 30 ms frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameVerdict {
    Speech,
    Silence,
}

/// A voice activity detector consuming consecutive 30 ms mono 16 kHz frames.
pub trait VoiceActivityDetector: Send {
    /// Classifies one frame. Frames shorter than [`FRAME_SAMPLES`] are allowed
    /// only as the final frame of a stream.
    fn classify(&mut self, frame: &[f32]) -> FrameVerdict;

    /// Resets internal state (e.g. between recordings).
    fn reset(&mut self);
}

/// Adaptive energy-based VAD.
///
/// Tracks a slow-moving noise floor and flags frames whose RMS exceeds the
/// floor by a fixed ratio. Hangover frames keep short intra-word pauses
/// classified as speech so words are not clipped.
pub struct EnergyVad {
    noise_floor: f32,
    hangover_left: u32,
    /// Frames of speech to keep emitting after energy drops (300 ms).
    hangover_frames: u32,
    /// Speech threshold as a multiple of the noise floor.
    threshold_ratio: f32,
}

impl Default for EnergyVad {
    fn default() -> Self {
        Self {
            noise_floor: 1e-3,
            hangover_left: 0,
            hangover_frames: 10,
            threshold_ratio: 3.0,
        }
    }
}

impl EnergyVad {
    fn rms(frame: &[f32]) -> f32 {
        if frame.is_empty() {
            return 0.0;
        }
        (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
    }
}

impl VoiceActivityDetector for EnergyVad {
    fn classify(&mut self, frame: &[f32]) -> FrameVerdict {
        let rms = Self::rms(frame);
        let is_speech = rms > self.noise_floor * self.threshold_ratio && rms > 0.005;

        if is_speech {
            self.hangover_left = self.hangover_frames;
            // Adapt the floor slowly during speech (capped at 2× the current
            // floor) — fast enough to absorb a new constant noise source in a
            // few seconds, slow enough that a multi-minute monologue cannot
            // raise it to speech level before a natural pause resets it.
            self.noise_floor = 0.995 * self.noise_floor + 0.005 * rms.min(self.noise_floor * 2.0);
            FrameVerdict::Speech
        } else {
            // Fast adaptation during silence tracks changing room tone.
            self.noise_floor = 0.95 * self.noise_floor + 0.05 * rms.max(1e-5);
            if self.hangover_left > 0 {
                self.hangover_left -= 1;
                FrameVerdict::Speech
            } else {
                FrameVerdict::Silence
            }
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silence_frame() -> Vec<f32> {
        vec![0.0001; FRAME_SAMPLES]
    }

    fn speech_frame() -> Vec<f32> {
        // 200 Hz tone at healthy speech amplitude.
        (0..FRAME_SAMPLES)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 200.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    #[test]
    fn detects_speech_after_silence() {
        let mut vad = EnergyVad::default();
        for _ in 0..20 {
            assert_eq!(vad.classify(&silence_frame()), FrameVerdict::Silence);
        }
        assert_eq!(vad.classify(&speech_frame()), FrameVerdict::Speech);
    }

    #[test]
    fn hangover_bridges_short_pauses() {
        let mut vad = EnergyVad::default();
        for _ in 0..20 {
            vad.classify(&silence_frame());
        }
        vad.classify(&speech_frame());
        // 5 frames (150 ms) of pause should still read as speech…
        for _ in 0..5 {
            assert_eq!(vad.classify(&silence_frame()), FrameVerdict::Speech);
        }
        // …but a long pause eventually flips to silence.
        let mut flipped = false;
        for _ in 0..30 {
            if vad.classify(&silence_frame()) == FrameVerdict::Silence {
                flipped = true;
                break;
            }
        }
        assert!(flipped, "VAD never returned to silence after speech ended");
    }

    #[test]
    fn adapts_to_noisy_environment() {
        let mut vad = EnergyVad::default();
        // Constant fan noise at moderate level: after adaptation it must be
        // classified as silence, not speech.
        let noise: Vec<f32> = (0..FRAME_SAMPLES).map(|i| 0.01 * ((i * 7919 % 65_536) as f32 / 32_768.0 - 1.0)).collect();
        // ~9 s of constant noise — well above the adaptation time constant.
        for _ in 0..300 {
            vad.classify(&noise);
        }
        assert_eq!(vad.classify(&noise), FrameVerdict::Silence);
        // Loud speech over that noise is still detected.
        assert_eq!(vad.classify(&speech_frame()), FrameVerdict::Speech);
    }
}
