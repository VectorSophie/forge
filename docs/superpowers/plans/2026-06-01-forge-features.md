# Forge Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add async upload flow, diff/status/SSE-stream endpoints, multi-file archive support, per-request model selection, and a real Candle CPU inference backend (Phi-3-mini Q4 GGUF) to Forge.

**Architecture:** `POST /forge` returns immediately with `{slug, status:"pending"}`; a background Tokio task runs inference and updates `Arc<RwLock<SessionStatus>>`. New endpoints expose status polling, SSE token streaming, and unified diffs. The `InferenceBackend` trait gains a `generate_stream` method that sends tokens to a `broadcast::Sender<String>` as they are produced.

**Tech Stack:** Rust 2021, Axum 0.7, Tokio 1.37, `candle-core`/`candle-transformers`/`candle-nn` 0.9, `hf-hub` 0.3, `tokenizers` 0.20, `diffy` 0.4, `tar` 0.4, `zip` 2.1, `tokio-stream` 0.1, `async-stream` 0.3

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `Cargo.toml` | Add new dependencies |
| Modify | `src/session.rs` | `SessionStatus` enum + async `Session` struct |
| Modify | `src/inference/mod.rs` | Updated trait + OpenRouter streaming + Mock streaming |
| Create | `src/inference/candle.rs` | Candle backend (Phi-3-mini Q4 GGUF) |
| Modify | `src/git/repo_builder.rs` | `create_multi_tree` for multi-file trees |
| Modify | `src/upload.rs` | Async flow, multi-file archive, model param |
| Create | `src/diff.rs` | `GET /forge/:slug/diff` handler |
| Create | `src/status.rs` | `GET /forge/:slug/status` + `GET /forge/:slug/stream` handlers |
| Modify | `src/main.rs` | Wire new routes |

---

## Task 1: Update Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add dependencies**

Replace the `[dependencies]` block in `Cargo.toml` with:

```toml
[dependencies]
axum = { version = "0.7", features = ["multipart", "macros"] }
tokio = { version = "1.37", features = ["full"] }
dashmap = "5.5"
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
uuid = { version = "1.8", features = ["v4"] }
chrono = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1.0"
sha1 = "0.10"
flate2 = "1.0"
hex = "0.4"
tower-http = { version = "0.5", features = ["limit", "trace"] }
base64 = "0.22"
dotenvy = "0.15"
async-trait = "0.1.89"
rand = "0.10.1"
diffy = "0.4"
tar = "0.4"
zip = "2.1"
tokio-stream = { version = "0.1", features = ["sync"] }
async-stream = "0.3"
futures-util = "0.3"
candle-core = "0.9"
candle-nn = "0.9"
candle-transformers = "0.9"
hf-hub = { version = "0.3", features = ["tokio"] }
tokenizers = { version = "0.20", default-features = false, features = ["onig"] }
```

- [ ] **Step 2: Verify it compiles**

```powershell
cargo build 2>&1 | Select-String "error"
```

Expected: no `error[E...]` lines (warnings are fine).

- [ ] **Step 3: Commit**

```powershell
git add Cargo.toml Cargo.lock
git commit -m "chore: add deps for streaming, diff, archive, candle"
```

---

## Task 2: Refactor Session for async state machine

**Files:**
- Modify: `src/session.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_pending() {
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let s = Session::new("slug".into(), "tok".into(), tx);
        let status = s.status.blocking_read();
        assert!(matches!(*status, SessionStatus::Pending));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cargo test session_starts_pending 2>&1
```

Expected: compile error — `Session::new` and `SessionStatus` not defined yet.

- [ ] **Step 3: Replace `src/session.rs` entirely**

```rust
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub enum SessionStatus {
    Pending,
    Processing,
    Done {
        repo_pack: Vec<u8>,
        head_hash: String,
        /// (filename, original_bytes, completed_bytes)
        completions: Vec<(String, Vec<u8>, Vec<u8>)>,
    },
    Failed(String),
}

pub struct Session {
    pub slug: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub status: Arc<RwLock<SessionStatus>>,
    /// SSE clients subscribe to this to receive tokens as inference runs.
    pub token_tx: broadcast::Sender<String>,
}

impl Session {
    pub fn new(slug: String, token: String, token_tx: broadcast::Sender<String>) -> Self {
        Self {
            slug,
            token,
            expires_at: Utc::now() + Duration::hours(1),
            status: Arc::new(RwLock::new(SessionStatus::Pending)),
            token_tx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_pending() {
        let (tx, _) = broadcast::channel(16);
        let s = Session::new("slug".into(), "tok".into(), tx);
        let status = s.status.blocking_read();
        assert!(matches!(*status, SessionStatus::Pending));
    }
}
```

