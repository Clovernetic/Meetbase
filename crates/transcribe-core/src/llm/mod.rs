//! LLM adapters for meeting summarization.
//!
//! Two backends cover the whole BYOK matrix:
//! - [`OllamaProvider`] talks to a local Ollama server (fully offline).
//! - [`OpenAiCompatProvider`] talks to any OpenAI-compatible chat endpoint
//!   (OpenAI, Groq, OpenRouter, Anthropic's compat layer, LM Studio, vLLM…).
//!
//! Privacy invariant: this module only ever sends *text* (the transcript),
//! never audio, and only when the user has configured a provider.

mod ollama;
mod openai_compat;
pub mod summarize;

pub use ollama::OllamaProvider;
pub use openai_compat::OpenAiCompatProvider;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }
}

/// A chat-completion backend.
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Returns the assistant's reply for the given conversation.
    async fn complete(&self, messages: &[ChatMessage]) -> Result<String>;

    /// Human-readable provider identifier for logs and error messages.
    fn name(&self) -> &str;
}

/// Provider configuration as stored in settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderConfig {
    Ollama {
        /// e.g. `http://localhost:11434`
        base_url: String,
        model: String,
    },
    OpenAiCompat {
        /// e.g. `https://api.openai.com/v1`
        base_url: String,
        api_key: String,
        model: String,
    },
}

impl ProviderConfig {
    /// Instantiates the concrete provider for this configuration.
    pub fn build(&self) -> Box<dyn ChatProvider> {
        match self {
            ProviderConfig::Ollama { base_url, model } => {
                Box::new(OllamaProvider::new(base_url.clone(), model.clone()))
            }
            ProviderConfig::OpenAiCompat { base_url, api_key, model } => Box::new(
                OpenAiCompatProvider::new(base_url.clone(), api_key.clone(), model.clone()),
            ),
        }
    }
}
