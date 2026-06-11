//! Shared application state managed by Tauri.

use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::recording::RecordingSession;
use crate::worker::TranscriberWorker;

pub struct AppState {
    pub pool: SqlitePool,
    pub worker: TranscriberWorker,
    /// At most one live recording at a time.
    pub recording: Mutex<Option<RecordingSession>>,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            worker: TranscriberWorker::spawn(),
            recording: Mutex::new(None),
        }
    }
}