- [ ] **Step 4: Run to verify it passes**

```powershell
cargo test session_starts_pending 2>&1
```

Expected: `test session::tests::session_starts_pending ... ok`

- [ ] **Step 5: Commit**

```powershell
git add src/session.rs
git commit -m "refactor: async Session state machine with SessionStatus"
```

---

## Task 3: Update InferenceBackend trait

**Files:**
- Modify: `src/inference/mod.rs`

The trait gains:
- `model: Option<&str>` on both methods
- `generate_stream` — sends tokens to a `broadcast::Sender<String>` and returns the full completion

- [ ] **Step 1: Write the failing test**

In `src/inference/mod.rs`, find the existing `MockBackend` and add a test below it:

```rust
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
        let (tx, mut rx) = tokio::sync::broadcast::channel(64);
        let result = backend.generate_stream("Original Code:\nhello", None, tx).await;
        assert!(result.is_ok());
        // At least one token should have been sent
        assert!(rx.try_recv().is_ok());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cargo test inference -- --nocapture 2>&1
```

Expected: compile errors — trait methods have wrong signatures.

- [ ] **Step 3: Replace `src/inference/mod.rs`**

```rust
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
        // Send word-by-word to simulate streaming
        for word in mock.split_inclusive(' ') {
            let _ = tx.send(word.to_string());
        }
        Ok(mock)
    }
}

pub fn get_backend(config: &Config) -> Box<dyn InferenceBackend> {
    if let Some(api_key) = &config.openrouter_api_key {
        if api_key == "dummy_key_for_testing" {
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
```

- [ ] **Step 4: Create `src/inference/candle.rs` stub** (needed for the module reference above)

```rust
use crate::error::AppError;
use tokio::sync::broadcast;

pub fn run_inference(
    _prompt: &str,
    _tx: broadcast::Sender<String>,
) -> Result<String, AppError> {
    Err(AppError::Internal(anyhow::anyhow!(
        "Candle backend not yet initialized — see Task 6"
    )))
}
```

- [ ] **Step 5: Add `pub mod candle;` to `src/inference/mod.rs`**

At the very top of `src/inference/mod.rs`, add:

```rust
pub mod candle;
```

- [ ] **Step 6: Run tests**

```powershell
cargo test inference 2>&1
```

Expected: both mock tests pass, candle stub compiles.

- [ ] **Step 7: Commit**

```powershell
git add src/inference/mod.rs src/inference/candle.rs
git commit -m "refactor: InferenceBackend trait with model param + generate_stream"
```

---

## Task 4: Multi-file tree support in RepoBuilder

**Files:**
- Modify: `src/git/repo_builder.rs`

- [ ] **Step 1: Write the failing test**

In `src/git/repo_builder.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_file_tree_round_trip() {
        let mut b = RepoBuilder::new();
        let h1 = b.create_blob(b"fn main() {}");
        let h2 = b.create_blob(b"x = 1");
        let tree = b.create_multi_tree(&[("main.rs", &h1), ("lib.py", &h2)]);
        assert_eq!(tree.len(), 40); // sha1 hex
        let pack = b.build_pack();
        assert!(pack.starts_with(b"PACK"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cargo test multi_file_tree_round_trip 2>&1
```

Expected: `error[E0599]: no method named 'create_multi_tree'`

- [ ] **Step 3: Add `create_multi_tree` to `RepoBuilder`**

Add after the existing `create_tree` method in `src/git/repo_builder.rs`:

```rust
/// Build a tree object containing multiple files at repo root.
pub fn create_multi_tree(&mut self, files: &[(&str, &str)]) -> String {
    let mut content = Vec::new();
    for (filename, blob_hash) in files {
        content.extend_from_slice(b"100644 ");
        content.extend_from_slice(filename.as_bytes());
        content.push(0);
        let raw = hex::decode(blob_hash).unwrap();
        content.extend_from_slice(&raw);
    }
    let (hash, _, content) = Self::hash_object("tree", &content);
    self.add_object(2, &content);
    hash
}
```

- [ ] **Step 4: Run to verify it passes**

```powershell
cargo test multi_file_tree_round_trip 2>&1
```

Expected: `test git::repo_builder::tests::multi_file_tree_round_trip ... ok`

- [ ] **Step 5: Commit**

```powershell
git add src/git/repo_builder.rs
git commit -m "feat: RepoBuilder::create_multi_tree for multi-file git trees"
```

---

## Task 5: Diff endpoint

**Files:**
- Create: `src/diff.rs`

- [ ] **Step 1: Write the failing test**

