use crate::config::Config;
use crate::error::AppError;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

#[async_trait]
pub trait InferenceBackend: Send + Sync {
    async fn generate(&self, prompt: &str) -> Result<String, AppError>;
}

pub struct OpenRouterBackend {
    api_key: String,
    client: Client,
}

impl OpenRouterBackend {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap(),
        }
    }
}

#[async_trait]
impl InferenceBackend for OpenRouterBackend {
    async fn generate(&self, prompt: &str) -> Result<String, AppError> {
        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                // Defaulting to a free model for the MVP
                "model": "mistralai/mistral-7b-instruct:free",
                "messages": [
                    {"role": "user", "content": prompt}
                ]
            }))
            .send()
            .await?;

        let response_data: serde_json::Value = response.json().await?;
        if let Some(content) = response_data["choices"][0]["message"]["content"].as_str() {
            let mut cleaned = content.trim();
            // clean up common markdown block wrappings
            for lang in [
                "```rust\n",
                "```python\n",
                "```javascript\n",
                "```typescript\n",
                "```\n",
            ] {
                if cleaned.starts_with(lang) {
                    cleaned = &cleaned[lang.len()..];
                }
            }
            if cleaned.ends_with("\n```") {
                cleaned = &cleaned[..cleaned.len() - 4];
            } else if cleaned.ends_with("```") {
                cleaned = &cleaned[..cleaned.len() - 3];
            }
            Ok(cleaned.trim().to_string())
        } else {
            Err(AppError::Internal(anyhow::anyhow!(
                "Invalid response from OpenRouter: {:?}",
                response_data
            )))
        }
    }
}

pub struct CandleBackend;

impl CandleBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl InferenceBackend for CandleBackend {
    async fn generate(&self, _prompt: &str) -> Result<String, AppError> {
        // In a real implementation this would initialize Phi-3 via Candle
        // and run inference locally with a 120 second timeout.
        // For MVP, this is deferred.
        Err(AppError::Internal(anyhow::anyhow!(
            "Candle offline inference is not yet implemented."
        )))
    }
}

pub fn get_backend(config: &Config) -> Box<dyn InferenceBackend> {
    if let Some(api_key) = &config.openrouter_api_key {
        Box::new(OpenRouterBackend::new(api_key.clone()))
    } else {
        tracing::info!("No OPENROUTER_API_KEY found, falling back to Candle (unimplemented)");
        Box::new(CandleBackend::new())
    }
}
