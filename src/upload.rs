use axum::{
    extract::{Multipart, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::json;
use std::{
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::AppError;
use crate::git::repo_builder::RepoBuilder;
use crate::inference::get_backend;
use crate::language::Language;
use crate::prompt::build_prompt;
use crate::session::{Session, SessionStatus};
use crate::state::AppState;

fn generate_slug() -> String {
    let adjs = ["ember", "frost", "neon", "cyan", "void", "shadow"];
    let nouns = ["raven", "fox", "wolf", "hawk", "bear", "snake"];
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let adj = adjs[(nanos as usize) % adjs.len()];
    let noun = nouns[((nanos / 10) as usize) % nouns.len()];
    let num = 10 + (nanos % 90);
    format!("{}-{}-{}", adj, noun, num)
}

async fn parse_multipart(
    mut multipart: Multipart,
) -> Result<(Vec<(String, Vec<u8>)>, Option<String>), AppError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut model: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("Invalid multipart".into()))?
    {
        match field.name() {
            Some("model") => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Failed to read model field".into()))?;
                model = Some(String::from_utf8_lossy(&bytes).trim().to_string());
            }
            Some("file") => {
                let name = field
                    .file_name()
                    .ok_or_else(|| AppError::BadRequest("file field missing filename".into()))?
                    .to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Failed to read file".into()))?;
                files.push((name, data.to_vec()));
            }
            Some("archive") => {
                let name = field
                    .file_name()
                    .unwrap_or("archive")
                    .to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Failed to read archive".into()))?;

                if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
                    files.extend(extract_tar_gz(&data)?);
                } else if name.ends_with(".zip") {
                    files.extend(extract_zip(&data)?);
                } else {
                    return Err(AppError::BadRequest(
                        "archive must be .tar.gz or .zip".into(),
                    ));
                }
            }
            _ => {}
        }
    }

    if files.is_empty() {
        return Err(AppError::BadRequest("No files provided".into()));
    }

    Ok((files, model))
}

fn extract_tar_gz(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);
    let mut files = Vec::new();

    for entry in archive.entries().map_err(|e| AppError::Internal(e.into()))? {
        let mut entry = entry.map_err(|e| AppError::Internal(e.into()))?;
        if entry.header().entry_type().is_file() {
            let path = entry
                .path()
                .map_err(|e| AppError::Internal(e.into()))?
                .to_string_lossy()
                .to_string();
            let filename = std::path::Path::new(&path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).map_err(|e| AppError::Internal(e.into()))?;
            if !filename.is_empty() {
                files.push((filename, content));
            }
        }
    }
    Ok(files)
}

fn extract_zip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let cursor = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("zip error: {}", e)))?;
    let mut files = Vec::new();

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("zip entry: {}", e)))?;
        if entry.is_file() {
            let filename = std::path::Path::new(entry.name())
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).map_err(|e| AppError::Internal(e.into()))?;
            if !filename.is_empty() {
                files.push((filename, content));
            }
        }
    }
    Ok(files)
}

pub async fn upload_handler(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let (files, model) = parse_multipart(multipart).await?;

    let slug = generate_slug();
    let token = Uuid::new_v4().to_string().replace("-", "");
    let (token_tx, _) = broadcast::channel(512);

    let session = Session::new(slug.clone(), token.clone(), token_tx.clone());
    let status_arc = session.status.clone();
    state.sessions.insert(slug.clone(), session);

    let state_clone = state.clone();
    tokio::spawn(async move {
        {
            let mut s = status_arc.write().await;
            *s = SessionStatus::Processing;
        }

        let backend = get_backend(&state_clone.config);
        let mut completions: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();

        for (filename, original) in &files {
            let lang = Language::from_filename(filename);
            let text = String::from_utf8_lossy(original).to_string();
            let prompt = build_prompt(lang, &text);

            match backend
                .generate_stream(&prompt, model.as_deref(), token_tx.clone())
                .await
            {
                Ok(completed) => {
                    completions.push((
                        filename.clone(),
                        original.clone(),
                        completed.into_bytes(),
                    ));
                }
                Err(e) => {
                    let mut s = status_arc.write().await;
                    *s = SessionStatus::Failed(e.to_string());
                    let _ = token_tx.send("[DONE]".into());
                    return;
                }
            }
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut builder = RepoBuilder::new();

        let orig_blobs: Vec<(String, String)> = completions
            .iter()
            .map(|(name, original, _)| (name.clone(), builder.create_blob(original)))
            .collect();
        let orig_file_refs: Vec<(&str, &str)> = orig_blobs
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let tree1 = builder.create_multi_tree(&orig_file_refs);
        let commit1 = builder.create_commit(
            &tree1,
            None,
            "Upload <upload@forge.local>",
            now,
            "Original skeleton file",
        );

        let comp_blobs: Vec<(String, String)> = completions
            .iter()
            .map(|(name, _, completed)| (name.clone(), builder.create_blob(completed)))
            .collect();
        let comp_file_refs: Vec<(&str, &str)> = comp_blobs
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let tree2 = builder.create_multi_tree(&comp_file_refs);
        let commit2 = builder.create_commit(
            &tree2,
            Some(&commit1),
            "Forge <ai@forge.local>",
            now + 1,
            "AI completion",
        );

        let pack = builder.build_pack();

        {
            let mut s = status_arc.write().await;
            *s = SessionStatus::Done {
                repo_pack: pack,
                head_hash: commit2,
                completions,
            };
        }
        let _ = token_tx.send("[DONE]".into());
    });

    let git_url = format!("{}/git/{}", state.config.external_url, slug);
    let clone_example = format!(
        "git clone {}://user:{}@{}/git/{}",
        if state.config.external_url.starts_with("https") { "https" } else { "http" },
        token,
        state.config.external_url
            .trim_start_matches("http://")
            .trim_start_matches("https://"),
        slug
    );

    Ok(Json(json!({
        "slug": slug,
        "status": "pending",
        "expires_at": (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        "git_url": git_url,
        "clone_example": clone_example,
        "status_url": format!("{}/forge/{}/status", state.config.external_url, slug),
        "stream_url": format!("{}/forge/{}/stream", state.config.external_url, slug),
        "diff_url": format!("{}/forge/{}/diff", state.config.external_url, slug),
    })))
}