Create `src/diff.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_unified_diff_shows_change() {
        let original = b"fn foo() -> i32 { 0 }".to_vec();
        let completed = b"fn foo() -> i32 { 42 }".to_vec();
        let patch = compute_diff("main.rs", &original, &completed);
        assert!(patch.contains("-fn foo() -> i32 { 0 }"));
        assert!(patch.contains("+fn foo() -> i32 { 42 }"));
    }

    #[test]
    fn diff_falls_back_for_binary() {
        let original = vec![0u8, 1, 2, 3];
        let completed = vec![0u8, 1, 99, 3];
        let patch = compute_diff("blob.bin", &original, &completed);
        assert!(patch.contains("binary"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cargo test diff:: 2>&1
```

Expected: compile error — module and functions not defined.

- [ ] **Step 3: Implement `src/diff.rs`**

```rust
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
};

use crate::error::AppError;
use crate::session::SessionStatus;
use crate::state::AppState;

pub fn compute_diff(filename: &str, original: &[u8], completed: &[u8]) -> String {
    match (std::str::from_utf8(original), std::str::from_utf8(completed)) {
        (Ok(orig_str), Ok(comp_str)) => {
            let patch = diffy::create_patch(orig_str, comp_str);
            format!(
                "--- a/{}\n+++ b/{}\n{}",
                filename,
                filename,
                patch
            )
        }
        _ => format!(
            "--- a/{filename}\n+++ b/{filename}\n(binary files differ)\n"
        ),
    }
}

pub async fn diff_handler(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let session = state.sessions.get(&slug).ok_or(AppError::NotFound)?;
    let status = session.status.read().await;

    match &*status {
        SessionStatus::Done { completions, .. } => {
            let mut full_diff = String::new();
            for (filename, original, completed) in completions {
                full_diff.push_str(&compute_diff(filename, original, completed));
                full_diff.push('\n');
            }
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                full_diff,
            )
                .into_response())
        }
        SessionStatus::Failed(msg) => Err(AppError::BadRequest(format!("Job failed: {}", msg))),
        _ => Err(AppError::BadRequest("Job not complete yet".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_unified_diff_shows_change() {
        let original = b"fn foo() -> i32 { 0 }".to_vec();
        let completed = b"fn foo() -> i32 { 42 }".to_vec();
        let patch = compute_diff("main.rs", &original, &completed);
        assert!(patch.contains("-fn foo() -> i32 { 0 }"));
        assert!(patch.contains("+fn foo() -> i32 { 42 }"));
    }

    #[test]
    fn diff_falls_back_for_binary() {
        let original = vec![0u8, 1, 2, 3];
        let completed = vec![0u8, 1, 99, 3];
        let patch = compute_diff("blob.bin", &original, &completed);
        assert!(patch.contains("binary"));
    }
}
```

- [ ] **Step 4: Run to verify it passes**

```powershell
cargo test diff:: 2>&1
```

