# Forge Architecture

## Request flow

```
POST /forge
  │
  ├─ parse multipart (file / archive / model)
  ├─ create Session { status: Pending, token_tx: broadcast }
  ├─ insert into DashMap<slug, Session>
  ├─ spawn background Tokio task ──────────────────────────────┐
  └─ return { slug, status:"pending", ... } immediately        │
                                                               │
  Background task:                                             │
  ├─ status → Processing                                       │
  ├─ for each file:                                            │
  │    ├─ detect language from filename                        │
  │    ├─ build (system, user) prompt                          │
  │    ├─ call InferenceBackend::generate_stream(...)          │
  │    │    └─ sends tokens to broadcast::Sender<String>       │
  │    └─ accumulate (filename, original, completed)           │
  ├─ build in-memory git packfile:                             │
  │    commit 1: original files                                │
  │    commit 2: completed files (child of commit 1)           │
  ├─ status → Done { repo_pack, head_hash, completions }       │
  └─ broadcast "[DONE]" to SSE clients                         │
```

## In-memory session lifecycle

```
Pending ──► Processing ──► Done { repo_pack, head_hash, completions }
                       └──► Failed(String)
```

Sessions are stored in `Arc<DashMap<String, Session>>`. Each session holds:

- `status: Arc<RwLock<SessionStatus>>` — updated by the background task
- `token_tx: broadcast::Sender<String>` — SSE clients subscribe here
- `token: String` — Basic Auth password for Git endpoints
- `expires_at: DateTime<Utc>` — 1-hour TTL

A cleanup task (60s interval) removes expired sessions. Sessions are also removed **immediately after the first successful `git clone`** (`/git/:slug/git-upload-pack`).

## Endpoints

| Method | Path | Handler |
|--------|------|---------|
| `POST` | `/forge` | `upload::upload_handler` |
| `GET` | `/forge/:slug/status` | `status::status_handler` |
| `GET` | `/forge/:slug/stream` | `status::stream_handler` (SSE) |
| `GET` | `/forge/:slug/diff` | `diff::diff_handler` |
| `GET` | `/git/:slug/info/refs` | `git::smart_http::info_refs` |
| `POST` | `/git/:slug/git-upload-pack` | `git::smart_http::upload_pack` |

All routes share `AppState { sessions, config }` via Axum's `State` extractor.

## Git packfile construction

`git::repo_builder::RepoBuilder` builds a valid Git packfile entirely in memory:

1. `create_blob(content)` → SHA-1 of `blob {len}\0{content}`, zlib-compressed
2. `create_multi_tree([(filename, blob_hash)])` → tree object
3. `create_commit(tree, parent, author, ts, msg)` → commit object
4. `build_pack()` → `PACK` header + version + count + objects + SHA-1 checksum

The result is a self-contained packfile served verbatim to `git clone`.

## Smart HTTP boundaries

Forge implements just enough of the [Git Smart HTTP protocol](https://www.git-scm.com/docs/http-backend) to support `clone`:

- `/info/refs` advertises `HEAD` and `refs/heads/main` with a capabilities line
- `/git-upload-pack` sends `NAK\n` followed by the pre-built packfile
- Push, delta negotiation, and shallow fetches are not supported

## Inference backends

`InferenceBackend` trait (`src/inference/mod.rs`):

```rust
async fn generate_stream(system, user, model, tx: broadcast::Sender<String>)
  -> Result<String, AppError>
```

| Backend | File | Notes |
|---------|------|-------|
| `OpenRouterBackend` | `inference/mod.rs` | HTTP SSE to openrouter.ai. Model fallback chain on 429. `temperature=0`. |
| `CandleBackend` | `inference/candle.rs` | Phi-3-mini Q4 GGUF via `spawn_blocking`. `OnceLock` for model state. |
| `MockBackend` | `inference/mod.rs` | Prepends `// Mock AI completion` to original. Test/dev only. |

## Rate limiting

Token-bucket per client IP, applied as Axum middleware:
- Capacity: 10 tokens
- Refill: 1 token/second
- Stored in `Arc<DashMap<String, (f64, Instant)>>`

## Module map

```
src/
  main.rs          router setup, server bind
  config.rs        env var loading
  state.rs         AppState (sessions + config)
  session.rs       Session struct + SessionStatus enum
  upload.rs        POST /forge handler, multipart parsing, archive extraction
  diff.rs          GET /forge/:slug/diff
  status.rs        GET /forge/:slug/status  +  SSE stream
  cleanup.rs       background expired-session reaper
  rate_limit.rs    token bucket middleware
  error.rs         AppError → HTTP response mapping
  language.rs      filename → Language detection
  prompt/mod.rs    (system, user) prompt builder per language
  inference/
    mod.rs         InferenceBackend trait + OpenRouter + Mock + Candle dispatch
    candle.rs      Phi-3-mini GGUF inference (OnceLock, spawn_blocking)
  git/
    mod.rs
    repo_builder.rs  in-memory blob/tree/commit/packfile construction
    smart_http.rs    Git Smart HTTP handlers
```
