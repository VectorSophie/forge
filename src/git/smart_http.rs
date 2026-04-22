use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use std::collections::HashMap;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct InfoRefsQuery {
    pub service: Option<String>,
}

fn pkt_line(s: &str) -> String {
    let len = s.len() + 4;
    format!("{:04x}{}", len, s)
}

fn validate_auth(req: &Request, slug: &str, state: &AppState) -> Result<String, AppError> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    if !auth_header.starts_with("Basic ") {
        return Err(AppError::Unauthorized);
    }

    let decoded = general_purpose::STANDARD
        .decode(&auth_header[6..])
        .map_err(|_| AppError::Unauthorized)?;
    let credentials = String::from_utf8(decoded).map_err(|_| AppError::Unauthorized)?;

    let parts: Vec<&str> = credentials.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(AppError::Unauthorized);
    }
    let (_, password) = (parts[0], parts[1]);

    let session = state.sessions.get(slug).ok_or(AppError::NotFound)?;
    if session.token != password {
        return Err(AppError::Unauthorized);
    }

    if chrono::Utc::now() > session.expires_at {
        drop(session);
        state.sessions.remove(slug);
        return Err(AppError::Unauthorized);
    }

    Ok(session.head_hash.clone())
}

pub async fn info_refs(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<InfoRefsQuery>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    if query.service.as_deref() != Some("git-upload-pack") {
        return Err(AppError::BadRequest(
            "Only git-upload-pack is supported".to_string(),
        ));
    }

    let head_hash = match validate_auth(&req, &slug, &state) {
        Ok(h) => h,
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Forge\"")],
                "Unauthorized",
            )
                .into_response());
        }
    };

    let mut body = String::new();
    body.push_str(&pkt_line("# service=git-upload-pack\n"));
    body.push_str("0000");

    let capabilities = "symref=HEAD:refs/heads/main agent=forge/0.1.0";
    let first_line = format!("{} HEAD\0{}\n", head_hash, capabilities);
    body.push_str(&pkt_line(&first_line));

    let second_line = format!("{} refs/heads/main\n", head_hash);
    body.push_str(&pkt_line(&second_line));
    body.push_str("0000");

    let headers = [
        (
            header::CONTENT_TYPE,
            "application/x-git-upload-pack-advertisement".to_string(),
        ),
        (header::CACHE_CONTROL, "no-cache".to_string()),
    ];

    Ok((headers, body).into_response())
}

pub async fn upload_pack(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    let _head_hash = match validate_auth(&req, &slug, &state) {
        Ok(h) => h,
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Forge\"")],
                "Unauthorized",
            )
                .into_response());
        }
    };

    let session = state.sessions.get(&slug).ok_or(AppError::NotFound)?;
    let pack = session.repo_pack.clone();

    // We invalidate the session immediately after first successful clone
    drop(session);
    state.sessions.remove(&slug);

    // Build the upload-pack response
    let mut resp_body = Vec::new();
    resp_body.extend_from_slice(b"0008NAK\n");
    resp_body.extend_from_slice(&pack);

    let headers = [
        (
            header::CONTENT_TYPE,
            "application/x-git-upload-pack-result".to_string(),
        ),
        (header::CACHE_CONTROL, "no-cache".to_string()),
    ];

    Ok((headers, Body::from(resp_body)).into_response())
}
