# Forge Offline Mode

## Candle Backend
Support for local offline inference via [Candle](https://github.com/huggingface/candle) is designed but marked as Deferred for the Phase 1 MVP.

### Design
- The `InferenceBackend` trait inside `src/inference/mod.rs` isolates the logic.
- A `CandleBackend` implementation will utilize the `candle-core` and `candle-transformers` crates.

### Hardware Expectations
- It requires CPU fallback, but heavily benefits from CUDA/Metal features on the target self-hosted environment.
- The default model target will be Phi-3-mini (~3.8B) or Llama-3-8B.

### Model Cache Behavior
Model weights will be downloaded from Hugging Face on the first run into a configurable cache directory (defaulting to `~/.cache/forge/models`). The download will block the first request.

### Timeout Behavior
Because local generation might be slow, the local generation timeout is extended to 120 seconds.
