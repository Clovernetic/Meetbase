//! Mixing of the microphone and system-audio streams into one mono stream.
//!
//! Both inputs are already 16 kHz mono. The two callbacks arrive on different
//! threads at different cadences, so each side writes into its own lock-free
//! ring buffer and the mixer thread drains both at a fixed pace, summing with
//! soft clipping. If one side stalls (e.g. no system audio during a mic-only
//! meeting) the other keeps flowing — missing samples are treated as silence.

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

/// Capacity per side: 10 s at 16 kHz. Overruns drop the oldest audio, which
/// for live captioning is preferable to unbounded latency growth.
const RING_CAPACITY: usize = 160_000;

/// Soft clip via tanh to avoid harsh distortion when both sides peak at once.
fn soft_clip(s: f32) -> f32 {
    s.tanh()
}

pub struct MixerInput {
    producer: ringbuf::HeapProd<f32>,
}

impl MixerInput {
    /// Pushes samples from a capture callback. Drops the newest samples on
    /// overrun (the consumer side is expected to keep up in practice).
    pub fn push(&mut self, samples: &[f32]) {
        self.producer.push_slice(samples);
    }
}

pub struct StreamMixer {
    mic: ringbuf::HeapCons<f32>,
    system: ringbuf::HeapCons<f32>,
    mic_gain: f32,
    system_gain: f32,
}

impl StreamMixer {
    /// Returns the mixer plus the two input handles to hand to capture
    /// callbacks.
    pub fn new(mic_gain: f32, system_gain: f32) -> (Self, MixerInput, MixerInput) {
        let (mic_prod, mic_cons) = HeapRb::<f32>::new(RING_CAPACITY).split();
        let (sys_prod, sys_cons) = HeapRb::<f32>::new(RING_CAPACITY).split();
        (
            Self {
                mic: mic_cons,
                system: sys_cons,
                mic_gain,
                system_gain,
            },
            MixerInput { producer: mic_prod },
            MixerInput { producer: sys_prod },
        )
    }

    /// Drains up to `max` mixed samples. Sides are summed where both have
    /// data; where only one has data its samples pass through alone, so a
    /// stalled side never blocks the stream.
    pub fn drain(&mut self, max: usize) -> Vec<f32> {
        let mut mic_buf = vec![0.0f32; max];
        let mut sys_buf = vec![0.0f32; max];
        let mic_n = self.mic.pop_slice(&mut mic_buf);
        let sys_n = self.system.pop_slice(&mut sys_buf);
        let n = mic_n.max(sys_n);
        (0..n)
            .map(|i| soft_clip(mic_buf[i] * self.mic_gain + sys_buf[i] * self.system_gain))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_both_sides() {
        let (mut mixer, mut mic, mut sys) = StreamMixer::new(1.0, 1.0);
        mic.push(&[0.1, 0.1, 0.1]);
        sys.push(&[0.2, 0.2, 0.2]);
        let out = mixer.drain(16);
        assert_eq!(out.len(), 3);
        for s in out {
            assert!((s - soft_clip(0.3)).abs() < 1e-6);
        }
    }

    #[test]
    fn one_side_stalled_passes_other_through() {
        let (mut mixer, mut mic, _sys) = StreamMixer::new(1.0, 1.0);
        mic.push(&[0.5; 100]);
        let out = mixer.drain(200);
        assert_eq!(out.len(), 100);
        assert!((out[0] - soft_clip(0.5)).abs() < 1e-6);
    }

    #[test]
    fn clipping_is_bounded() {
        let (mut mixer, mut mic, mut sys) = StreamMixer::new(1.0, 1.0);
        mic.push(&[1.0; 10]);
        sys.push(&[1.0; 10]);
        for s in mixer.drain(10) {
            assert!(s.abs() <= 1.0);
        }
    }

    #[test]
    fn gains_are_applied() {
        let (mut mixer, mut mic, _sys) = StreamMixer::new(0.5, 1.0);
        mic.push(&[0.4; 4]);
        let out = mixer.drain(4);
        assert!((out[0] - soft_clip(0.2)).abs() < 1e-6);
    }
}
