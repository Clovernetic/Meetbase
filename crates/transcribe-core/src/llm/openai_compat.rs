//! OpenAI-compatible chat backend (`POST {base_url}/chat/completions`).
//!
//! Works with OpenAI, Groq, OpenRouter, Anthropic's OpenAI-compat endpoint,
//! LM Studio, vLLM and anything else speaking the same dialect — the user
//! supplies base URL, API key and model name.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::error::{CoreError, Result};

use super::{ChatMessage, ChatProvider};

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[async_trait]
impl ChatProvider for OpenAiCompatProvider {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": messages,
        });
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Llm(format!("request failed: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CoreError::Llm(format!(
                "provider returned {status}: {text}"
            )));
        }
        let parsed: ChatResponse = response
            .json()
            .await
            .map_err(|e| CoreError::Llm(format!("response parse: {e}")))?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| CoreError::Llm("provider returned no choices".into()))
    }

    fn name(&self) -> &str {
        "openai-compatible"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_bearer_auth_and_parses_first_choice() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "OK"}}]
            })))
            .mount(&server)
            .await;

        let provider = OpenAiCompatProvider::new(
            format!("{}/v1", server.uri()),
            "sk-test".into(),
            "gpt-test".into(),
        );
        let reply = provider.complete(&[ChatMessage::user("hi")]).await.unwrap();
        assert_eq!(reply, "OK");
    }

    #[tokio::test]
    async fn trailing_slash_in_base_url_is_tolerated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "OK"}}]
            })))
            .mount(&server)
            .await;
        let provider =
            OpenAiCompatProvider::new(format!("{}/v1/", server.uri()), "k".into(), "m".into());
        assert_eq!(
            provider.complete(&[ChatMessage::user("hi")]).await.unwrap(),
            "OK"
        );
    }

    #[tokio::test]
    async fn empty_choices_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices": []})))
            .mount(&server)
            .await;
        let provider =
            OpenAiCompatProvider::new(format!("{}/v1", server.uri()), "k".into(), "m".into());
        assert!(provider.complete(&[ChatMessage::user("hi")]).await.is_err());
    }
}
