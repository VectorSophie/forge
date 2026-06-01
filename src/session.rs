use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub enum SessionStatus {
    Pending,
    Processing,
    Done {
        repo_pack: Vec<u8>,
        head_hash: String,
        /// (filename, original_bytes, completed_bytes)
        completions: Vec<(String, Vec<u8>, Vec<u8>)>,
    },
    Failed(String),
}

pub struct Session {
    pub slug: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub status: Arc<RwLock<SessionStatus>>,
    /// SSE clients subscribe to this to receive tokens as inference runs.
    pub token_tx: broadcast::Sender<String>,
}

impl Session {
    pub fn new(slug: String, token: String, token_tx: broadcast::Sender<String>) -> Self {
        Self {
            slug,
            token,
            expires_at: Utc::now() + Duration::hours(1),
            status: Arc::new(RwLock::new(SessionStatus::Pending)),
            token_tx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_starts_pending() {
        let (tx, _) = broadcast::channel(16);
        let s = Session::new("slug".into(), "tok".into(), tx);
        let status = s.status.blocking_read();
        assert!(matches!(*status, SessionStatus::Pending));
    }
}
