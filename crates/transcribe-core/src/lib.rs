//! transcribe-core: the local-first meeting transcription engine behind Meetbase.
//!
//! The crate is organized as a pipeline:
//!
//! ```text
//! audio (capture / import) → resample 16 kHz mono → VAD → chunking
//!     → transcription (whisper.cpp) → segments
//!     → llm (Ollama / OpenAI-compatible BYOK) → summaries
//! ```
//!
//! Everything runs on the user's machine. The only component that may touch
//! the network is the LLM layer when the user explicitly configures a remote
//! provider (BYOK) — and it only ever sends text, never audio.

pub mod audio;
pub mod error;
pub mod llm;
pub mod models;
pub mod transcribe;

pub use error::CoreError;
