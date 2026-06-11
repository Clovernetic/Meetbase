use serde::Serialize;
use thiserror::Error;

/// Application-level errors; serialized to the frontend as `{ message }`.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0} not found")]
    NotFound(String),

    #[error("a recording is already in progress")]
    AlreadyRecording,

    #[error("no recording in progress")]
    NotRecording,

    #[error("{0}")]
    InvalidInput(String),

    #[error(transparent)]
    Core(#[from] transcribe_core::CoreError),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("database migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 1)?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}
