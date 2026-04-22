use std::env;

#[derive(Clone)]
pub struct Config {
    pub bind_addr: String,
    pub external_url: String,
    pub openrouter_api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            external_url: env::var("EXTERNAL_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
            openrouter_api_key: env::var("OPENROUTER_API_KEY").ok(),
        }
    }
}
