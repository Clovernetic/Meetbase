//! Typed application settings, persisted as one JSON document in SQLite.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use transcribe_core::llm::ProviderConfig;

use crate::db;
use crate::error::Result;

const SETTINGS_KEY: &str = "app_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Whisper model id from the registry (e.g. `small`).
    pub whisper_model: String,
    /// ISO-639-1 code of the spoken language; `None` = auto-detect.
    pub spoken_language: Option<String>,
    /// Output language for summaries, as a plain word ("English", "Polski"…).
    pub summary_language: String,
    /// Default summary template id.
    pub summary_template: String,
    /// LLM provider used for summaries; `None` until configured.
    pub llm: Option<ProviderConfig>,
    /// Preferred microphone; `None` = system default.
    pub mic_device: Option<String>,
    /// Mix system audio (other meeting participants) into the recording
    /// where the platform supports loopback capture.
    pub capture_system_audio: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            whisper_model: "small".into(),
            spoken_language: None,
            summary_language: "English".into(),
            summary_template: "standard".into(),
            llm: None,
            mic_device: None,
            capture_system_audio: true,
        }
    }
}

pub async fn load(pool: &SqlitePool) -> Result<AppSettings> {
    match db::get_setting(pool, SETTINGS_KEY).await? {
        // Unknown/legacy fields are ignored; broken JSON falls back to
        // defaults rather than bricking the app.
        Some(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
        None => Ok(AppSettings::default()),
    }
}

pub async fn save(pool: &SqlitePool, settings: &AppSettings) -> Result<()> {
    let json = serde_json::to_string(settings)
        .map_err(|e| crate::error::AppError::Internal(format!("serialize settings: {e}")))?;
    db::set_setting(pool, SETTINGS_KEY, &json).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn defaults_then_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::open(&dir.path().join("t.db")).await.unwrap();

        let s = load(&pool).await.unwrap();
        assert_eq!(s.whisper_model, "small");
        assert!(s.llm.is_none());

        let mut s2 = s.clone();
        s2.summary_language = "Polski".into();
        s2.llm = Some(ProviderConfig::Ollama {
            base_url: "http://localhost:11434".into(),
            model: "llama3.2".into(),
        });
        save(&pool, &s2).await.unwrap();

        let loaded = load(&pool).await.unwrap();
        assert_eq!(loaded.summary_language, "Polski");
        assert!(matches!(loaded.llm, Some(ProviderConfig::Ollama { .. })));
    }

    #[tokio::test]
    async fn corrupt_settings_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let pool = db::open(&dir.path().join("t.db")).await.unwrap();
        db::set_setting(&pool, SETTINGS_KEY, "{not json").await.unwrap();
        let s = load(&pool).await.unwrap();
        assert_eq!(s.whisper_model, "small");
    }
}
