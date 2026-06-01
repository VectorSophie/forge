use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;

use crate::error::AppError;
use crate::session::SessionStatus;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct InfoRefsQuery {
    pub service: Option<String>,
}

fn pkt_line(s: &str) -> String {
    let len = s.len() + 4;
    format!("{:04x}{}", len, s)
}

/// Extract the raw Authorization header value from a request as an owned String.
fn extract_auth_header(req: &Request) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_owned())
}

async fn validate_auth(auth_header: &str, slug: &str, state: &AppState) -> Result<String, AppError> {
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
    let password = parts[1].to_owned();

    // Extract what we need from the DashMap guard before any await points,
    // since DashMap Ref guards are not Send.
    let (token, expires_at, status_arc) = {
        let session = state.sessions.get(slug).ok_or(AppError::NotFound)?;
        (session.token.clone(), session.expires_at, session.status.clone())
    };

    if token != password {
        return Err(AppError::Unauthorized);
    }
    if chrono::Utc::now() > expires_at {
        state.sessions.remove(slug);
        return Err(AppError::Unauthorized);
    }

    let status = status_arc.read().await;
    match &*status {
        SessionStatus::Done { head_hash, .. } => Ok(head_hash.clone()),
        SessionStatus::Failed(msg) => {
            Err(AppError::BadRequest(format!("Job failed: {}", msg)))
        }
        _ => Err(AppError::BadRequest("Job not complete yet".into())),
    }
}

pub async fn info_refs(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<InfoRefsQuery>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    if query.service.as_deref() != Some("git-upload-pack") {
        return Err(AppError::BadRequest("Only git-upload-pack is supported".into()));
    }

    let auth_header = extract_auth_header(&req).unwrap_or_default();
    // req is no longer needed; drop it so the non-Sync body doesn't cross await points.
    drop(req);

    let head_hash = match validate_auth(&auth_header, &slug, &state).await {
        Ok(h) => h,
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Forge\"")],
                "Unauthorized",
            ).into_response());
        }
    };

    let mut body = String::new();
    body.push_str(&pkt_line("# service=git-upload-pack\n"));
    body.push_str("0000");
    let capabilities = "symref=HEAD:refs/heads/main agent=forge/0.1.0";
    body.push_str(&pkt_line(&format!("{} HEAD\0{}\n", head_hash, capabilities)));
    body.push_str(&pkt_line(&format!("{} refs/heads/main\n", head_hash)));
    body.push_str("0000");

    Ok((
        [(header::CONTENT_TYPE, "application/x-git-upload-pack-advertisement".to_string()),
         (header::CACHE_CONTROL, "no-cache".to_string())],
        body,
    ).into_response())
}

pub async fn upload_pack(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    let auth_header = extract_auth_header(&req).unwrap_or_default();
    // Drop req so the non-Sync Body doesn't cross await points.
    drop(req);

    match validate_auth(&auth_header, &slug, &state).await {
        Ok(_) => {}
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic realm=\"Forge\"")],
                "Unauthorized",
            ).into_response());
        }
    }

    let pack = {
        let status_arc = {
            let session = state.sessions.get(&slug).ok_or(AppError::NotFound)?;
            session.status.clone()
        };
        let status = status_arc.read().await;
        match &*status {
            SessionStatus::Done { repo_pack, .. } => repo_pack.clone(),
            _ => return Err(AppError::BadRequest("Job not complete yet".into())),
        }
    };

    state.sessions.remove(&slug);

    let mut resp_body = Vec::new();
    resp_body.extend_from_slice(b"0008NAK\n");
    resp_body.extend_from_slice(&pack);

    Ok((
        [(header::CONTENT_TYPE, "application/x-git-upload-pack-result".to_string()),
         (header::CACHE_CONTROL, "no-cache".to_string())],
        Body::from(resp_body),
    ).into_response())
}
