//! Speaker diarization: who spoke when.
//!
//! Pipeline (all local, ONNX on CPU):
//! 1. pyannote `segmentation-3.0` splits audio into speaker turns,
//! 2. a WeSpeaker CAM++ model turns each turn into a voice embedding,
//! 3. embeddings are clustered online by cosine similarity — the same
//!    [`Diarizer`] instance keeps its speaker registry across calls, so
//!    chunked live audio gets consistent speaker ids for a whole meeting.
//! 4. [`assign_speakers`] labels Whisper transcript segments by overlap.
//!
//! Works on the pipeline's 16 kHz mono f32 samples (converted to i16
//! internally, which is what the pyannote models expect).

use std::path::{Path, PathBuf};
use std::time::Duration;

use pyannote_rs::{EmbeddingExtractor, EmbeddingManager};
use tracing::{debug, warn};

use crate::error::{CoreError, Result};
use crate::transcribe::TranscriptSegment;

/// One diarized speaker turn, in absolute recording time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 1-based stable speaker id within the diarization session.
    pub speaker: u32,
}

/// Minimum cosine similarity to match an existing speaker; below this a new
/// speaker is registered (up to `max_speakers`).
const SIMILARITY_THRESHOLD: f32 = 0.5;

/// Turns shorter than this carry too little voice for a reliable embedding.
const MIN_TURN: Duration = Duration::from_millis(400);

pub struct Diarizer {
    segmentation_model: PathBuf,
    extractor: EmbeddingExtractor,
    manager: EmbeddingManager,
    max_speakers: usize,
}

impl Diarizer {
    pub fn new(
        segmentation_model: &Path,
        embedding_model: &Path,
        max_speakers: usize,
    ) -> Result<Self> {
        let extractor = EmbeddingExtractor::new(embedding_model)
            .map_err(|e| CoreError::Diarization(format!("load embedding model: {e}")))?;
        Ok(Self {
            segmentation_model: segmentation_model.to_path_buf(),
            extractor,
            manager: EmbeddingManager::new(max_speakers),
            max_speakers,
        })
    }

    /// Forgets all learned speakers (call between meetings).
    pub fn reset(&mut self) {
        self.manager = EmbeddingManager::new(self.max_speakers);
    }

    /// Diarizes 16 kHz mono samples; `offset` is the position of
    /// `samples[0]` in the recording and is added to all returned times.
    pub fn diarize(&mut self, samples: &[f32], offset: Duration) -> Result<Vec<SpeakerSegment>> {
        let pcm: Vec<i16> = samples
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        let offset_ms = offset.as_millis() as u64;

        let turns = pyannote_rs::get_segments(
            &pcm,
            crate::audio::PIPELINE_SAMPLE_RATE,
            &self.segmentation_model,
        )
        .map_err(|e| CoreError::Diarization(format!("segmentation: {e}")))?;

        let mut out = Vec::new();
        for turn in turns {
            let turn = match turn {
                Ok(t) => t,
                Err(e) => {
                    warn!("diarization segment failed: {e}");
                    continue;
                }
            };
            if turn.end - turn.start < MIN_TURN.as_secs_f64() {
                continue;
            }
            let embedding: Vec<f32> = match self.extractor.compute(&turn.samples) {
                Ok(e) => e.collect(),
                Err(e) => {
                    warn!("speaker embedding failed: {e}");
                    continue;
                }
            };
            // Once the registry is full, force-match the closest speaker
            // instead of dropping the turn.
            let speaker = self
                .manager
                .search_speaker(embedding.clone(), SIMILARITY_THRESHOLD)
                .or_else(|| self.manager.get_best_speaker_match(embedding).ok())
                .map(|id| id as u32);
            let Some(speaker) = speaker else { continue };

            out.push(SpeakerSegment {
                start_ms: offset_ms + (turn.start * 1000.0) as u64,
                end_ms: offset_ms + (turn.end * 1000.0) as u64,
                speaker,
            });
        }
        debug!(turns = out.len(), "diarized chunk");
        Ok(out)
    }
}

