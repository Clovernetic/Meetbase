//! Long-lived transcription worker thread.
//!
//! whisper.cpp model state is `Send` but not `Sync`, and loading a model
//! takes seconds — so one dedicated OS thread owns the loaded model and
//! serves transcription jobs from a channel. The model is lazily (re)loaded
//! when a job asks for a different model id.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use tracing::{error, info};
use transcribe_core::transcribe::{TranscribeOptions, Transcriber, TranscriptSegment};
use transcribe_core::CoreError;

use crate::error::{AppError, Result};

pub struct TranscribeJob {
    pub samples: Vec<f32>,
    /// Offset of `samples[0]` within the recording (added to timestamps).
    pub offset: Duration,
    pub model_id: String,
    pub model_path: PathBuf,
    pub options: TranscribeOptions,
    pub reply: tokio::sync::oneshot::Sender<Result<Vec<TranscriptSegment>>>,
}

#[derive(Clone)]
pub struct TranscriberWorker {
    tx: mpsc::Sender<TranscribeJob>,
}

impl TranscriberWorker {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<TranscribeJob>();
        std::thread::Builder::new()
            .name("transcriber".into())
            .spawn(move || worker_loop(rx))
            .expect("failed to spawn transcriber thread");
        Self { tx }
    }

    /// Queues a job and awaits its result without blocking the async runtime.
    pub async fn transcribe(
        &self,
        samples: Vec<f32>,
        offset: Duration,
        model_id: String,
        model_path: PathBuf,
        options: TranscribeOptions,
    ) -> Result<Vec<TranscriptSegment>> {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(TranscribeJob {
                samples,
                offset,
                model_id,
                model_path,
                options,
                reply,
            })
            .map_err(|_| AppError::Internal("transcriber thread is gone".into()))?;
        rx.await
            .map_err(|_| AppError::Internal("transcriber dropped the job".into()))?
    }
}

fn worker_loop(rx: mpsc::Receiver<TranscribeJob>) {
    let mut loaded: Option<(String, Transcriber)> = None;
    while let Ok(job) = rx.recv() {
        // (Re)load the model if the job needs a different one.
        if loaded.as_ref().map(|(id, _)| id.as_str()) != Some(job.model_id.as_str()) {
            loaded = None; // free the old model before loading the new one
            match Transcriber::load(&job.model_path, &job.model_id) {
                Ok(t) => {
                    info!(model = %job.model_id, "transcriber model loaded");
                    loaded = Some((job.model_id.clone(), t));
                }
                Err(e) => {
                    error!(model = %job.model_id, "model load failed: {e}");
                    let _ = job.reply.send(Err(e.into()));
                    continue;
                }
            }
        }
        let (_, transcriber) = loaded.as_mut().expect("model just loaded");
        let result = transcriber
            .transcribe(&job.samples, job.offset, &job.options)
            .map_err(|e: CoreError| e.into());
        // Receiver may have been dropped (e.g. recording cancelled) — fine.
        let _ = job.reply.send(result);
    }
}
