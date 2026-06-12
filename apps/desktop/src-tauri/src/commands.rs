//! Tauri command layer — the IPC surface consumed by the React frontend.
//!
//! Commands stay thin: argument validation and orchestration only; logic
//! lives in `transcribe-core`, `db`, `recording` and `export`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use transcribe_core::audio::capture::list_input_devices;
use transcribe_core::llm::summarize::{builtin_templates, summarize, SummaryTemplate};
use transcribe_core::models::ModelManager;
use transcribe_core::transcribe::TranscribeOptions;

use crate::db::{self, Meeting, MeetingListItem, Segment, Summary};
use crate::error::{AppError, Result};
use crate::export::meeting_to_markdown;
use crate::recording;
use crate::settings::{self, AppSettings};
use crate::state::AppState;

// ---- meetings ----

#[tauri::command]
pub async fn list_meetings(
    state: State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<MeetingListItem>> {
    db::list_meetings(&state.pool, query.as_deref()).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub segments: Vec<Segment>,
    pub summaries: Vec<Summary>,
}

#[tauri::command]
pub async fn get_meeting(state: State<'_, AppState>, id: String) -> Result<MeetingDetail> {
    Ok(MeetingDetail {
        meeting: db::get_meeting(&state.pool, &id).await?,
        segments: db::list_segments(&state.pool, &id).await?,
        summaries: db::list_summaries(&state.pool, &id).await?,
    })
}

#[tauri::command]
pub async fn rename_meeting(state: State<'_, AppState>, id: String, title: String) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::InvalidInput("title must not be empty".into()));
    }
    db::rename_meeting(&state.pool, &id, title).await
}

#[tauri::command]
pub async fn delete_meeting(state: State<'_, AppState>, id: String) -> Result<()> {
    db::delete_meeting(&state.pool, &id).await
}