/// Labels each transcript segment with the speaker whose turns overlap it
/// the most. Segments with no overlapping turn keep `speaker = None`.
pub fn assign_speakers(transcript: &mut [TranscriptSegment], turns: &[SpeakerSegment]) {
    for seg in transcript.iter_mut() {
        let mut overlap_by_speaker: std::collections::HashMap<u32, u64> =
            std::collections::HashMap::new();
        for turn in turns {
            let start = seg.start_ms.max(turn.start_ms);
            let end = seg.end_ms.min(turn.end_ms);
            if end > start {
                *overlap_by_speaker.entry(turn.speaker).or_default() += end - start;
            }
        }
        seg.speaker = overlap_by_speaker
            .into_iter()
            .max_by_key(|&(_, overlap)| overlap)
            .map(|(speaker, _)| speaker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: u64, end_ms: u64) -> TranscriptSegment {
        TranscriptSegment {
            text: "x".into(),
            start_ms,
            end_ms,
            speaker: None,
        }
    }

    fn turn(start_ms: u64, end_ms: u64, speaker: u32) -> SpeakerSegment {
        SpeakerSegment {
            start_ms,
            end_ms,
            speaker,
        }
    }

    #[test]
    fn assigns_speaker_with_largest_overlap() {
        let mut transcript = vec![seg(0, 1000), seg(1000, 3000), seg(5000, 6000)];
        let turns = vec![
            turn(0, 1200, 1),    // covers seg 0 fully, seg 1 slightly
            turn(1200, 3000, 2), // covers most of seg 1
        ];
        assign_speakers(&mut transcript, &turns);
        assert_eq!(transcript[0].speaker, Some(1));
        assert_eq!(transcript[1].speaker, Some(2));
        assert_eq!(transcript[2].speaker, None, "no overlapping turn");
    }

    #[test]
    fn boundary_touching_turns_do_not_count_as_overlap() {
        let mut transcript = vec![seg(1000, 2000)];
        assign_speakers(&mut transcript, &[turn(0, 1000, 1)]);
        assert_eq!(transcript[0].speaker, None);
    }

    #[test]
    fn empty_turns_leave_speakers_unset() {
        let mut transcript = vec![seg(0, 1000)];
        assign_speakers(&mut transcript, &[]);
        assert_eq!(transcript[0].speaker, None);
    }

    /// Full model test; run manually after downloading the diarization
    /// models (Settings → Speaker recognition, or `ensure_diarization`).
    #[test]
    #[ignore = "requires downloaded diarization models"]
    fn diarizes_two_synthetic_speakers() {
        let manager = crate::models::ModelManager::with_default_dir().unwrap();
        let Ok((seg_model, emb_model)) = manager.resolve_diarization() else {
            eprintln!("skipping: diarization models not downloaded");
            return;
        };
        let mut diarizer = Diarizer::new(&seg_model, &emb_model, 6).unwrap();

        // Two clearly different synthetic "voices": modulated tones at very
        // different pitches, separated by silence.
        let sr = crate::audio::PIPELINE_SAMPLE_RATE as f32;
        let voice = |f0: f32, seconds: f32| -> Vec<f32> {
            (0..(seconds * sr) as usize)
                .map(|i| {
                    let t = i as f32 / sr;
                    // Vibrato + harmonics make it voice-like enough to segment.
                    let f = f0 * (1.0 + 0.02 * (2.0 * std::f32::consts::PI * 5.0 * t).sin());
                    0.4 * (2.0 * std::f32::consts::PI * f * t).sin()
                        + 0.2 * (2.0 * std::f32::consts::PI * 2.0 * f * t).sin()
                })
                .collect()
        };
        let mut samples = voice(120.0, 3.0);
        samples.extend(vec![0.0f32; sr as usize]); // 1 s pause
        samples.extend(voice(280.0, 3.0));

        let turns = diarizer.diarize(&samples, Duration::ZERO).unwrap();
        println!("turns: {turns:?}");
        assert!(!turns.is_empty(), "no speaker turns detected");
    }
}