Expected: both diff tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src/diff.rs
git commit -m "feat: diff endpoint with unified patch output"
```

---

## Task 6: Status polling + SSE stream endpoints

**Files:**
- Create: `src/status.rs`

- [ ] **Step 1: Write the failing test**

Create `src/status.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Session, SessionStatus};
    use tokio::sync::broadcast;

    #[test]
    fn status_response_pending() {
        let s = StatusResponse::from_status(&SessionStatus::Pending);
        assert_eq!(s.status, "pending");
        assert!(s.error.is_none());
    }

    #[test]
    fn status_response_done() {
        let s = StatusResponse::from_status(&SessionStatus::Done {
            repo_pack: vec![],
            head_hash: "abc".into(),
            completions: vec![],
        });
        assert_eq!(s.status, "done");
    }

    #[test]
    fn status_response_failed() {
        let s = StatusResponse::from_status(&SessionStatus::Failed("oops".into()));
        assert_eq!(s.status, "failed");
        assert_eq!(s.error.as_deref(), Some("oops"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```powershell
cargo test status:: 2>&1
```

Expected: compile errors — module not defined.

- [ ] **Step 3: Implement `src/status.rs`**

```rust
use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use serde::Serialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::error::AppError;
use crate::session::SessionStatus;
use crate::state::AppState;

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StatusResponse {
    pub fn from_status(s: &SessionStatus) -> Self {
        match s {
            SessionStatus::Pending => Self { status: "pending", error: None },
            SessionStatus::Processing => Self { status: "processing", error: None },
            SessionStatus::Done { .. } => Self { status: "done", error: None },
            SessionStatus::Failed(msg) => Self { status: "failed", error: Some(msg.clone()) },
        }
    }
}

pub async fn status_handler(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let session = state.sessions.get(&slug).ok_or(AppError::NotFound)?;
    let status = session.status.read().await;
    Ok(Json(StatusResponse::from_status(&status)))
}

pub async fn stream_handler(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let session = state.sessions.get(&slug).ok_or(AppError::NotFound)?;
    let rx = session.token_tx.subscribe();
    drop(session);

    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(token) => Some(Ok(Event::default().data(token))),
        Err(_) => None,
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionStatus;

    #[test]
    fn status_response_pending() {
        let s = StatusResponse::from_status(&SessionStatus::Pending);
        assert_eq!(s.status, "pending");
        assert!(s.error.is_none());
    }

    #[test]
    fn status_response_done() {
        let s = StatusResponse::from_status(&SessionStatus::Done {
            repo_pack: vec![],
            head_hash: "abc".into(),
            completions: vec![],
        });
        assert_eq!(s.status, "done");
    }

    #[test]
    fn status_response_failed() {
        let s = StatusResponse::from_status(&SessionStatus::Failed("oops".into()));
        assert_eq!(s.status, "failed");
        assert_eq!(s.error.as_deref(), Some("oops"));
    }
}
```

- [ ] **Step 4: Run to verify it passes**

```powershell
cargo test status:: 2>&1
```

Expected: all 3 status tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src/status.rs
git commit -m "feat: status polling and SSE stream endpoints"
```

---

## Task 7: Refactor upload handler (async, multi-file, model selection)

**Files:**
- Modify: `src/upload.rs`

This is the largest change. `POST /forge` now:
1. Parses multipart for either `file` (single) or `archive` (`.tar.gz`/`.zip`) + optional `model` field
2. Creates a `Session` in `Pending` state immediately
3. Spawns a background Tokio task that runs inference and transitions the session to `Done` or `Failed`
4. Returns `{slug, status: "pending", ...}` immediately

- [ ] **Step 1: Replace `src/upload.rs`**

```rust
use axum::{
    extract::{Multipart, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::json;
use std::{
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::AppError;
use crate::git::repo_builder::RepoBuilder;
use crate::inference::get_backend;
use crate::language::Language;
use crate::prompt::build_prompt;
use crate::session::{Session, SessionStatus};
use crate::state::AppState;

fn generate_slug() -> String {
    let adjs = ["ember", "frost", "neon", "cyan", "void", "shadow"];
    let nouns = ["raven", "fox", "wolf", "hawk", "bear", "snake"];
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let adj = adjs[(nanos as usize) % adjs.len()];
    let noun = nouns[((nanos / 10) as usize) % nouns.len()];
    let num = 10 + (nanos % 90);
    format!("{}-{}-{}", adj, noun, num)
}

/// Extract (filename, bytes) pairs from a multipart upload.
/// Supports:
///   - `file` field: single source file
///   - `archive` field: .tar.gz or .zip containing multiple source files
/// Also reads an optional `model` field.
async fn parse_multipart(
    mut multipart: Multipart,
) -> Result<(Vec<(String, Vec<u8>)>, Option<String>), AppError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut model: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("Invalid multipart".into()))?
    {
        match field.name() {
            Some("model") => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Failed to read model field".into()))?;
                model = Some(String::from_utf8_lossy(&bytes).trim().to_string());
            }
            Some("file") => {
                let name = field
                    .file_name()
                    .ok_or_else(|| AppError::BadRequest("file field missing filename".into()))?
                    .to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Failed to read file".into()))?;
                files.push((name, data.to_vec()));
            }
            Some("archive") => {
                let name = field
                    .file_name()
                    .unwrap_or("archive")
                    .to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Failed to read archive".into()))?;

                if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
                    files.extend(extract_tar_gz(&data)?);
                } else if name.ends_with(".zip") {
                    files.extend(extract_zip(&data)?);
                } else {
                    return Err(AppError::BadRequest(
                        "archive must be .tar.gz or .zip".into(),
                    ));
                }
            }
            _ => {}
        }
    }

    if files.is_empty() {
        return Err(AppError::BadRequest("No files provided".into()));
    }

    Ok((files, model))
}

fn extract_tar_gz(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);
    let mut files = Vec::new();

    for entry in archive.entries().map_err(|e| AppError::Internal(e.into()))? {
        let mut entry = entry.map_err(|e| AppError::Internal(e.into()))?;
        if entry.header().entry_type().is_file() {
            let path = entry
                .path()
                .map_err(|e| AppError::Internal(e.into()))?
                .to_string_lossy()
                .to_string();
            let filename = std::path::Path::new(&path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).map_err(|e| AppError::Internal(e.into()))?;
            if !filename.is_empty() {
                files.push((filename, content));
            }
        }
    }
    Ok(files)
}

fn extract_zip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let cursor = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("zip error: {}", e)))?;
    let mut files = Vec::new();

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("zip entry: {}", e)))?;
        if entry.is_file() {
            let filename = std::path::Path::new(entry.name())
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).map_err(|e| AppError::Internal(e.into()))?;
            if !filename.is_empty() {
                files.push((filename, content));
            }
        }
    }
    Ok(files)
}

