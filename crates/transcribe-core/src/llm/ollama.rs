//! Local Ollama chat backend (`POST /api/chat`).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{CoreError, Result};

use super::{ChatMessage, ChatProvider};

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        }
    }

    /// Lists models available on the local Ollama server.
    pub async fn list_models(base_url: &str) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<TagModel>,
        }
        #[derive(Deserialize)]
        struct TagModel {
            name: String,
        }
        let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
        let tags: Tags = reqwest::get(&url)
            .await
            .map_err(|e| CoreError::Llm(format!("ollama unreachable: {e}")))?
            .error_for_status()
            .map_err(|e| CoreError::Llm(e.to_string()))?
            .json()
            .await
            .map_err(|e| CoreError::Llm(format!("ollama tags parse: {e}")))?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

#[async_trait]
impl ChatProvider for OllamaProvider {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Llm(format!("ollama request: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CoreError::Llm(format!("ollama returned {status}: {text}")));
        }
        let parsed: ChatResponse = response
            .json()
            .await
            .map_err(|e| CoreError::Llm(format!("ollama response parse: {e}")))?;
        Ok(parsed.message.content)
    }

    fn name(&self) -> &str {
        "ollama"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_chat_request_and_parses_reply() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(
                json!({"model": "llama3.2", "stream": false}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "Summary here."}
            })))
            .mount(&server)
            .await;

        let provider = OllamaProvider::new(server.uri(), "llama3.2".into());
        let reply = provider
            .complete(&[ChatMessage::user("Summarize this")])
            .await
            .unwrap();
        assert_eq!(reply, "Summary here.");
    }

    #[tokio::test]
    async fn surfaces_server_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(404).set_body_string("model not found"))
            .mount(&server)
            .await;

        let provider = OllamaProvider::new(server.uri(), "missing".into());
        let err = provider
            .complete(&[ChatMessage::user("hi")])
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("404"), "error should carry status: {msg}");
        assert!(
            msg.contains("model not found"),
            "error should carry body: {msg}"
        );
    }

    #[tokio::test]
    async fn lists_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": [{"name": "llama3.2"}, {"name": "qwen3:8b"}]
            })))
            .mount(&server)
            .await;
        let models = OllamaProvider::list_models(&server.uri()).await.unwrap();
        assert_eq!(models, vec!["llama3.2", "qwen3:8b"]);
    }
}
