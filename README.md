# Forge

Forge turns an incomplete source file into an ephemeral Git repository containing the original and an AI-completed version as a two-commit history — accessible via standard `git clone`.

It is designed for environments where you have `curl` and `git` but may not have IDE plugins, browser AI access, or local LLM tooling.

**Live demo:** `https://forge-ai.fly.dev`

---

## Supported languages

`.py` · `.rs` · `.js` · `.ts` · `Dockerfile`

---

## Quick start

Upload a file containing `TODO`, `todo!()`, `pass`, `unimplemented!()`, or empty function bodies. Forge fills them in.

### curl (Linux / macOS / Windows)

```bash
# 1. Upload
curl -X POST https://forge-ai.fly.dev/forge \
  -F "file=@my_file.rs" \
  -o result.json

cat result.json

# 2. Poll until done (replace SLUG)
curl https://forge-ai.fly.dev/forge/SLUG/status

# 3. View the diff
curl https://forge-ai.fly.dev/forge/SLUG/diff

# 4. Or clone the full git history (token is in result.json)
git clone https://user:TOKEN@forge-ai.fly.dev/git/SLUG
```

### PowerShell (Windows)

```powershell
# 1. Upload
$r = curl.exe -s -X POST https://forge-ai.fly.dev/forge `
     -F "file=@my_file.rs" | ConvertFrom-Json

# 2. Poll status
curl.exe -s "https://forge-ai.fly.dev/forge/$($r.slug)/status"

# 3. View diff
curl.exe -s "https://forge-ai.fly.dev/forge/$($r.slug)/diff"

# 4. Clone
git clone $r.clone_example
```

### Python

```python
import requests, time

r = requests.post("https://forge-ai.fly.dev/forge",
                  files={"file": open("my_file.py", "rb")}).json()
slug = r["slug"]

while True:
    s = requests.get(f"https://forge-ai.fly.dev/forge/{slug}/status").json()
    print(s["status"])
    if s["status"] in ("done", "failed"):
        break
    time.sleep(3)

