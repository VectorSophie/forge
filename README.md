# Forge

Forge is a CLI-first, single-binary, self-hostable service that turns a single incomplete source file into an ephemeral Git repository containing both the original file and an AI-completed version as a two-commit history.

It is designed for restricted or managed environments where users often have `curl` and `git`, but may not have IDE plugins, browser AI access, Docker, or local LLM tooling.

## User Flow

1. **Upload a file** with TODOs:
   ```bash
   curl -X POST https://forge.ai.com/forge -F "file=@test.py"
   ```

2. **Forge responds** with JSON containing your session details and Git clone URL:
   ```json
   {
     "slug": "ember-raven-42",
     "detected_language": "python",
     "expires_at": "2026-04-19T13:00:00Z",
     "git_url": "https://forge.ai.com/git/ember-raven-42",
     "clone_example": "git clone https://user:TOKEN@forge.ai.com/git/ember-raven-42"
   }
   ```

3. **Clone the repository** via standard HTTPS Smart HTTP:
   ```bash
   git clone https://user:TOKEN@forge.ai.com/git/ember-raven-42
   ```

4. **Inspect the changes** in the cloned repository:
   ```bash
   cd ember-raven-42
   git log --oneline
   git diff HEAD~1
   ```
   The Git history contains exactly two commits:
   - `commit 1`: Original uploaded skeleton file (`Upload <upload@forge.local>`)
   - `commit 2`: AI-completed file (`Forge <ai@forge.local>`)

## Architecture Overview
Forge is built entirely in Rust, utilizing:
- `tokio` + `axum` for async runtime and web framework.
- In-memory `DashMap` for short-lived sessions.
- In-memory pure-Rust Git repository construction. 
- Real Git Smart HTTP endpoints tailored exclusively to serve the ephemeral memory-packed repositories.

## Limitations
- Only one file can be uploaded per request for the MVP.
- The Git repository exists purely in RAM and is **deleted immediately after the first successful clone**.
- There is no web UI. This is purely a CLI-first tool.

## Deployment

### Single Binary + Systemd (Primary)
```bash
cargo build --release
sudo ./deploy.sh
```

### Docker
```bash
docker-compose up -d --build
```

## Security Caveats
- **Shell History Leakage:** Passing tokens directly in the clone URL stores them in `~/.bash_history`. Since tokens expire in 1 hour or immediately upon the first successful clone, this risk is mitigated. For strict environments, consider configuring Git credentials helpers.
- **Source Code Privacy:** If hosted publicly, uploaded files will be processed using external LLM providers (e.g. OpenRouter) unless Local Mode is implemented. Ensure you understand data transmission policies.
- **TLS Assumptions:** Forge assumes it is running behind a reverse proxy (like Nginx or Caddy) that terminates TLS.
