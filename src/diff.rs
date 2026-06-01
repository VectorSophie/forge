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
            diffy::create_patch(orig_str, comp_str).to_string()
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
        // Use invalid UTF-8 sequences: 0xFF is not valid in UTF-8
        let original = vec![0xFFu8, 0xFE, 0x00, 0x00];
        let completed = vec![0xFFu8, 0xFE, 0xFF, 0xFF];
        let patch = compute_diff("blob.bin", &original, &completed);
        assert!(patch.contains("binary"));
    }
}
