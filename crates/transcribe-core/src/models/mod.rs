//! Whisper model registry and download manager.
//!
//! Models are GGML files served from the huggingface `ggerganov/whisper.cpp`
//! repository. They are stored under the platform data directory (e.g.
//! `~/Library/Application Support/meetbase/models` on macOS) and verified by
//! SHA-256 after download.

mod registry;

pub use registry::{ModelInfo, MODEL_REGISTRY};

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::error::{CoreError, Result};

/// Download progress callback: (bytes_downloaded, total_bytes).
pub type ProgressFn = Box<dyn Fn(u64, u64) + Send + Sync>;

pub struct ModelManager {
    models_dir: PathBuf,
}

impl ModelManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    /// Default location under the platform data dir.
    pub fn with_default_dir() -> Result<Self> {
        let base = dirs::data_dir()
            .ok_or_else(|| CoreError::ModelDownload("no platform data directory".into()))?;
        Ok(Self::new(base.join("meetbase").join("models")))
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    pub fn info(&self, id: &str) -> Result<&'static ModelInfo> {
        MODEL_REGISTRY
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| CoreError::UnknownModel(id.to_string()))
    }

    /// Local path a model would occupy (whether or not it is downloaded).
    pub fn path_for(&self, id: &str) -> Result<PathBuf> {
        let info = self.info(id)?;
        Ok(self.models_dir.join(info.file_name))
    }

    pub fn is_downloaded(&self, id: &str) -> Result<bool> {
        Ok(self.path_for(id)?.exists())
    }

    /// All registry entries with their local download state.
    pub fn list(&self) -> Vec<(&'static ModelInfo, bool)> {
        MODEL_REGISTRY
            .iter()
            .map(|m| (m, self.models_dir.join(m.file_name).exists()))
            .collect()
    }

    /// Resolves a model for transcription, erroring if not downloaded.
    pub fn resolve(&self, id: &str) -> Result<PathBuf> {
        let path = self.path_for(id)?;
        if !path.exists() {
            return Err(CoreError::ModelNotDownloaded(id.to_string()));
        }
        Ok(path)
    }

    /// Downloads a model with progress reporting and SHA-256 verification.
    ///
    /// Downloads to a `.partial` file first so an interrupted download never
    /// masquerades as a complete model.
    pub async fn download(&self, id: &str, progress: Option<ProgressFn>) -> Result<PathBuf> {
        let info = self.info(id)?;
        self.download_with_info(info, progress).await
    }

    /// Same as [`Self::download`] but takes the registry entry directly
    /// (also used by tests to point at a mock server).
    pub async fn download_with_info(
        &self,
        info: &ModelInfo,
        progress: Option<ProgressFn>,
    ) -> Result<PathBuf> {
        let final_path = self.models_dir.join(info.file_name);
        if final_path.exists() {
            return Ok(final_path);
        }
        tokio::fs::create_dir_all(&self.models_dir).await?;
        let partial_path = final_path.with_extension("partial");

        info!(model = info.id, url = info.url, "downloading model");
        let response = reqwest::get(info.url)
            .await
            .map_err(|e| CoreError::ModelDownload(e.to_string()))?
            .error_for_status()
            .map_err(|e| CoreError::ModelDownload(e.to_string()))?;
        let total = response.content_length().unwrap_or(info.size_bytes);

        let mut file = tokio::fs::File::create(&partial_path).await?;
        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| CoreError::ModelDownload(e.to_string()))?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if let Some(cb) = &progress {
                cb(downloaded, total);
            }
        }
        file.flush().await?;
        drop(file);

        let actual = hex::encode(hasher.finalize());
        if !info.sha256.is_empty() && actual != info.sha256 {
            tokio::fs::remove_file(&partial_path).await.ok();
            return Err(CoreError::ChecksumMismatch {
                file: info.file_name.to_string(),
                expected: info.sha256.to_string(),
                actual,
            });
        }

        tokio::fs::rename(&partial_path, &final_path).await?;
        info!(model = info.id, path = %final_path.display(), "model ready");
        Ok(final_path)
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let path = self.path_for(id)?;
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn manager(dir: &Path) -> ModelManager {
        ModelManager::new(dir.to_path_buf())
    }

    #[test]
    fn registry_has_expected_models_and_unique_ids() {
        let ids: Vec<&str> = MODEL_REGISTRY.iter().map(|m| m.id).collect();
        for required in ["tiny", "base", "small", "medium", "large-v3-turbo"] {
            assert!(ids.contains(&required), "missing model `{required}`");
        }
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(deduped.len(), ids.len(), "duplicate model ids in registry");
    }

    #[test]
    fn unknown_model_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        assert!(matches!(m.info("gpt-5"), Err(CoreError::UnknownModel(_))));
    }

    #[test]
    fn resolve_requires_download() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        assert!(matches!(
            m.resolve("tiny"),
            Err(CoreError::ModelNotDownloaded(_))
        ));
    }

    #[tokio::test]
    async fn download_writes_file_and_reports_progress() {
        let server = MockServer::start().await;
        let body = vec![42u8; 1024];
        Mock::given(method("GET"))
            .and(path("/model.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        // Use a test-only registry entry by downloading via URL override:
        // simplest is to test the streaming path through a real entry with
        // its URL swapped — covered instead by downloading to a temp registry.
        let info = ModelInfo {
            id: "test",
            display_name: "Test",
            file_name: "model.bin",
            url: Box::leak(format!("{}/model.bin", server.uri()).into_boxed_str()),
            size_bytes: 1024,
            sha256: "",
            quality_hint: "",
        };
        let progress_calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let pc = progress_calls.clone();
        let final_path = m
            .download_with_info(&info, Some(Box::new(move |done, total| {
                assert!(done <= total);
                pc.store(done, std::sync::atomic::Ordering::SeqCst);
            })))
            .await
            .unwrap();
        assert!(final_path.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), body);
        assert_eq!(
            progress_calls.load(std::sync::atomic::Ordering::SeqCst),
            1024
        );
        assert!(!final_path.with_extension("partial").exists());
    }

    #[tokio::test]
    async fn checksum_mismatch_removes_partial() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/model.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"corrupted".to_vec()))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let m = manager(dir.path());
        let info = ModelInfo {
            id: "test",
            display_name: "Test",
            file_name: "model.bin",
            url: Box::leak(format!("{}/model.bin", server.uri()).into_boxed_str()),
            size_bytes: 9,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            quality_hint: "",
        };
        let err = m.download_with_info(&info, None).await.unwrap_err();
        assert!(matches!(err, CoreError::ChecksumMismatch { .. }));
        assert!(!dir.path().join("model.bin").exists());
        assert!(!dir.path().join("model.partial").exists());
    }
}