pub async fn upload_handler(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let (files, model) = parse_multipart(multipart).await?;

    let slug = generate_slug();
    let token = Uuid::new_v4().to_string().replace("-", "");
    let (token_tx, _) = broadcast::channel(512);

    let session = Session::new(slug.clone(), token.clone(), token_tx.clone());
    let status_arc = session.status.clone();
    state.sessions.insert(slug.clone(), session);

    // Spawn background inference task
    let state_clone = state.clone();
    let slug_clone = slug.clone();
    tokio::spawn(async move {
        // Transition to Processing
        {
            let mut s = status_arc.write().await;
            *s = SessionStatus::Processing;
        }

        let backend = get_backend(&state_clone.config);
        let mut completions: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();

        for (filename, original) in &files {
            let lang = Language::from_filename(filename);
            let text = String::from_utf8_lossy(original).to_string();
            let prompt = build_prompt(lang, &text);

            match backend
                .generate_stream(&prompt, model.as_deref(), token_tx.clone())
                .await
            {
                Ok(completed) => {
                    completions.push((
                        filename.clone(),
                        original.clone(),
                        completed.into_bytes(),
                    ));
                }
                Err(e) => {
                    let mut s = status_arc.write().await;
                    *s = SessionStatus::Failed(e.to_string());
                    // Signal SSE clients that streaming ended
                    let _ = token_tx.send("[DONE]".into());
                    return;
                }
            }
        }

        // Build git pack with all files
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut builder = RepoBuilder::new();

        // Commit 1: originals
        let orig_blobs: Vec<(String, String)> = completions
            .iter()
            .map(|(name, original, _)| (name.clone(), builder.create_blob(original)))
            .collect();
        let orig_file_refs: Vec<(&str, &str)> = orig_blobs
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let tree1 = builder.create_multi_tree(&orig_file_refs);
        let commit1 = builder.create_commit(
            &tree1,
            None,
            "Upload <upload@forge.local>",
            now,
            "Original skeleton file",
        );

        // Commit 2: completions
        let comp_blobs: Vec<(String, String)> = completions
            .iter()
            .map(|(name, _, completed)| (name.clone(), builder.create_blob(completed)))
            .collect();
        let comp_file_refs: Vec<(&str, &str)> = comp_blobs
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let tree2 = builder.create_multi_tree(&comp_file_refs);
        let commit2 = builder.create_commit(
            &tree2,
            Some(&commit1),
            "Forge <ai@forge.local>",
            now + 1,
            "AI completion",
        );

        let pack = builder.build_pack();

        // Transition to Done and signal SSE clients
        {
            let mut s = status_arc.write().await;
            *s = SessionStatus::Done {
                repo_pack: pack,
                head_hash: commit2,
                completions,
            };
        }
        let _ = token_tx.send("[DONE]".into());
    });

    let git_url = format!("{}/git/{}", state.config.external_url, slug);
    let clone_example = format!(
        "git clone {}://user:{}@{}/git/{}",
        if state.config.external_url.starts_with("https") { "https" } else { "http" },
        token,
        state.config.external_url
            .trim_start_matches("http://")
            .trim_start_matches("https://"),
        slug
    );

    Ok(Json(json!({
        "slug": slug,
        "status": "pending",
        "expires_at": Utc::now() + chrono::Duration::hours(1),
        "git_url": git_url,
        "clone_example": clone_example,
        "status_url": format!("{}/forge/{}/status", state.config.external_url, slug),
        "stream_url": format!("{}/forge/{}/stream", state.config.external_url, slug),
        "diff_url": format!("{}/forge/{}/diff", state.config.external_url, slug),
    })))
}
```

- [ ] **Step 2: Compile check**

```powershell
cargo build 2>&1
```

Expected: no errors (fix any that arise — likely import adjustments).

- [ ] **Step 3: Commit**

```powershell
git add src/upload.rs
git commit -m "feat: async upload with multi-file archive support and model selection"
```

---

## Task 8: Wire routes in main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace `src/main.rs`**

```rust
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tokio::net::TcpListener;

mod cleanup;
mod config;
mod diff;
mod error;
mod git;
mod inference;
mod language;
mod prompt;
mod rate_limit;
mod session;
mod state;
mod status;
mod upload;

