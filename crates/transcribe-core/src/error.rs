use thiserror::Error;

/// Errors surfaced by the transcription engine.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("audio device error: {0}")]
    AudioDevice(String),

    #[error("audio decode error: {0}")]
    AudioDecode(String),

    #[error("model `{0}` is not downloaded")]
    ModelNotDownloaded(String),

    #[error("unknown model `{0}`")]
    UnknownModel(String),

    #[error("model download failed: {0}")]
    ModelDownload(String),

    #[error("checksum mismatch for `{file}`: expected {expected}, got {actual}")]
    ChecksumMismatch {
        file: String,
        expected: String,
        actual: String,
    },

    #[error("transcription error: {0}")]
    Transcription(String),

    #[error("LLM provider error: {0}")]
    Llm(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
