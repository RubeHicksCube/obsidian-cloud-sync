use axum::{
    extract::{Multipart, Path, Query, State},
    http::header,
    response::IntoResponse,
    Json,
};
use base64::Engine as _;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::tokens::Claims;
use crate::auth::AppState;
use crate::errors::AppError;
use crate::files::storage::BlobStorage;
use crate::sync::engine::compute_delta;
use crate::sync::models::*;

/// Validates and normalizes a vault-relative file path.
fn validate_file_path(path: &str) -> Result<String, AppError> {
    if path.is_empty() {
        return Err(AppError::BadRequest("File path cannot be empty".into()));
    }
    if path.len() > 1024 {
        return Err(AppError::BadRequest(
            "File path too long (max 1024 chars)".into(),
        ));
    }
    if path.bytes().any(|b| b == 0 || (b < 0x20 && b != b'\t')) {
        return Err(AppError::BadRequest(
            "File path contains invalid characters".into(),
        ));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(AppError::BadRequest("File path must be relative".into()));
    }
    for component in path.split(['/', '\\']) {
        if component == ".." || component == "." {
            return Err(AppError::BadRequest(
                "File path contains traversal sequences".into(),
            ));
        }
    }
    Ok(path.to_string())
}

/// Sanitize a filename for safe use in Content-Disposition headers.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .collect::<String>()
}

/// Check storage quota for user. Returns error if quota exceeded.
async fn check_storage_quota(state: &AppState, user_id: &str) -> Result<(), AppError> {
    let max_bytes = state.config.max_storage_per_user_mb as i64 * 1024 * 1024;
    let used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(size), 0) FROM files WHERE user_id = ? AND is_deleted = FALSE",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if used >= max_bytes {
        return Err(AppError::PayloadTooLarge(format!(
            "Storage quota exceeded ({} MB limit)",
            state.config.max_storage_per_user_mb
        )));
    }
    Ok(())
}

/// Heuristic check for plaintext data when encryption is required.
fn check_encryption_enforcement(data: &[u8], require_encryption: bool) -> Result<(), AppError> {
    if !require_encryption || data.len() < 3 {
        return Ok(());
    }
    // Check for common plaintext markers
    let plaintext_markers: &[&[u8]] = &[
        b"\xef\xbb\xbf", // UTF-8 BOM
        b"---",           // YAML front matter
        b"# ",            // Markdown heading
        b"## ",
        b"### ",
        b"{\n",  // JSON
        b"{\r",  // JSON
        b"{\t",  // JSON
        b"{ ",   // JSON
        b"<!",   // HTML
        b"<?",   // PHP/XML
    ];
    for marker in plaintext_markers {
        if data.starts_with(marker) {
            return Err(AppError::BadRequest(
                "Encryption is required. The uploaded data appears to be unencrypted.".into(),
            ));
        }
    }
    Ok(())
}

pub async fn delta(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<DeltaRequest>,
) -> Result<Json<DeltaResponse>, AppError> {
    let instructions =
        compute_delta(&state.db, &claims.sub, &req.files, &req.deleted_paths).await?;
    let server_time = Utc::now().timestamp();

    Ok(Json(DeltaResponse {
        instructions,
        server_time,
    }))
}

/// JSON body for the upload endpoint.
#[derive(Deserialize)]
pub struct UploadBody {
    /// Vault-relative file path.
    pub path: String,
    /// Optional plaintext SHA-256 hash (hex). Used for delta comparison
    /// when encryption is enabled (blob hash ≠ plaintext hash).
    pub hash: Option<String>,
    /// File contents, base64-encoded. Encoding avoids proxy WAF rules
    /// that inspect or block binary (application/octet-stream) bodies.
    pub data: String,
}

/// Upload a file as base64-encoded data inside a JSON body.
/// Using JSON bypasses Cloudflare WAF rules that block/modify
/// application/octet-stream or multipart POST bodies.
pub async fn upload(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(body): Json<UploadBody>,
) -> Result<Json<UploadResponse>, AppError> {
    check_storage_quota(&state, &claims.sub).await?;

    let file_path = validate_file_path(&body.path)?;
    let file_data = base64::engine::general_purpose::STANDARD
        .decode(&body.data)
        .map_err(|e| AppError::BadRequest(format!("Invalid base64 data: {e}")))?;

    if file_data.is_empty() {
        return Err(AppError::BadRequest("Empty file data".into()));
    }

    check_encryption_enforcement(&file_data, state.config.require_encryption)?;

    let storage = BlobStorage::new(&state.config.data_dir);
    let size = file_data.len() as i64;
    let (_blob_hash, blob_path) = storage.store(&claims.sub, &file_data).await?;
    // Use the client-provided plaintext hash for delta comparison.
    // Critical when encryption is enabled: the blob hash covers encrypted
    // bytes, but the client manifest hashes plaintext content.
    let hash = body.hash.unwrap_or(_blob_hash);
    let blob_path_str = blob_path.to_string_lossy().to_string();
    let now = Utc::now().timestamp();

    let (file_id, version) = upsert_file_record(
        &state.db,
        &claims.sub,
        &file_path,
        &hash,
        size,
        &blob_path_str,
        &claims.device_id,
        now,
    )
    .await?;

    state.notify_sync_update(&claims.sub, &claims.device_id, &file_path, "upload");

    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "file_upload",
        Some("file"),
        Some(&file_id),
        Some(&format!("path={}", file_path)),
        None,
    )
    .await;

    Ok(Json(UploadResponse { file_id, version }))
}

