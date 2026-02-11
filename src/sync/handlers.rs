use axum::{
    extract::{Multipart, Path, State},
    http::header,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use crate::auth::tokens::Claims;
use crate::auth::AppState;
use crate::errors::AppError;
use crate::files::storage::BlobStorage;
use crate::sync::engine::compute_delta;
use crate::sync::models::*;

/// Validates and normalizes a vault-relative file path.
/// Rejects paths with traversal sequences, null bytes, control characters,
/// absolute paths, or excessively long paths.
fn validate_file_path(path: &str) -> Result<String, AppError> {
    // Reject empty paths
    if path.is_empty() {
        return Err(AppError::BadRequest("File path cannot be empty".into()));
    }
    // Reject excessively long paths
    if path.len() > 1024 {
        return Err(AppError::BadRequest("File path too long (max 1024 chars)".into()));
    }
    // Reject null bytes and control characters
    if path.bytes().any(|b| b == 0 || (b < 0x20 && b != b'\t')) {
        return Err(AppError::BadRequest("File path contains invalid characters".into()));
    }
    // Reject absolute paths
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(AppError::BadRequest("File path must be relative".into()));
    }
    // Reject path traversal
    for component in path.split(['/', '\\']) {
        if component == ".." || component == "." {
            return Err(AppError::BadRequest("File path contains traversal sequences".into()));
        }
    }
    Ok(path.to_string())
}

/// Sanitize a filename for safe use in Content-Disposition headers.
/// Removes any characters that could cause header injection.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .collect::<String>()
}

pub async fn delta(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<DeltaRequest>,
) -> Result<Json<DeltaResponse>, AppError> {
    let instructions = compute_delta(&state.db, &claims.sub, &req.files).await?;
    let server_time = Utc::now().timestamp();

    Ok(Json(DeltaResponse {
        instructions,
        server_time,
    }))
}

pub async fn upload(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    let storage = BlobStorage::new(&state.config.data_dir);
    let mut file_path: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "path" => {
                file_path = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("Invalid path field: {e}")))?,
                );
            }
            "file" => {
                file_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("Invalid file field: {e}")))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let file_path = file_path.ok_or_else(|| AppError::BadRequest("Missing 'path' field".into()))?;
    let file_path = validate_file_path(&file_path)?;
    let file_data = file_data.ok_or_else(|| AppError::BadRequest("Missing 'file' field".into()))?;

    let size = file_data.len() as i64;
    let (hash, blob_path) = storage.store(&claims.sub, &file_data).await?;
    let blob_path_str = blob_path.to_string_lossy().to_string();
    let now = Utc::now().timestamp();

    // Upsert the file record
    let existing: Option<(String, i64)> = sqlx::query_as(
        "SELECT id, current_version FROM files WHERE user_id = ? AND path = ?",
    )
    .bind(&claims.sub)
    .bind(&file_path)
    .fetch_optional(&state.db)
    .await?;

    let (file_id, version) = match existing {
        Some((id, current_version)) => {
            let new_version = current_version + 1;
            sqlx::query(
                "UPDATE files SET hash = ?, size = ?, current_version = ?, is_deleted = FALSE, updated_at = ? WHERE id = ?",
            )
            .bind(&hash)
            .bind(size)
            .bind(new_version)
            .bind(now)
            .bind(&id)
            .execute(&state.db)
            .await?;
            (id, new_version)
        }
        None => {
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO files (id, user_id, path, current_version, hash, size, is_deleted, created_at, updated_at) \
                 VALUES (?, ?, ?, 1, ?, ?, FALSE, ?, ?)",
            )
            .bind(&id)
            .bind(&claims.sub)
            .bind(&file_path)
            .bind(&hash)
            .bind(size)
            .bind(now)
            .bind(now)
            .execute(&state.db)
            .await?;
            (id, 1)
        }
    };

    // Create version record
    sqlx::query(
        "INSERT INTO file_versions (id, file_id, version, hash, size, blob_path, device_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&file_id)
    .bind(version)
    .bind(&hash)
    .bind(size)
    .bind(&blob_path_str)
    .bind(&claims.device_id)
    .bind(now)
    .execute(&state.db)
    .await?;

    Ok(Json(UploadResponse { file_id, version }))
}

