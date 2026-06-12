//! Long-lived transcription worker thread.
//!
//! whisper.cpp model state is `Send` but not `Sync`, and loading a model
//! takes seconds — so one dedicated OS thread owns the loaded model and
//! serves transcription jobs from a channel. The model is lazily (re)loaded
//! when a job asks for a different model id.
//!
//! The same thread owns the optional [`Diarizer`]: its online speaker
//! clustering is stateful per meeting, so jobs carry a `session_key` and the
//! worker resets the speaker registry whenever the key changes.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use tracing::{error, info, warn};
use transcribe_core::diarize::{assign_speakers, Diarizer};
use transcribe_core::transcribe::{TranscribeOptions, Transcriber, TranscriptSegment};
use transcribe_core::CoreError;

use crate::error::{AppError, Result};

/// Maximum distinct speakers tracked per meeting.
const MAX_SPEAKERS: usize = 8;

#[derive(Clone)]
pub struct DiarizationRequest {
    pub segmentation_model: PathBuf,
    pub embedding_model: PathBuf,
    /// Jobs with the same key share one speaker registry (one meeting);
    /// a new key resets it.
    pub session_key: String,
}

pub struct TranscribeJob {
    pub samples: Vec<f32>,
    /// Offset of `samples[0]` within the recording (added to timestamps).
    pub offset: Duration,
    pub model_id: String,
    pub model_path: PathBuf,
    pub options: TranscribeOptions,
    pub diarization: Option<DiarizationRequest>,
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
        diarization: Option<DiarizationRequest>,
    ) -> Result<Vec<TranscriptSegment>> {
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(TranscribeJob {
                samples,
                offset,
                model_id,
                model_path,
                options,
                diarization,
                reply,
            })
            .map_err(|_| AppError::Internal("transcriber thread is gone".into()))?;
        rx.await
            .map_err(|_| AppError::Internal("transcriber dropped the job".into()))?
    }
}

fn worker_loop(rx: mpsc::Receiver<TranscribeJob>) {
    let mut loaded: Option<(String, Transcriber)> = None;
    let mut diarizer: Option<(String, Diarizer)> = None;

    while let Ok(job) = rx.recv() {
        // (Re)load the whisper model if the job needs a different one.
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
        let mut result = transcriber
            .transcribe(&job.samples, job.offset, &job.options)
            .map_err(|e: CoreError| AppError::from(e));

        // Diarize on top of a successful transcription. Failures degrade to
        // an unlabeled transcript rather than failing the job.
        if let (Ok(segments), Some(req)) = (&mut result, &job.diarization) {
            if !segments.is_empty() {
                match diarizer_for(&mut diarizer, req) {
                    Ok(d) => match d.diarize(&job.samples, job.offset) {
                        Ok(turns) => assign_speakers(segments, &turns),
                        Err(e) => warn!("diarization failed for chunk: {e}"),
                    },
                    Err(e) => warn!("diarizer unavailable: {e}"),
                }
            }
        }

        // Receiver may have been dropped (e.g. recording cancelled) — fine.
        let _ = job.reply.send(result);
    }
}

/// Returns a diarizer bound to the job's session, creating or resetting as
/// needed.
fn diarizer_for<'a>(
    slot: &'a mut Option<(String, Diarizer)>,
    req: &DiarizationRequest,
) -> Result<&'a mut Diarizer> {
    if slot.is_none() {
        let d = Diarizer::new(&req.segmentation_model, &req.embedding_model, MAX_SPEAKERS)
            .map_err(AppError::from)?;
        info!("diarizer initialized");
        *slot = Some((req.session_key.clone(), d));
    } else {
        let (key, d) = slot.as_mut().expect("checked above");
        if *key != req.session_key {
            // Same models, new meeting: keep the loaded ONNX sessions and
            // just forget the learned speakers.
            d.reset();
            *key = req.session_key.clone();
        }
    }
    Ok(&mut slot.as_mut().expect("just ensured").1)
}
