use crate::config::Config;
use crate::session::Session;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub sessions: Arc<DashMap<String, Session>>,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            config: Arc::new(config),
        }
    }
}