// ---- recording ----

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<Meeting> {
    let mut recording_slot = state.recording.lock().await;
    if recording_slot.is_some() {
        return Err(AppError::AlreadyRecording);
    }

    let settings = settings::load(&state.pool).await?;
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("Meeting {}", chrono::Local::now().format("%Y-%m-%d %H:%M")));
    let meeting_id = uuid::Uuid::new_v4().to_string();
    let meeting = db::create_meeting(
        &state.pool,
        &meeting_id,
        &title,
        "live",
        settings.spoken_language.as_deref(),
    )
    .await?;

    let session = recording::start(
        app,
        state.pool.clone(),
        state.worker.clone(),
        settings,
        meeting_id.clone(),
    )
    .await;
    match session {
        Ok(session) => {
            *recording_slot = Some(session);
            Ok(meeting)
        }
        Err(e) => {
            // Don't keep an empty meeting around if capture failed to start.
            db::delete_meeting(&state.pool, &meeting_id).await.ok();
            Err(e)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    pub meeting_id: String,
    pub duration_ms: u64,
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, AppState>) -> Result<StopResult> {
    let session = state
        .recording
        .lock()
        .await
        .take()
        .ok_or(AppError::NotRecording)?;
    let (meeting_id, duration) = session.stop().await;
    db::set_meeting_duration(&state.pool, &meeting_id, duration.as_millis() as i64).await?;
    Ok(StopResult {
        meeting_id,
        duration_ms: duration.as_millis() as u64,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub meeting_id: Option<String>,
    pub elapsed_ms: u64,
}

#[tauri::command]
pub async fn recording_status(state: State<'_, AppState>) -> Result<RecordingStatus> {
    let slot = state.recording.lock().await;
    Ok(match slot.as_ref() {
        Some(s) => RecordingStatus {
            meeting_id: Some(s.meeting_id.clone()),
            elapsed_ms: s.elapsed().as_millis() as u64,
        },
        None => RecordingStatus {
            meeting_id: None,
            elapsed_ms: 0,
        },
    })
}

// ---- import ----

#[tauri::command]
pub async fn import_media(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    title: Option<String>,
) -> Result<Meeting> {
    let settings = settings::load(&state.pool).await?;
    let models = ModelManager::with_default_dir()?;
    let model_path = models.resolve(&settings.whisper_model)?;

    let source_path = PathBuf::from(&path);
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| {
            source_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "Imported meeting".into());

    let decode_path = source_path.clone();
    let samples = tauri::async_runtime::spawn_blocking(move || {
        transcribe_core::audio::import::decode_to_pipeline_format(&decode_path)
    })
    .await
    .map_err(|e| AppError::Internal(format!("decode task: {e}")))??;
    let duration_ms =
        (samples.len() as u64 * 1000) / transcribe_core::audio::PIPELINE_SAMPLE_RATE as u64;

    let meeting_id = uuid::Uuid::new_v4().to_string();
    let meeting = db::create_meeting(
        &state.pool,
        &meeting_id,
        &title,
        "import",
        settings.spoken_language.as_deref(),
    )
    .await?;
    db::set_meeting_duration(&state.pool, &meeting_id, duration_ms as i64).await?;

    let diarization = if settings.diarization {
        models
            .resolve_diarization()
            .map(
                |(segmentation_model, embedding_model)| crate::worker::DiarizationRequest {
                    segmentation_model,
                    embedding_model,
                    session_key: meeting_id.clone(),
                },
            )
            .ok()
    } else {
        None
    };

    // One whisper pass over the whole file: best quality and native
    // timestamps (whisper windows long audio internally).
    let segments = state
        .worker
        .transcribe(
            samples,
            Duration::ZERO,
            settings.whisper_model.clone(),
            model_path,
            TranscribeOptions {
                language: settings.spoken_language.clone(),
                translate: false,
                threads: 0,
            },
            diarization,
        )
        .await;
    let segments = match segments {
        Ok(s) => s,
        Err(e) => {
            db::delete_meeting(&state.pool, &meeting_id).await.ok();
            return Err(e);
        }
    };
    for seg in &segments {
        db::insert_segment(
            &state.pool,
            &meeting_id,
            seg.start_ms as i64,
            seg.end_ms as i64,
            &seg.text,
            seg.speaker.map(|s| s as i64),
        )
        .await?;
    }
    let _ = app.emit("import-finished", &meeting_id);
    db::get_meeting(&state.pool, &meeting_id).await?;
    Ok(meeting)
}

// ---- models ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub quality_hint: String,
    pub downloaded: bool,
}

#[tauri::command]
pub async fn list_models() -> Result<Vec<ModelEntry>> {
    let manager = ModelManager::with_default_dir()?;
    Ok(manager
        .list()
        .into_iter()
        .map(|(info, downloaded)| ModelEntry {
            id: info.id.into(),
            display_name: info.display_name.into(),
            size_bytes: info.size_bytes,
            quality_hint: info.quality_hint.into(),
            downloaded,
        })
        .collect())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    model_id: String,
    downloaded: u64,
    total: u64,
}

#[tauri::command]
pub async fn download_model(app: AppHandle, id: String) -> Result<()> {
    let manager = ModelManager::with_default_dir()?;
    let event_id = id.clone();
    manager
        .download(
            &id,
            Some(std::sync::Arc::new(move |downloaded, total| {
                let _ = app.emit(
                    "model-download-progress",
                    DownloadProgress {
                        model_id: event_id.clone(),
                        downloaded,
                        total,
                    },
                );
            })),
        )
        .await?;
    Ok(())
}

/// Whether the diarization models are present locally.
#[tauri::command]
pub async fn diarization_status() -> Result<bool> {
    Ok(ModelManager::with_default_dir()?.diarization_downloaded())
}

/// Downloads the diarization models (segmentation + voice embeddings),
/// emitting combined progress under the pseudo-model id `diarization`.
#[tauri::command]
pub async fn enable_diarization(app: AppHandle) -> Result<()> {
    let manager = ModelManager::with_default_dir()?;
    manager
        .ensure_diarization(Some(std::sync::Arc::new(move |downloaded, total| {
            let _ = app.emit(
                "model-download-progress",
                DownloadProgress {
                    model_id: "diarization".into(),
                    downloaded,
                    total,
                },
            );
        })))
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn delete_model(id: String) -> Result<()> {
    let manager = ModelManager::with_default_dir()?;
    manager.delete(&id).await?;
    Ok(())
}

// ---- audio devices ----

#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<String>> {
    tauri::async_runtime::spawn_blocking(|| list_input_devices().map_err(AppError::from))
        .await
        .map_err(|e| AppError::Internal(format!("device enumeration: {e}")))?
}

// ---- settings & templates ----

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings> {
    settings::load(&state.pool).await
}

#[tauri::command]
pub async fn set_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<()> {
    settings::save(&state.pool, &settings).await
}

#[tauri::command]
pub async fn list_templates() -> Result<Vec<SummaryTemplate>> {
    Ok(builtin_templates())
}

#[tauri::command]
pub async fn list_ollama_models(base_url: String) -> Result<Vec<String>> {
    Ok(transcribe_core::llm::OllamaProvider::list_models(&base_url).await?)
}

// ---- summaries ----

#[tauri::command]
pub async fn generate_summary(
    state: State<'_, AppState>,
    meeting_id: String,
    template_id: Option<String>,
    language: Option<String>,
) -> Result<Summary> {
    let settings = settings::load(&state.pool).await?;
    let provider_config = settings.llm.ok_or_else(|| {
        AppError::InvalidInput(
            "No AI provider configured. Set up Ollama or an API key in Settings.".into(),
        )
    })?;

    let template_id = template_id.unwrap_or(settings.summary_template);
    let template = builtin_templates()
        .into_iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown template `{template_id}`")))?;
    let language = language.unwrap_or(settings.summary_language);

    let segments = db::list_segments(&state.pool, &meeting_id).await?;
    if segments.is_empty() {
        return Err(AppError::InvalidInput(
            "This meeting has no transcript to summarize.".into(),
        ));
    }
    let core_segments: Vec<transcribe_core::transcribe::TranscriptSegment> = segments
        .iter()
        .map(|s| transcribe_core::transcribe::TranscriptSegment {
            text: s.text.clone(),
            start_ms: s.start_ms as u64,
            end_ms: s.end_ms as u64,
            speaker: s.speaker.map(|sp| sp as u32),
        })
        .collect();

    let provider = provider_config.build();
    let content = summarize(provider.as_ref(), &core_segments, &template, &language).await?;

    let (provider_name, model_name) = match &provider_config {
        transcribe_core::llm::ProviderConfig::Ollama { model, .. } => ("ollama", model.clone()),
        transcribe_core::llm::ProviderConfig::OpenAiCompat { model, .. } => {
            ("openai-compatible", model.clone())
        }
    };
    db::insert_summary(
        &state.pool,
        &meeting_id,
        &template.id,
        &language,
        &content,
        provider_name,
        &model_name,
    )
    .await
}

// ---- export ----

#[tauri::command]
pub async fn export_markdown(state: State<'_, AppState>, meeting_id: String) -> Result<String> {
    let meeting = db::get_meeting(&state.pool, &meeting_id).await?;
    let segments = db::list_segments(&state.pool, &meeting_id).await?;
    let summaries = db::list_summaries(&state.pool, &meeting_id).await?;
    Ok(meeting_to_markdown(&meeting, &segments, summaries.first()))
}

#[tauri::command]
pub async fn save_markdown(
    state: State<'_, AppState>,
    meeting_id: String,
    path: String,
) -> Result<()> {
    let markdown = export_markdown(state, meeting_id).await?;
    tokio::fs::write(&path, markdown).await?;
    Ok(())
}
