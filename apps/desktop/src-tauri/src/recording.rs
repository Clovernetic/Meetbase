//! Live recording orchestration.
//!
//! Three moving parts per recording:
//!
//! 1. **Capture thread** (owns the `!Send` cpal streams): microphone — and
//!    system audio where supported — feed a [`StreamMixer`]; the thread
//!    drains the mixer every 100 ms, runs the speech chunker and forwards
//!    finished chunks. It exits when the stop flag flips.
//! 2. **Transcription task** (async): receives chunks, sends them to the
//!    shared [`TranscriberWorker`], persists resulting segments and emits
//!    `transcript-segment` events to the UI.
//! 3. The [`RecordingSession`] handle held in app state, used to stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};
use transcribe_core::audio::capture::{start_microphone, start_system_audio};
use transcribe_core::audio::chunker::{ChunkerConfig, SpeechChunk, SpeechChunker};
use transcribe_core::audio::mixer::StreamMixer;
use transcribe_core::audio::vad::EnergyVad;
use transcribe_core::audio::PIPELINE_SAMPLE_RATE;
use transcribe_core::transcribe::TranscribeOptions;

use crate::db;
use crate::error::{AppError, Result};
use crate::settings::AppSettings;
use crate::worker::TranscriberWorker;

pub struct RecordingSession {
    pub meeting_id: String,
    stop: Arc<AtomicBool>,
    started: Instant,
    /// Completes when the transcription task has drained all chunks.
    finished: tokio::sync::oneshot::Receiver<()>,
}

/// Payload of the `transcript-segment` event.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SegmentEvent {
    meeting_id: String,
    segment: db::Segment,
}

pub async fn start(
    app: AppHandle,
    pool: SqlitePool,
    worker: TranscriberWorker,
    settings: AppSettings,
    meeting_id: String,
) -> Result<RecordingSession> {
    // Resolve the model up front so failure surfaces before audio starts.
    let models = transcribe_core::models::ModelManager::with_default_dir()?;
    let model_path = models.resolve(&settings.whisper_model)?;

    let stop = Arc::new(AtomicBool::new(false));
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<SpeechChunk>();

    // --- capture thread ---
    let capture_stop = stop.clone();
    let mic_device = settings.mic_device.clone();
    let want_system = settings.capture_system_audio;
    let (capture_err_tx, capture_err_rx) = std::sync::mpsc::sync_channel::<Result<()>>(1);
    std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            let (mut mixer, mut mic_in, mut sys_in) = StreamMixer::new(1.0, 1.0);

            let mic = match start_microphone(mic_device.as_deref(), move |s| mic_in.push(s)) {
                Ok(h) => h,
                Err(e) => {
                    let _ = capture_err_tx.send(Err(e.into()));
                    return;
                }
            };
            info!(device = %mic.device_name, "microphone capturing");

            let _system = if want_system {
                match start_system_audio(move |s| sys_in.push(s)) {
                    Ok(h) => {
                        info!(device = %h.device_name, "system audio capturing");
                        Some(h)
                    }
                    Err(e) => {
                        // Expected on platforms without loopback — mic-only.
                        warn!("system audio unavailable: {e}");
                        None
                    }
                }
            } else {
                None
            };
            let _ = capture_err_tx.send(Ok(()));

            let mut chunker =
                SpeechChunker::new(Box::new(EnergyVad::default()), ChunkerConfig::default());
            let drain_size = PIPELINE_SAMPLE_RATE as usize / 2;
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let samples = mixer.drain(drain_size);
                for chunk in chunker.push(&samples) {
                    let _ = chunk_tx.send(chunk);
                }
                if capture_stop.load(Ordering::Relaxed) {
                    // Final drain + flush, then drop the senders so the
                    // transcription task knows the stream ended.
                    let tail = mixer.drain(drain_size * 4);
                    for chunk in chunker.push(&tail) {
                        let _ = chunk_tx.send(chunk);
                    }
                    if let Some(chunk) = chunker.finish() {
                        let _ = chunk_tx.send(chunk);
                    }
                    break;
                }
            }
            // cpal streams stop when the handles drop here.
        })
        .map_err(|e| AppError::Internal(format!("spawn capture thread: {e}")))?;

    // Fail fast if the microphone could not be opened.
    capture_err_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| AppError::Internal("audio capture did not start in time".into()))??;

    // --- transcription task ---
    let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
    let options = TranscribeOptions {
        language: settings.spoken_language.clone(),
        translate: false,
        threads: 0,
    };
    let model_id = settings.whisper_model.clone();
    let task_meeting = meeting_id.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(chunk) = chunk_rx.recv().await {
            let result = worker
                .transcribe(
                    chunk.samples,
                    chunk.start,
                    model_id.clone(),
                    model_path.clone(),
                    options.clone(),
                )
                .await;
            let segments = match result {
                Ok(s) => s,
                Err(e) => {
                    warn!("chunk transcription failed: {e}");
                    continue;
                }
            };
            for seg in segments {
                match db::insert_segment(
                    &pool,
                    &task_meeting,
                    seg.start_ms as i64,
                    seg.end_ms as i64,
                    &seg.text,
                )
                .await
                {
                    Ok(stored) => {
                        let _ = app.emit(
                            "transcript-segment",
                            SegmentEvent {
                                meeting_id: task_meeting.clone(),
                                segment: stored,
                            },
                        );
                    }
                    Err(e) => warn!("failed to persist segment: {e}"),
                }
            }
        }
        let _ = finished_tx.send(());
    });

    Ok(RecordingSession {
        meeting_id,
        stop,
        started: Instant::now(),
        finished: finished_rx,
    })
}

impl RecordingSession {
    /// Signals stop and waits until all pending chunks are transcribed.
    pub async fn stop(self) -> (String, Duration) {
        self.stop.store(true, Ordering::Relaxed);
        // Transcribing the tail can lag behind real time; cap the wait so the
        // UI never hangs on stop.
        let _ = tokio::time::timeout(Duration::from_secs(120), self.finished).await;
        (self.meeting_id, self.started.elapsed())
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}