use crate::config::Config;
use crate::rate_limit::RateLimiter;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env();
    let bind_addr = config.bind_addr.clone();

    let state = AppState::new(config);
    let state_clone = state.clone();
    tokio::spawn(async move { cleanup::run_cleanup_task(state_clone).await });

    let rate_limiter = RateLimiter::new(10.0, 1.0);

    let app = Router::new()
        .route("/forge", post(upload::upload_handler))
        .route("/forge/:slug/status", get(status::status_handler))
        .route("/forge/:slug/stream", get(status::stream_handler))
        .route("/forge/:slug/diff", get(diff::diff_handler))
        .route("/git/:slug/info/refs", get(git::smart_http::info_refs))
        .route("/git/:slug/git-upload-pack", post(git::smart_http::upload_pack))
        .layer(middleware::from_fn(rate_limit::rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
        .with_state(state);

    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!("Forge listening on {}", bind_addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
```

- [ ] **Step 2: Also update `src/git/smart_http.rs` to read `repo_pack` and `head_hash` from `SessionStatus::Done`**

In `src/git/smart_http.rs`, replace the `validate_auth` and `upload_pack` functions:

```rust
use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;

use crate::error::AppError;
use crate::session::SessionStatus;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct InfoRefsQuery {
    pub service: Option<String>,
}

fn pkt_line(s: &str) -> String {
    let len = s.len() + 4;
    format!("{:04x}{}", len, s)
}

async fn validate_auth(req: &Request, slug: &str, state: &AppState) -> Result<String, AppError> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    if !auth_header.starts_with("Basic ") {
        return Err(AppError::Unauthorized);
    }

    let decoded = general_purpose::STANDARD
        .decode(&auth_header[6..])
        .map_err(|_| AppError::Unauthorized)?;
    let credentials = String::from_utf8(decoded).map_err(|_| AppError::Unauthorized)?;
    let parts: Vec<&str> = credentials.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(AppError::Unauthorized);
    }
    let password = parts[1];

    let session = state.sessions.get(slug).ok_or(AppError::NotFound)?;
    if session.token != password {
        return Err(AppError::Unauthorized);
    }
    if chrono::Utc::now() > session.expires_at {
        drop(session);
        state.sessions.remove(slug);
        return Err(AppError::Unauthorized);
    }

    let status = session.status.read().await;
    match &*status {
        SessionStatus::Done { head_hash, .. } => Ok(head_hash.clone()),
        SessionStatus::Failed(msg) => {
            Err(AppError::BadRequest(format!("Job failed: {}", msg)))
        }
        _ => Err(AppError::BadRequest("Job not complete yet".into())),
    }
}

pub async fn info_refs(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<InfoRefsQuery>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    if query.service.as_deref() != Some("git-upload-pack") {
        return Err(AppError::BadRequest("Only git-upload-pack is supported".into()));
    }

    let head_hash = match validate_auth(&req, &slug, &state).await {
        Ok(h) => h,
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Forge\"")],
                "Unauthorized",
            ).into_response());
        }
    };

    let mut body = String::new();
    body.push_str(&pkt_line("# service=git-upload-pack\n"));
    body.push_str("0000");
    let capabilities = "symref=HEAD:refs/heads/main agent=forge/0.1.0";
    body.push_str(&pkt_line(&format!("{} HEAD\0{}\n", head_hash, capabilities)));
    body.push_str(&pkt_line(&format!("{} refs/heads/main\n", head_hash)));
    body.push_str("0000");

    Ok((
        [(header::CONTENT_TYPE, "application/x-git-upload-pack-advertisement".to_string()),
         (header::CACHE_CONTROL, "no-cache".to_string())],
        body,
    ).into_response())
}