pub async fn upload_batch(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    mut multipart: Multipart,
) -> Result<Json<Vec<UploadResponse>>, AppError> {
    let storage = BlobStorage::new(&state.config.data_dir);
    let mut results = Vec::new();
    let now = Utc::now().timestamp();

    // Parse all fields: expect pairs of path_N and file_N
    let mut paths: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut datas: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if let Some(idx) = name.strip_prefix("path_") {
            let text = field
                .text()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            paths.insert(idx.to_string(), text);
        } else if let Some(idx) = name.strip_prefix("file_") {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            datas.insert(idx.to_string(), bytes.to_vec());
        }
    }

    // Limit batch size to prevent excessive DB operations
    if paths.len() > 100 {
        return Err(AppError::BadRequest("Batch upload limited to 100 files".into()));
    }

    for (idx, file_path) in &paths {
        let file_data = match datas.get(idx) {
            Some(d) => d,
            None => continue,
        };

        let file_path = &validate_file_path(file_path)?;
        let size = file_data.len() as i64;
        let (hash, blob_path) = storage.store(&claims.sub, file_data).await?;
        let blob_path_str = blob_path.to_string_lossy().to_string();

        let existing: Option<(String, i64)> = sqlx::query_as(
            "SELECT id, current_version FROM files WHERE user_id = ? AND path = ?",
        )
        .bind(&claims.sub)
        .bind(file_path)
        .fetch_optional(&state.db)
        .await?;

        let (file_id, version) = match existing {
            Some((id, current_version)) => {
                let new_version = current_version + 1;
                sqlx::query(
                    "UPDATE files SET hash = ?, size = ?, current_version = ?, is_deleted = FALSE, updated_at = ? WHERE id = ?",
                )
                .bind(&hash)
                .bind(size)
                .bind(new_version)
                .bind(now)
                .bind(&id)
                .execute(&state.db)
                .await?;
                (id, new_version)
            }
            None => {
                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO files (id, user_id, path, current_version, hash, size, is_deleted, created_at, updated_at) \
                     VALUES (?, ?, ?, 1, ?, ?, FALSE, ?, ?)",
                )
                .bind(&id)
                .bind(&claims.sub)
                .bind(file_path)
                .bind(&hash)
                .bind(size)
                .bind(now)
                .bind(now)
                .execute(&state.db)
                .await?;
                (id, 1)
            }
        };

        sqlx::query(
            "INSERT INTO file_versions (id, file_id, version, hash, size, blob_path, device_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&file_id)
        .bind(version)
        .bind(&hash)
        .bind(size)
        .bind(&blob_path_str)
        .bind(&claims.device_id)
        .bind(now)
        .execute(&state.db)
        .await?;

        results.push(UploadResponse { file_id, version });
    }

    Ok(Json(results))
}

pub async fn download(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(file_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT hash, path FROM files WHERE id = ? AND user_id = ?",
    )
    .bind(&file_id)
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await?;

    let (hash, file_path) =
        row.ok_or_else(|| AppError::NotFound("File not found".into()))?;

    let storage = BlobStorage::new(&state.config.data_dir);
    let data = storage.read(&claims.sub, &hash).await?;

    let raw_filename = file_path.rsplit('/').next().unwrap_or("file");
    let filename = sanitize_filename(raw_filename);
    let filename = if filename.is_empty() { "file".to_string() } else { filename };
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/octet-stream".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        data,
    ))
}

pub async fn complete(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(_req): Json<CompleteRequest>,
) -> Result<Json<CompleteResponse>, AppError> {
    let now = Utc::now().timestamp();

    // Get latest server version (max of all file updated_at timestamps)
    let server_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(updated_at), 0) FROM files WHERE user_id = ?",
    )
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await?;

    // Upsert sync cursor
    sqlx::query(
        "INSERT INTO sync_cursors (user_id, device_id, last_sync_at, server_version) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(user_id, device_id) DO UPDATE SET last_sync_at = ?, server_version = ?",
    )
    .bind(&claims.sub)
    .bind(&claims.device_id)
    .bind(now)
    .bind(server_version)
    .bind(now)
    .bind(server_version)
    .execute(&state.db)
    .await?;

    Ok(Json(CompleteResponse {
        message: "Sync complete".into(),
        server_version,
    }))
}