/// Legacy multipart upload — kept for backward compatibility.
pub async fn upload_multipart(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    check_storage_quota(&state, &claims.sub).await?;

    let storage = BlobStorage::new(&state.config.data_dir);
    let mut file_path: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut plaintext_hash: Option<String> = None;

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
            "hash" => {
                plaintext_hash = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("Invalid hash field: {e}")))?,
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

    let file_path =
        file_path.ok_or_else(|| AppError::BadRequest("Missing 'path' field".into()))?;
    let file_path = validate_file_path(&file_path)?;
    let file_data =
        file_data.ok_or_else(|| AppError::BadRequest("Missing 'file' field".into()))?;

    check_encryption_enforcement(&file_data, state.config.require_encryption)?;

    let size = file_data.len() as i64;
    let (_blob_hash, blob_path) = storage.store(&claims.sub, &file_data).await?;
    let hash = plaintext_hash.unwrap_or(_blob_hash);
    let blob_path_str = blob_path.to_string_lossy().to_string();
    let now = Utc::now().timestamp();

    let (file_id, version) = upsert_file_record(
        &state.db,
        &claims.sub,
        &file_path,
        &hash,
        size,
        &blob_path_str,
        &claims.device_id,
        now,
    )
    .await?;

    state.notify_sync_update(&claims.sub, &claims.device_id, &file_path, "upload");

    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "file_upload",
        Some("file"),
        Some(&file_id),
        Some(&format!("path={}", file_path)),
        None,
    )
    .await;

    Ok(Json(UploadResponse { file_id, version }))
}

/// Upsert file record within a transaction to prevent race conditions.
/// Only creates a new version when the content hash actually changes.
#[allow(clippy::too_many_arguments)]
async fn upsert_file_record(
    db: &sqlx::SqlitePool,
    user_id: &str,
    file_path: &str,
    hash: &str,
    size: i64,
    blob_path_str: &str,
    device_id: &str,
    now: i64,
) -> Result<(String, i64), AppError> {
    let mut tx = db.begin().await?;

    let existing: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT id, current_version, hash FROM files WHERE user_id = ? AND path = ?",
    )
    .bind(user_id)
    .bind(file_path)
    .fetch_optional(&mut *tx)
    .await?;

    let (file_id, version) = match existing {
        Some((id, current_version, existing_hash)) => {
            if existing_hash == hash {
                // Content unchanged — no new version, no timestamp update.
                // Just ensure it's not marked deleted.
                sqlx::query("UPDATE files SET is_deleted = FALSE WHERE id = ? AND is_deleted = TRUE")
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                return Ok((id, current_version));
            }

            // Content actually changed — new version
            let new_version = current_version + 1;
            sqlx::query(
                "UPDATE files SET hash = ?, size = ?, current_version = ?, is_deleted = FALSE, updated_at = ? WHERE id = ?",
            )
            .bind(hash)
            .bind(size)
            .bind(new_version)
            .bind(now)
            .bind(&id)
            .execute(&mut *tx)
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
            .bind(user_id)
            .bind(file_path)
            .bind(hash)
            .bind(size)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            (id, 1)
        }
    };

    // Only create a version record when content changed (new file or new hash)
    sqlx::query(
        "INSERT INTO file_versions (id, file_id, version, hash, size, blob_path, device_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&file_id)
    .bind(version)
    .bind(hash)
    .bind(size)
    .bind(blob_path_str)
    .bind(device_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((file_id, version))
}

pub async fn upload_batch(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    mut multipart: Multipart,
) -> Result<Json<Vec<UploadResponse>>, AppError> {
    // Check storage quota
    check_storage_quota(&state, &claims.sub).await?;

    let storage = BlobStorage::new(&state.config.data_dir);
    let mut results = Vec::new();
    let now = Utc::now().timestamp();

    // Parse all fields: expect pairs of path_N, hash_N, and file_N
    let mut paths: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut hashes: std::collections::HashMap<String, String> = std::collections::HashMap::new();
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
        } else if let Some(idx) = name.strip_prefix("hash_") {
            let text = field
                .text()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            hashes.insert(idx.to_string(), text);
        } else if let Some(idx) = name.strip_prefix("file_") {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::BadRequest(e.to_string()))?;
            datas.insert(idx.to_string(), bytes.to_vec());
        }
    }

    if paths.len() > 100 {
        return Err(AppError::BadRequest(
            "Batch upload limited to 100 files".into(),
        ));
    }

    for (idx, file_path) in &paths {
        let file_data = match datas.get(idx) {
            Some(d) => d,
            None => continue,
        };

        let file_path = &validate_file_path(file_path)?;
        check_encryption_enforcement(file_data, state.config.require_encryption)?;

        let size = file_data.len() as i64;
        let (_blob_hash, blob_path) = storage.store(&claims.sub, file_data).await?;
        let hash = hashes.get(idx).cloned().unwrap_or(_blob_hash);
        let blob_path_str = blob_path.to_string_lossy().to_string();

        let (file_id, version) = upsert_file_record(
            &state.db,
            &claims.sub,
            file_path,
            &hash,
            size,
            &blob_path_str,
            &claims.device_id,
            now,
        )
        .await?;

        state.notify_sync_update(&claims.sub, &claims.device_id, file_path, "upload");

        results.push(UploadResponse { file_id, version });
    }

    Ok(Json(results))
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    pub version: Option<i64>,
}