pub async fn upload_pack(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    match validate_auth(&req, &slug, &state).await {
        Ok(_) => {}
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Forge\"")],
                "Unauthorized",
            ).into_response());
        }
    }

    let pack = {
        let session = state.sessions.get(&slug).ok_or(AppError::NotFound)?;
        let status = session.status.read().await;
        match &*status {
            SessionStatus::Done { repo_pack, .. } => repo_pack.clone(),
            _ => return Err(AppError::BadRequest("Job not complete yet".into())),
        }
    };

    state.sessions.remove(&slug);

    let mut resp_body = Vec::new();
    resp_body.extend_from_slice(b"0008NAK\n");
    resp_body.extend_from_slice(&pack);

    Ok((
        [(header::CONTENT_TYPE, "application/x-git-upload-pack-result".to_string()),
         (header::CACHE_CONTROL, "no-cache".to_string())],
        Body::from(resp_body),
    ).into_response())
}
```

- [ ] **Step 3: Full build + test**

```powershell
cargo build 2>&1
cargo test 2>&1
```

Expected: builds cleanly, all tests pass.

- [ ] **Step 4: Commit**

```powershell
git add src/main.rs src/git/smart_http.rs
git commit -m "feat: wire diff/status/stream routes, update git handlers for async session"
```

---

## Task 9: Candle Backend (Phi-3-mini Q4 GGUF, CPU)

**Files:**
- Modify: `src/inference/candle.rs`

This replaces the stub from Task 3. The model is downloaded once via `hf-hub` (cached in `~/.cache/huggingface/`) and kept in a process-level `OnceLock`.

- [ ] **Step 1: Write the failing test**

In `src/inference/candle.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    // NOTE: This test downloads ~2.3 GB on first run.
    // Skip in CI unless HF_HOME is pre-populated.
    #[test]
    #[ignore = "requires model download (~2.3 GB)"]
    fn candle_generates_nonempty_output() {
        let (tx, _) = broadcast::channel(512);
        let result = run_inference("Original Code:\nfn add(a: i32, b: i32) -> i32 { todo!() }", tx);
        let output = result.expect("inference failed");
        assert!(!output.trim().is_empty());
    }
}
```

- [ ] **Step 2: Run to confirm it compiles and is skipped**

```powershell
cargo test candle 2>&1
```

Expected: `test inference::candle::tests::candle_generates_nonempty_output ... ignored`

- [ ] **Step 3: Implement `src/inference/candle.rs`**

```rust
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_phi3::ModelWeights;
use hf_hub::{api::sync::Api, Repo, RepoType};
use tokenizers::Tokenizer;
use tokio::sync::broadcast;

use crate::error::AppError;

struct ModelState {
    weights: Mutex<ModelWeights>,
    tokenizer: Tokenizer,
    eos_token: u32,
}

static MODEL: OnceLock<ModelState> = OnceLock::new();

fn load_model_once() -> Result<&'static ModelState> {
    if let Some(m) = MODEL.get() {
        return Ok(m);
    }

    tracing::info!("Downloading Phi-3-mini-4k-instruct GGUF (first run, ~2.3 GB)…");
    let api = Api::new()?;

    let model_path = api
        .repo(Repo::new(
            "microsoft/Phi-3-mini-4k-instruct-gguf".into(),
            RepoType::Model,
        ))
        .get("Phi-3-mini-4k-instruct-q4.gguf")?;

    let tokenizer_path = api
        .repo(Repo::new(
            "microsoft/Phi-3-mini-4k-instruct".into(),
            RepoType::Model,
        ))
        .get("tokenizer.json")?;

    let device = Device::Cpu;
    let mut file = std::fs::File::open(&model_path)?;
    let model_content = candle_core::quantized::gguf_file::Content::read(&mut file)?;
    let weights = ModelWeights::from_gguf(model_content, &mut file, &device)?;
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;

    // Phi-3 <|end|> token id
    let eos_token = tokenizer
        .token_to_id("<|end|>")
        .unwrap_or(32007);

    MODEL.get_or_init(|| ModelState {
        weights: Mutex::new(weights),
        tokenizer,
        eos_token,
    });

    Ok(MODEL.get().unwrap())
}

pub fn run_inference(
    prompt: &str,
    tx: broadcast::Sender<String>,
) -> Result<String, AppError> {
    let state = load_model_once()?;
    let device = Device::Cpu;

    // Phi-3 chat template
    let formatted = format!("<|user|>\n{prompt}<|end|>\n<|assistant|>");

    let encoding = state
        .tokenizer
        .encode(formatted, true)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("encode: {e}")))?;
    let mut tokens: Vec<u32> = encoding.get_ids().to_vec();

    let mut model = state.weights.lock().unwrap();
    let max_new_tokens = 1024usize;
    let mut generated = String::new();

    for i in 0..max_new_tokens {
        let context_size = if i == 0 { tokens.len() } else { 1 };
        let start_pos = tokens.len() - context_size;
        let input = Tensor::new(&tokens[start_pos..], &device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| AppError::Internal(e.into()))?;

        let logits = model
            .forward(&input, start_pos)
            .map_err(|e| AppError::Internal(e.into()))?;
        let logits = logits
            .squeeze(0)
            .and_then(|t| t.to_dtype(candle_core::DType::F32))
            .map_err(|e| AppError::Internal(e.into()))?;

        // Last token logits
        let last = logits.dim(0).map_err(|e| AppError::Internal(e.into()))? - 1;
        let logits_last = logits
            .get(last)
            .map_err(|e| AppError::Internal(e.into()))?;

        let next_token = logits_last
            .argmax(candle_core::D::Minus1)
            .and_then(|t| t.to_scalar::<u32>())
            .map_err(|e| AppError::Internal(e.into()))?;

        if next_token == state.eos_token
            || next_token == 32000  // <|endoftext|>
            || next_token == 32001  // <|end_of_turn|>
        {
            break;
        }

        tokens.push(next_token);

        if let Ok(piece) = state.tokenizer.decode(&[next_token], false) {
            if !piece.is_empty() {
                let _ = tx.send(piece.clone());
                generated.push_str(&piece);
            }
        }
    }

    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    #[ignore = "requires model download (~2.3 GB)"]
    fn candle_generates_nonempty_output() {
        let (tx, _) = broadcast::channel(512);
        let result =
            run_inference("Original Code:\nfn add(a: i32, b: i32) -> i32 { todo!() }", tx);
        let output = result.expect("inference failed");
        assert!(!output.trim().is_empty());
    }
}
```

- [ ] **Step 4: Compile check**

```powershell
cargo build 2>&1
```

Expected: no errors.

- [ ] **Step 5: Run normal tests (candle ignored)**

```powershell
cargo test 2>&1
```

Expected: all non-ignored tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src/inference/candle.rs
git commit -m "feat: Candle CPU backend with Phi-3-mini Q4 GGUF via hf-hub"
```

