# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo build              # compile
cargo run                # run server (default: 0.0.0.0:8080)
RUST_LOG=info cargo run  # run with tracing output
cargo test               # run all tests (11 pass, 1 ignored)
cargo test <test_name>   # run a single test by name
cargo test -- --include-ignored  # run candle download test (~2.3GB)
```

**Windows (PowerShell):**
```powershell
$env:RUST_LOG = "info"; cargo run
$env:OPENROUTER_API_KEY = "dummy_key_for_testing"; $env:EXTERNAL_URL = "http://localhost:8080"; cargo run
```

## Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `BIND_ADDR` | `0.0.0.0:8080` | TCP bind address |
| `EXTERNAL_URL` | `http://localhost:8080` | Base URL embedded in clone URLs returned to clients |
| `OPENROUTER_API_KEY` | _(absent)_ | Selects inference backend (see below) |
| `RUST_LOG` | _(silent)_ | Log level (`info`, `debug`) |

**Inference backend selection:**
- Real key → `OpenRouterBackend` (qwen3-coder:free with fallback chain, temperature=0)
- `dummy_key_for_testing` → `MockBackend` (instant, no network)
- Absent → `CandleBackend` (Phi-3-mini Q4 GGUF, downloads ~2.3GB on first use, needs 2GB+ RAM)

## Architecture

Forge is an Axum HTTP server that accepts a source code file or archive, completes it with AI, and vends the result as a one-time-clonable in-memory git repository.

**Request flow (`POST /forge`):**
1. Multipart upload receives `file` (single) or `archive` (.tar.gz/.zip) + optional `model` field
2. Session created immediately in `Pending` state; response returns `{slug, status:"pending", ...}` at once
3. Background Tokio task runs inference:
   - Language detected from filename extension (`language.rs`)
   - `(system, user)` prompt built (`prompt/mod.rs`) with strict "don't change signatures" rules
   - Tokens streamed to `broadcast::Sender<String>` as they arrive
   - Two git commits built in memory via `git/repo_builder.rs`
   - Session transitions to `Done { repo_pack, head_hash, completions }`
4. SSE clients connected to `GET /forge/:slug/stream` receive tokens in real time

**Session state machine:** `Pending → Processing → Done | Failed`

**Session storage:** `Arc<DashMap<slug, Session>>` — each session holds `Arc<RwLock<SessionStatus>>` + `broadcast::Sender<String>` for SSE.

**New endpoints (beyond original design):**
- `GET /forge/:slug/status` — poll `{status: "pending"|"processing"|"done"|"failed"}`
- `GET /forge/:slug/stream` — SSE token stream; sends `[DONE]` when inference finishes
- `GET /forge/:slug/diff` — unified diff (`text/plain`) of original vs completed

**Git serve (`GET /git/:slug/info/refs`, `POST /git/:slug/git-upload-pack`):**
- Git Smart HTTP read-only; auth is HTTP Basic (any username, password = `session.token`)
- Session is **deleted immediately** after the first successful `upload-pack` (one-time clone)

**Inference backends** (`inference/mod.rs`, `inference/candle.rs`):
- `OpenRouterBackend` — SSE streaming, model fallback chain on 429, `temperature=0`
- `MockBackend` — for `dummy_key_for_testing`, prepends comment to original code
- `CandleBackend` — Phi-3-mini Q4 via `spawn_blocking` + `OnceLock<ModelState>`

**Prompt design** (`prompt/mod.rs`):
- Returns `(system: String, user: String)` tuple
- System message enforces: no signature changes, no commentary, fill only TODOs
- Language-specific instructions in the system message

**Rate limiting** (`rate_limit.rs`): token-bucket per client IP, 10 burst / 1 req·s⁻¹, Axum middleware.

**Supported languages** (prompt tuning + detection): Python, Rust, JavaScript, TypeScript, Dockerfile.

## Deployment

See `fly.toml` and `Dockerfile`. Live at `https://forge-ai.fly.dev`.
- Single machine required (sessions are in-memory; `fly scale count 1`)
- Free tier: `shared-cpu-1x / 256MB` (no Candle — needs 2GB+)
