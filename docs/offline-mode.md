# Forge Offline Mode (Candle Backend)

Local CPU inference via [Candle](https://github.com/huggingface/candle) is fully implemented. It activates automatically when `OPENROUTER_API_KEY` is not set.

## Model

**Phi-3-mini-4k-instruct Q4 GGUF** — downloaded from HuggingFace on first use.

| Property | Value |
|----------|-------|
| Source | `microsoft/Phi-3-mini-4k-instruct-gguf` |
| File | `Phi-3-mini-4k-instruct-q4.gguf` |
| Download size | ~2.3 GB |
| Cache location | `~/.cache/huggingface/` |
| RAM required | ~2 GB |
| First-request latency | Minutes (download) |
| Subsequent latency | 30–120s per completion (CPU) |

## Activation

```bash
# Unset OPENROUTER_API_KEY (or simply don't set it)
EXTERNAL_URL=http://localhost:8080 RUST_LOG=info cargo run
```

The first request will log:
```
INFO forge: No OPENROUTER_API_KEY — using CandleBackend
INFO forge: Downloading Phi-3-mini-4k-instruct GGUF (first run, ~2.3 GB)…
```

Subsequent requests use the cached model with no download.

## Deployment constraints

| Platform | Viable | Notes |
|----------|--------|-------|
| Local machine (4GB+ RAM) | ✅ | Works well |
| Fly.io `shared-cpu-1x` (256MB) | ❌ | Not enough RAM |
| Fly.io `performance-1x` (2GB) | ✅ | `fly scale vm performance-1x` |
| Docker with `--memory 3g` | ✅ | Mount a volume for the model cache |

## Streaming

The Candle backend streams tokens to connected SSE clients (`GET /forge/:slug/stream`) as they are generated, token by token — identical behaviour to the OpenRouter backend.

## Running the integration test

The candle test is skipped by default (requires download):

```bash
cargo test -- --include-ignored candle_generates_nonempty_output
```