---

## Task 10: End-to-end smoke test + Fly.io deployment

**Files:**
- Create: `fly.toml`
- Create: `Dockerfile`

- [ ] **Step 1: Run full test suite**

```powershell
cargo test 2>&1
```

Expected: all non-ignored tests pass.

- [ ] **Step 2: Run server locally with mock key**

```powershell
$env:OPENROUTER_API_KEY = "dummy_key_for_testing"
$env:EXTERNAL_URL = "http://localhost:8080"
cargo run
```

In a second terminal:

```powershell
# Upload a file
$response = Invoke-RestMethod -Method POST -Uri "http://localhost:8080/forge" `
  -Form @{ file = Get-Item "src/session.rs" }
$slug = $response.slug
Write-Host "Slug: $slug"

# Poll status
Invoke-RestMethod "http://localhost:8080/forge/$slug/status"

# After status is "done":
Invoke-RestMethod "http://localhost:8080/forge/$slug/diff"
```

Expected: status cycles pending → processing → done, diff shows `// Mock AI completion` lines.

- [ ] **Step 3: Create `Dockerfile`**

```dockerfile
FROM rust:1.78-slim AS builder
WORKDIR /app
COPY . .
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/forge .
EXPOSE 8080
CMD ["./forge"]
```

- [ ] **Step 4: Create `fly.toml`**

```toml
app = "forge-ai"
primary_region = "iad"

[build]

[env]
  BIND_ADDR = "0.0.0.0:8080"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = true
  auto_start_machines = true
  min_machines_running = 0

[[vm]]
  memory = "2gb"
  cpus = 2
```

> **Note on Candle:** The Phi-3-mini model (~2.3 GB) will be downloaded to the container's `~/.cache/huggingface/` on first request when `OPENROUTER_API_KEY` is unset. Use a Fly volume for persistence between restarts:
> `fly volumes create hf_cache --size 5 --region iad`
> Then add to `fly.toml`:
> ```toml
> [mounts]
>   source = "hf_cache"
>   destination = "/root/.cache/huggingface"
> ```

- [ ] **Step 5: Deploy to Fly.io**

```powershell
# Install flyctl if needed: https://fly.io/docs/hands-on/install-flyctl/
fly auth login
fly launch --no-deploy        # creates app, pick region
fly secrets set OPENROUTER_API_KEY=<your-key> EXTERNAL_URL=https://forge-ai.fly.dev
fly deploy
fly status
```

- [ ] **Step 6: Smoke test on live URL**

```powershell
$base = "https://forge-ai.fly.dev"
$resp = Invoke-RestMethod -Method POST -Uri "$base/forge" `
  -Form @{ file = Get-Item "src/session.rs" }
Write-Host ($resp | ConvertTo-Json)
```

Expected: JSON with `slug`, `status: "pending"`, `diff_url`, `stream_url`.

- [ ] **Step 7: Commit deployment files**

```powershell
git add Dockerfile fly.toml
git commit -m "chore: add Dockerfile and fly.toml for Fly.io deployment"
```

- [ ] **Step 8: Push**

```powershell
git push origin main
```

---

## Summary of new endpoints

| Method | Path | Returns |
|--------|------|---------|
| `POST` | `/forge` | `{slug, status:"pending", git_url, diff_url, stream_url, status_url}` |
| `GET` | `/forge/:slug/status` | `{status:"pending"\|"processing"\|"done"\|"failed"}` |
| `GET` | `/forge/:slug/stream` | SSE token stream, `[DONE]` event when complete |
| `GET` | `/forge/:slug/diff` | `text/plain` unified diff |
| `GET` | `/git/:slug/info/refs` | Git smart HTTP (unchanged) |
| `POST` | `/git/:slug/git-upload-pack` | Git smart HTTP (unchanged) |
