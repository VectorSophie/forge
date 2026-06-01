pub mod candle;

use crate::config::Config;
use crate::error::AppError;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tokio::sync::broadcast;

#[async_trait]
pub trait InferenceBackend: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, AppError>;

    async fn generate_stream(
        &self,
        prompt: &str,
        model: Option<&str>,
        tx: broadcast::Sender<String>,
    ) -> Result<String, AppError>;
}

// ── OpenRouter ────────────────────────────────────────────────────────────────

pub struct OpenRouterBackend {
    api_key: String,
    client: Client,
}

impl OpenRouterBackend {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap(),
        }
    }
}

fn strip_markdown_fences(s: &str) -> String {
    let mut cleaned = s.trim();
    for lang in ["```rust\n", "```python\n", "```javascript\n", "```typescript\n", "```\n"] {
        if cleaned.starts_with(lang) {
            cleaned = &cleaned[lang.len()..];
            break;
        }
    }
    if cleaned.ends_with("\n```") {
        cleaned = &cleaned[..cleaned.len() - 4];
    } else if cleaned.ends_with("```") {
        cleaned = &cleaned[..cleaned.len() - 3];
    }
    cleaned.trim().to_string()
}

#[async_trait]
impl InferenceBackend for OpenRouterBackend {
    async fn generate(&self, prompt: &str, model: Option<&str>) -> Result<String, AppError> {
        let model = model.unwrap_or("mistralai/mistral-7b-instruct:free");
        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "model": model,
                "messages": [{"role": "user", "content": prompt}]
            }))
            .send()
            .await?;

        let data: serde_json::Value = response.json().await?;
        if let Some(content) = data["choices"][0]["message"]["content"].as_str() {
            Ok(strip_markdown_fences(content))
        } else {
            Err(AppError::Internal(anyhow::anyhow!(
                "Invalid response from OpenRouter: {:?}", data
            )))
        }
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        model: Option<&str>,
        tx: broadcast::Sender<String>,
    ) -> Result<String, AppError> {
        use futures_util::StreamExt;

        let model = model.unwrap_or("mistralai/mistral-7b-instruct:free");
        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "model": model,
                "stream": true,
                "messages": [{"role": "user", "content": prompt}]
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(anyhow::anyhow!(
                "OpenRouter HTTP {}: {}", status, body
            )));
        }

        let mut stream = response.bytes_stream();
        let mut full = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                let line = line.trim();
                if line == "data: [DONE]" {
                    break;
                }
                let Some(json_str) = line.strip_prefix("data: ") else { continue };
                let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) else { continue };
                if let Some(token) = val["choices"][0]["delta"]["content"].as_str() {
                    if !token.is_empty() {
                        full.push_str(token);
                        let _ = tx.send(token.to_string());
                    }
                }
            }
        }

        Ok(strip_markdown_fences(&full))
    }
}

// ── Candle ────────────────────────────────────────────────────────────────────

pub struct CandleBackend;

#[async_trait]
impl InferenceBackend for CandleBackend {
    async fn generate(&self, prompt: &str, model: Option<&str>) -> Result<String, AppError> {
        let (tx, _) = broadcast::channel(512);
        self.generate_stream(prompt, model, tx).await
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        _model: Option<&str>,
        tx: broadcast::Sender<String>,
    ) -> Result<String, AppError> {
        use crate::inference::candle::run_inference;
        let prompt = prompt.to_string();
        tokio::task::spawn_blocking(move || run_inference(&prompt, tx))
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn_blocking: {}", e)))?
    }
}

// ── Mock ──────────────────────────────────────────────────────────────────────

pub struct MockBackend;

#[async_trait]
impl InferenceBackend for MockBackend {
    async fn generate(&self, prompt: &str, _model: Option<&str>) -> Result<String, AppError> {
        let (tx, _) = broadcast::channel(512);
        self.generate_stream(prompt, None, tx).await
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        _model: Option<&str>,
        tx: broadcast::Sender<String>,
    ) -> Result<String, AppError> {
        let code = prompt.split("Original Code:\n").last().unwrap_or("");
        let mock = format!("// Mock AI completion\n{}", code);
        for word in mock.split_inclusive(' ') {
            let _ = tx.send(word.to_string());
        }
        Ok(mock)
    }
}

pub fn get_backend(config: &Config) -> Box<dyn InferenceBackend> {
    if let Some(api_key) = &config.openrouter_api_key {
        if api_key.trim() == "dummy_key_for_testing" {
            tracing::info!("Using MockBackend");
            return Box::new(MockBackend);
        }
        Box::new(OpenRouterBackend::new(api_key.clone()))
    } else {
        tracing::info!("No OPENROUTER_API_KEY — using CandleBackend");
        Box::new(CandleBackend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_generate_with_model_param() {
        let backend = MockBackend;
        let result = backend.generate("Original Code:\nfn foo() {}", Some("gpt-4")).await;
        assert!(result.unwrap().contains("// Mock AI completion"));
    }

    #[tokio::test]
    async fn mock_generate_stream_sends_tokens() {
        let backend = MockBackend;
        let (tx, mut rx) = broadcast::channel(64);
        let result = backend.generate_stream("Original Code:\nhello", None, tx).await;
        assert!(result.is_ok());
        assert!(rx.try_recv().is_ok());
    }
}
