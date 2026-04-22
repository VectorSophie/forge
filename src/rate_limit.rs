use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RateLimiter {
    // Map of IP to (token_count, last_update)
    buckets: Arc<DashMap<String, (f64, Instant)>>,
    capacity: f64,
    fill_rate: f64, // tokens per second
}

impl RateLimiter {
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            capacity,
            fill_rate,
        }
    }

    pub fn check_and_consume(&self, ip: &str) -> bool {
        let mut entry = self
            .buckets
            .entry(ip.to_string())
            .or_insert_with(|| (self.capacity, Instant::now()));

        let now = Instant::now();
        let elapsed = now.duration_since(entry.1).as_secs_f64();

        // Add tokens based on elapsed time
        entry.0 = (entry.0 + elapsed * self.fill_rate).min(self.capacity);
        entry.1 = now;

        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    // We attach the RateLimiter to the request extensions in main.rs
    let limiter = req
        .extensions()
        .get::<RateLimiter>()
        .cloned()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let ip = addr.ip().to_string();
    if !limiter.check_and_consume(&ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(req).await)
}