pub async fn download(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(file_id): Path<String>,
    Query(query): Query<DownloadQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Verify file belongs to user and get path
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT hash, path FROM files WHERE id = ? AND user_id = ?")
            .bind(&file_id)
            .bind(&claims.sub)
            .fetch_optional(&state.db)
            .await?;

    let (current_hash, file_path) =
        row.ok_or_else(|| AppError::NotFound("File not found".into()))?;

    let storage = BlobStorage::new(&state.config.data_dir);

    // If a specific version is requested, look up its hash
    let hash = if let Some(ver) = query.version {
        let ver_hash: Option<(String,)> = sqlx::query_as(
            "SELECT hash FROM file_versions WHERE file_id = ? AND version = ?",
        )
        .bind(&file_id)
        .bind(ver)
        .fetch_optional(&state.db)
        .await?;
        let (h,) = ver_hash.ok_or_else(|| AppError::NotFound("Version not found".into()))?;
        h
    } else {
        current_hash
    };

    let data = storage.read(&claims.sub, &hash).await?;

    let raw_filename = file_path.rsplit('/').next().unwrap_or("file");
    let filename = sanitize_filename(raw_filename);
    let filename = if filename.is_empty() {
        "file".to_string()
    } else {
        filename
    };
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

/// Mark a file as deleted on the server (soft delete).
pub async fn delete_file(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(file_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let now = Utc::now().timestamp();

    let result = sqlx::query(
        "UPDATE files SET is_deleted = TRUE, updated_at = ? WHERE id = ? AND user_id = ?",
    )
    .bind(now)
    .bind(&file_id)
    .bind(&claims.sub)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("File not found".into()));
    }

    // Notify other devices
    let path: Option<(String,)> =
        sqlx::query_as("SELECT path FROM files WHERE id = ?")
            .bind(&file_id)
            .fetch_optional(&state.db)
            .await?;
    if let Some((file_path,)) = path {
        state.notify_sync_update(&claims.sub, &claims.device_id, &file_path, "delete");
    }

    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "file_delete",
        Some("file"),
        Some(&file_id),
        None,
        None,
    )
    .await;

    Ok(Json(serde_json::json!({ "message": "File deleted" })))
}

pub async fn complete(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(_req): Json<CompleteRequest>,
) -> Result<Json<CompleteResponse>, AppError> {
    let now = Utc::now().timestamp();

    // Get latest server version
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

/// Correct a stale hash in the files table without re-uploading file data.
/// Used when the client detects that downloaded content is identical to its
/// local copy — the server's stored hash was wrong (e.g. encrypted blob hash
/// instead of plaintext hash).
pub async fn fix_hash(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<FixHashRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        "UPDATE files SET hash = ? WHERE id = ? AND user_id = ?",
    )
    .bind(&req.hash)
    .bind(&req.file_id)
    .bind(&claims.sub)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("File not found".into()));
    }

    Ok(Json(serde_json::json!({ "message": "Hash updated" })))
}
