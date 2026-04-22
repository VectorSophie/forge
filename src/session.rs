use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct Session {
    pub slug: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub repo_pack: Vec<u8>,
    pub head_hash: String,
}
