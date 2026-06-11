//! Audio acquisition and pre-processing.
//!
//! All downstream consumers (VAD, chunker, Whisper) operate on **16 kHz mono
//! f32** samples — the format Whisper expects. The capture and import layers
//! are responsible for converting whatever the OS or media file provides into
//! that canonical format as early as possible.

pub mod capture;
pub mod chunker;
pub mod import;
pub mod mixer;
pub mod resample;
pub mod vad;

/// The canonical sample rate of the whole pipeline (what Whisper consumes).
pub const PIPELINE_SAMPLE_RATE: u32 = 16_000;