print(requests.get(f"https://forge-ai.fly.dev/forge/{slug}/diff").text)
```

### Upload a zip/tar.gz (multiple files)

```bash
# tar.gz
tar czf bundle.tar.gz src/*.rs
curl -X POST https://forge-ai.fly.dev/forge \
  -F "archive=@bundle.tar.gz"

# zip
zip bundle.zip src/*.rs
curl -X POST https://forge-ai.fly.dev/forge \
  -F "archive=@bundle.zip"
```

### Choose a specific model

```bash
curl -X POST https://forge-ai.fly.dev/forge \
  -F "file=@my_file.py" \
  -F "model=meta-llama/llama-3.2-3b-instruct:free"
```

---

## API reference

All responses are JSON unless noted.

### `POST /forge`

Upload a file or archive for AI completion.

**Form fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `file` | One of | Single source file |
| `archive` | One of | `.tar.gz` or `.zip` of source files |
| `model` | No | OpenRouter model ID to use (overrides default) |

**Response:**

```json
{
  "slug": "ember-raven-42",
  "status": "pending",
  "expires_at": "2026-04-19T13:00:00Z",
  "git_url": "https://forge-ai.fly.dev/git/ember-raven-42",
  "clone_example": "git clone https://user:TOKEN@forge-ai.fly.dev/git/ember-raven-42",
  "status_url": "https://forge-ai.fly.dev/forge/ember-raven-42/status",
  "stream_url":  "https://forge-ai.fly.dev/forge/ember-raven-42/stream",
  "diff_url":    "https://forge-ai.fly.dev/forge/ember-raven-42/diff"
}
```

### `GET /forge/:slug/status`

Poll for job completion.

```json
{ "status": "pending" }
{ "status": "processing" }
{ "status": "done" }
{ "status": "failed", "error": "..." }
```

### `GET /forge/:slug/stream`

Server-Sent Events stream of tokens as the model generates them. Connect before or after uploading.

```bash
curl -N https://forge-ai.fly.dev/forge/SLUG/stream
```

### `GET /forge/:slug/diff`

Unified diff of original vs AI-completed file(s). Returns `text/plain`.

### Git clone endpoints

```
GET  /git/:slug/info/refs?service=git-upload-pack
POST /git/:slug/git-upload-pack
```

Standard Git Smart HTTP. Use `git clone https://user:TOKEN@.../git/SLUG`. The token is the value after `user:` in `clone_example`. **Clone is one-time use** — the session is deleted immediately after the packfile is served.

---

## Session lifecycle

- Sessions expire after **1 hour** (or immediately after the first `git clone`)
- In-memory only — no disk state
- A background task cleans up expired sessions every 60 seconds

---

## Self-hosting

### Option A: Fly.io (recommended, free tier)

```bash
# 1. Install flyctl: https://fly.io/docs/hands-on/install-flyctl/
fly auth login

# 2. Create app
fly apps create forge-ai

# 3. Set secrets
fly secrets set \
  OPENROUTER_API_KEY=your_key_here \
  EXTERNAL_URL=https://forge-ai.fly.dev

# 4. Deploy
fly deploy

# 5. Scale to 1 machine (required — sessions are in-memory)
fly scale count 1
```

The included `fly.toml` targets `shared-cpu-1x / 256MB` which falls within Fly's free allowance. Machines auto-stop when idle.

> **Candle/offline backend:** Requires at least `performance-1x` (2GB RAM) to load Phi-3-mini Q4 (~2.3GB). Run `fly scale vm performance-1x` to enable it.

### Option B: Docker

```bash
docker build -t forge .

docker run -d \
  -p 8080:8080 \
  -e OPENROUTER_API_KEY=your_key_here \
  -e EXTERNAL_URL=http://your-server:8080 \
  forge
```

### Option C: systemd (bare metal / VPS)

```bash
# Build
cargo build --release

# Copy binary
sudo cp target/release/forge /usr/local/bin/forge

# Create service file
sudo tee /etc/systemd/system/forge.service > /dev/null <<EOF
[Unit]
Description=Forge AI code completion service
After=network.target

[Service]
ExecStart=/usr/local/bin/forge
Restart=on-failure
Environment=OPENROUTER_API_KEY=your_key_here
Environment=EXTERNAL_URL=https://your-domain.com
Environment=BIND_ADDR=0.0.0.0:8080
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now forge
```

Put Caddy or Nginx in front for TLS termination.

### Option D: local development

```bash
# With OpenRouter (real AI)
OPENROUTER_API_KEY=your_key_here \
EXTERNAL_URL=http://localhost:8080 \
RUST_LOG=info \
cargo run

# With mock backend (instant, no API key needed)
OPENROUTER_API_KEY=dummy_key_for_testing \
EXTERNAL_URL=http://localhost:8080 \
cargo run

# With offline Candle backend (downloads ~2.3GB model on first run)
EXTERNAL_URL=http://localhost:8080 \
RUST_LOG=info \
cargo run
```

**Windows (PowerShell):**

```powershell
$env:OPENROUTER_API_KEY = "your_key_here"
$env:EXTERNAL_URL      = "http://localhost:8080"
$env:RUST_LOG          = "info"
cargo run
```

---

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BIND_ADDR` | `0.0.0.0:8080` | TCP address to listen on |
| `EXTERNAL_URL` | `http://localhost:8080` | Base URL embedded in clone URLs returned to clients |
| `OPENROUTER_API_KEY` | _(absent)_ | OpenRouter API key. If absent → Candle offline backend. Set to `dummy_key_for_testing` for mock mode. |
| `RUST_LOG` | _(silent)_ | Log level, e.g. `info`, `debug` |

---

## Inference backends

| Backend | Activated when | Notes |
|---------|---------------|-------|
| **OpenRouter** | `OPENROUTER_API_KEY` is a real key | Streams from `qwen/qwen3-coder:free` with fallback chain. Pass `model=` in the upload to override. |
| **Mock** | `OPENROUTER_API_KEY=dummy_key_for_testing` | Returns original code with `// Mock AI completion` prefix. Instant, no network. |
| **Candle (offline)** | `OPENROUTER_API_KEY` not set | Downloads Phi-3-mini-4k-instruct Q4 GGUF (~2.3GB) from HuggingFace on first run, cached at `~/.cache/huggingface/`. Requires 2GB+ RAM. |

---

## Building from source

```bash
# Prerequisites: Rust 1.88+
cargo build --release        # binary at target/release/forge
cargo test                   # 11 unit tests (candle test skipped by default)
cargo test -- --include-ignored  # run candle test (requires model download)
```

---

## Architecture

See [docs/architecture.md](docs/architecture.md) for the full design. In brief:

- **`POST /forge`** returns immediately (`status: pending`). Inference runs in a background Tokio task.
- Two Git commits are constructed entirely in memory: one for the original file, one for the AI completion.
- The packfile is served once via Git Smart HTTP then discarded.
- Token bucket rate limiting (10 burst / 1 req·s⁻¹ per IP).

---

## Security notes

See [docs/security.md](docs/security.md). Key points:

- Tokens in clone URLs appear in shell history. Mitigated by 1-hour expiry and one-time use.
- Source code is sent to OpenRouter unless using the Candle offline backend.
- Always run behind a TLS-terminating proxy (Caddy, Nginx, or Fly's built-in HTTPS).
