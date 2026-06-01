use crate::error::AppError;
use tokio::sync::broadcast;

pub fn run_inference(
    _prompt: &str,
    _tx: broadcast::Sender<String>,
) -> Result<String, AppError> {
    Err(AppError::Internal(anyhow::anyhow!(
        "Candle backend not yet initialized"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires model download (~2.3 GB)"]
    fn candle_generates_nonempty_output() {
        let (tx, _) = broadcast::channel(512);
        let result = run_inference("Original Code:\nfn add(a: i32, b: i32) -> i32 { todo!() }", tx);
        let output = result.expect("inference failed");
        assert!(!output.trim().is_empty());
    }
}
