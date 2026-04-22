use crate::state::AppState;
use chrono::Utc;
use std::time::Duration;

pub async fn run_cleanup_task(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute
    loop {
        interval.tick().await;
        let now = Utc::now();
        let mut expired = Vec::new();

        for entry in state.sessions.iter() {
            if entry.value().expires_at < now {
                expired.push(entry.key().clone());
            }
        }

        for key in expired {
            tracing::info!("Cleaning up expired session: {}", key);
            state.sessions.remove(&key);
        }
    }
}
