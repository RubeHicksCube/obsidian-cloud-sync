use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::tokens::Claims;
use crate::auth::AppState;
use crate::errors::AppError;

type FileRow = (String, String, i64, String, i64, bool, i64, i64);

#[derive(Serialize)]
pub struct FileInfo {
    pub id: String,
    pub path: String,
    pub current_version: i64,
    pub hash: String,
    pub size: i64,
    pub is_deleted: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
pub struct VersionInfo {
    pub id: String,
    pub version: i64,
    pub hash: String,
    pub size: i64,
    pub device_id: Option<String>,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub include_deleted: Option<bool>,
}

pub async fn list_files(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<FileInfo>>, AppError> {
    let include_deleted = query.include_deleted.unwrap_or(false);

    let files: Vec<FileRow> = if include_deleted {
        sqlx::query_as(
            "SELECT id, path, current_version, hash, size, is_deleted, created_at, updated_at \
             FROM files WHERE user_id = ? ORDER BY path",
        )
        .bind(&claims.sub)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, path, current_version, hash, size, is_deleted, created_at, updated_at \
             FROM files WHERE user_id = ? AND is_deleted = FALSE ORDER BY path",
        )
        .bind(&claims.sub)
        .fetch_all(&state.db)
        .await?
    };

    let result: Vec<FileInfo> = files
        .into_iter()
        .map(
            |(id, path, current_version, hash, size, is_deleted, created_at, updated_at)| {
                FileInfo {
                    id,
                    path,
                    current_version,
                    hash,
                    size,
                    is_deleted,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect();

    Ok(Json(result))
}

pub async fn file_versions(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(file_id): Path<String>,
) -> Result<Json<Vec<VersionInfo>>, AppError> {
    // Verify file belongs to user
    let exists: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM files WHERE id = ? AND user_id = ?",
    )
    .bind(&file_id)
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await?;

    if !exists {
        return Err(AppError::NotFound("File not found".into()));
    }

    let versions: Vec<(String, i64, String, i64, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, version, hash, size, device_id, created_at \
         FROM file_versions WHERE file_id = ? ORDER BY version DESC",
    )
    .bind(&file_id)
    .fetch_all(&state.db)
    .await?;

    let result: Vec<VersionInfo> = versions
        .into_iter()
        .map(|(id, version, hash, size, device_id, created_at)| VersionInfo {
            id,
            version,
            hash,
            size,
            device_id,
            created_at,
        })
        .collect();

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct RollbackRequest {
    pub version: i64,
}

pub async fn rollback(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(file_id): Path<String>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<FileInfo>, AppError> {
    // Verify file belongs to user
    let file: Option<(String, i64)> = sqlx::query_as(
        "SELECT id, current_version FROM files WHERE id = ? AND user_id = ?",
    )
    .bind(&file_id)
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await?;

    let (_id, current_version) =
        file.ok_or_else(|| AppError::NotFound("File not found".into()))?;

    // Get the target version
    let target: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT hash, size, blob_path FROM file_versions WHERE file_id = ? AND version = ?",
    )
    .bind(&file_id)
    .bind(req.version)
    .fetch_optional(&state.db)
    .await?;

    let (hash, size, blob_path) =
        target.ok_or_else(|| AppError::NotFound("Version not found".into()))?;

    let now = Utc::now().timestamp();
    let new_version = current_version + 1;

    // Update file to point to old version's content
    sqlx::query(
        "UPDATE files SET hash = ?, size = ?, current_version = ?, is_deleted = FALSE, updated_at = ? WHERE id = ?",
    )
    .bind(&hash)
    .bind(size)
    .bind(new_version)
    .bind(now)
    .bind(&file_id)
    .execute(&state.db)
    .await?;

    // Create a new version record pointing to the same blob (no data copy)
    sqlx::query(
        "INSERT INTO file_versions (id, file_id, version, hash, size, blob_path, device_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&file_id)
    .bind(new_version)
    .bind(&hash)
    .bind(size)
    .bind(&blob_path)
    .bind(&claims.device_id)
    .bind(now)
    .execute(&state.db)
    .await?;

    // Log audit event
    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "file_rollback",
        Some("file"),
        Some(&file_id),
        Some(&format!("version={}", req.version)),
        None,
    )
    .await;

    // Return updated file info
    let updated: (String, String, i64, String, i64, bool, i64, i64) = sqlx::query_as(
        "SELECT id, path, current_version, hash, size, is_deleted, created_at, updated_at FROM files WHERE id = ?",
    )
    .bind(&file_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(FileInfo {
        id: updated.0,
        path: updated.1,
        current_version: updated.2,
        hash: updated.3,
        size: updated.4,
        is_deleted: updated.5,
        created_at: updated.6,
        updated_at: updated.7,
    }))
}

pub async fn restore(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(file_id): Path<String>,
) -> Result<Json<FileInfo>, AppError> {
    // Verify file belongs to user and is deleted
    let file: Option<(String, bool)> = sqlx::query_as(
        "SELECT id, is_deleted FROM files WHERE id = ? AND user_id = ?",
    )
    .bind(&file_id)
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await?;

    let (_id, is_deleted) =
        file.ok_or_else(|| AppError::NotFound("File not found".into()))?;

    if !is_deleted {
        return Err(AppError::BadRequest("File is not deleted".into()));
    }

    let now = Utc::now().timestamp();

    sqlx::query("UPDATE files SET is_deleted = FALSE, updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(&file_id)
        .execute(&state.db)
        .await?;

    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "file_restore",
        Some("file"),
        Some(&file_id),
        None,
        None,
    )
    .await;

    let updated: FileRow = sqlx::query_as(
        "SELECT id, path, current_version, hash, size, is_deleted, created_at, updated_at FROM files WHERE id = ?",
    )
    .bind(&file_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(FileInfo {
        id: updated.0,
        path: updated.1,
        current_version: updated.2,
        hash: updated.3,
        size: updated.4,
        is_deleted: updated.5,
        created_at: updated.6,
        updated_at: updated.7,
    }))
}

#[derive(Serialize)]
pub struct WipeResponse {
    pub message: String,
}

pub async fn wipe_archive(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
) -> Result<Json<WipeResponse>, AppError> {
    let deleted_ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM files WHERE user_id = ? AND is_deleted = TRUE",
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await?;

    let count = deleted_ids.len();

    for (file_id,) in &deleted_ids {
        sqlx::query("DELETE FROM file_versions WHERE file_id = ?")
            .bind(file_id)
            .execute(&state.db)
            .await?;

        sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(file_id)
            .execute(&state.db)
            .await?;
    }

    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "archive_wipe",
        None,
        None,
        Some(&format!("count={}", count)),
        None,
    )
    .await;

    Ok(Json(WipeResponse {
        message: format!("{} files permanently deleted", count),
    }))
}
