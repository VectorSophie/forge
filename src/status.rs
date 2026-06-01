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
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, AppError> {
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
