use std::sync::{Mutex, OnceLock};

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

fn load_model_once() -> Result<&'static ModelState, AppError> {
    if let Some(m) = MODEL.get() {
        return Ok(m);
    }

    tracing::info!("Downloading Phi-3-mini-4k-instruct GGUF (first run, ~2.3 GB)…");
    let api = Api::new().map_err(|e| AppError::Internal(e.into()))?;

    let model_path = api
        .repo(Repo::new(
            "microsoft/Phi-3-mini-4k-instruct-gguf".into(),
            RepoType::Model,
        ))
        .get("Phi-3-mini-4k-instruct-q4.gguf")
        .map_err(|e| AppError::Internal(e.into()))?;

    let tokenizer_path = api
        .repo(Repo::new(
            "microsoft/Phi-3-mini-4k-instruct".into(),
            RepoType::Model,
        ))
        .get("tokenizer.json")
        .map_err(|e| AppError::Internal(e.into()))?;

    let device = Device::Cpu;
    let mut file = std::fs::File::open(&model_path).map_err(|e| AppError::Internal(e.into()))?;
    let model_content = candle_core::quantized::gguf_file::Content::read(&mut file)
        .map_err(|e| AppError::Internal(e.into()))?;
    let weights = ModelWeights::from_gguf(false, model_content, &mut file, &device)
        .map_err(|e| AppError::Internal(e.into()))?;
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("tokenizer: {e}")))?;

    let eos_token = tokenizer.token_to_id("<|end|>").unwrap_or(32007);

    MODEL.get_or_init(|| ModelState {
        weights: Mutex::new(weights),
        tokenizer,
        eos_token,
    });

    Ok(MODEL.get().unwrap())
}

pub fn run_inference(
    system: &str,
    user: &str,
    tx: broadcast::Sender<String>,
) -> Result<String, AppError> {
    let state = load_model_once()?;
    let device = Device::Cpu;

    // Phi-3 chat template with system + user roles
    let formatted = format!("<|system|>\n{system}<|end|>\n<|user|>\n{user}<|end|>\n<|assistant|>");

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

        // For incremental decoding the model returns logits for the last
        // position only, so the tensor may be rank-1 already. Handle both
        // [seq, vocab] and [vocab] shapes by taking the final row.
        let logits_last = match logits.rank() {
            1 => logits,
            _ => {
                let last = logits.dim(0).map_err(|e| AppError::Internal(e.into()))? - 1;
                logits.get(last).map_err(|e| AppError::Internal(e.into()))?
            }
        };

        let next_token = logits_last
            .argmax(candle_core::D::Minus1)
            .and_then(|t| t.to_scalar::<u32>())
            .map_err(|e| AppError::Internal(e.into()))?;

        if next_token == state.eos_token
            || next_token == 32000
            || next_token == 32001
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

    #[test]
    #[ignore = "requires model download (~2.3 GB)"]
    fn candle_generates_nonempty_output() {
        let (tx, _) = broadcast::channel(512);
        let result =
            run_inference("You are a code completion engine.", "Complete this file:\n\nfn add(a: i32, b: i32) -> i32 { todo!() }", tx);
        let output = result.expect("inference failed");
        assert!(!output.trim().is_empty());
    }
}
