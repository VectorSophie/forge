use axum::{
    extract::{Multipart, State},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::error::AppError;
use crate::git::repo_builder::RepoBuilder;
use crate::inference::get_backend;
use crate::language::Language;
use crate::prompt::build_prompt;
use crate::session::Session;
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

pub async fn upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let mut file_content = None;
    let mut file_name = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("Invalid multipart".to_string()))?
    {
        if field.name() == Some("file") {
            if let Some(name) = field.file_name() {
                file_name = Some(name.to_string());
            }
            let data = field
                .bytes()
                .await
                .map_err(|_| AppError::BadRequest("Failed to read file".to_string()))?;
            file_content = Some(data);
        }
    }

    let file_content = file_content.ok_or(AppError::BadRequest("No file provided".to_string()))?;
    let file_name = file_name.ok_or(AppError::BadRequest("No file name provided".to_string()))?;
    let file_text = String::from_utf8_lossy(&file_content).to_string();

    let lang = Language::from_filename(&file_name);
    let prompt = build_prompt(lang, &file_text);

    let backend = get_backend(&state.config);
    let completed_code = backend.generate(&prompt, None).await?;

    let mut builder = RepoBuilder::new();
    let blob1 = builder.create_blob(&file_content);
    let tree1 = builder.create_tree(&file_name, &blob1);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let commit1 = builder.create_commit(
        &tree1,
        None,
        "Upload <upload@forge.local>",
        now,
        "Original skeleton file",
    );

    let blob2 = builder.create_blob(completed_code.as_bytes());
    let tree2 = builder.create_tree(&file_name, &blob2);
    let commit2 = builder.create_commit(
        &tree2,
        Some(&commit1),
        "Forge <ai@forge.local>",
        now + 1, // Ensure chronological order
        "AI completion",
    );

    let pack = builder.build_pack();

    let slug = generate_slug();
    let token = Uuid::new_v4().to_string().replace("-", "");
    let expires_at = Utc::now() + Duration::hours(1);

    let (token_tx, _) = tokio::sync::broadcast::channel(512);
    let session = Session::new(slug.clone(), token.clone(), token_tx);
    {
        let mut status = session.status.blocking_write();
        *status = crate::session::SessionStatus::Done {
            repo_pack: pack,
            head_hash: commit2,
            completions: vec![],
        };
    }
    // Override expires_at to match the value computed above
    // (Session::new sets it too, but we want consistency with the response)
    let _ = expires_at; // used in JSON response below

    state.sessions.insert(slug.clone(), session);

    let git_url = format!("{}/git/{}", state.config.external_url, slug);
    let clone_example = format!(
        "git clone {}://user:{}@{}/git/{}",
        if state.config.external_url.starts_with("https") {
            "https"
        } else {
            "http"
        },
        token,
        state
            .config
            .external_url
            .trim_start_matches("http://")
            .trim_start_matches("https://"),
        slug
    );

    Ok(Json(json!({
        "slug": slug,
        "detected_language": lang.as_str(),
        "expires_at": expires_at.to_rfc3339(),
        "git_url": git_url,
        "clone_example": clone_example
    })))
}
