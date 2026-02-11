use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::password::hash_password;
use crate::auth::tokens::Claims;
use crate::auth::AppState;
use crate::errors::AppError;

#[derive(Serialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub is_admin: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub file_count: i64,
    pub device_count: i64,
}

pub async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<UserInfo>>, AppError> {
    let users: Vec<(String, String, Option<String>, bool, i64, i64)> = sqlx::query_as(
        "SELECT id, username, email, is_admin, created_at, updated_at FROM users ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::new();
    for (id, username, email, is_admin, created_at, updated_at) in users {
        let file_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM files WHERE user_id = ? AND is_deleted = FALSE",
        )
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

        let device_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM devices WHERE user_id = ? AND revoked = FALSE",
        )
        .bind(&id)
        .fetch_one(&state.db)
        .await?;

        result.push(UserInfo {
            id,
            username,
            email,
            is_admin,
            created_at,
            updated_at,
            file_count,
            device_count,
        });
    }

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub is_admin: Option<bool>,
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<UserInfo>, AppError> {
    // Validate username: 3-64 chars, alphanumeric plus underscore/hyphen
    if req.username.len() < 3 {
        return Err(AppError::BadRequest("Username must be at least 3 characters".into()));
    }
    if req.username.len() > 64 {
        return Err(AppError::BadRequest("Username must be at most 64 characters".into()));
    }
    if !req.username.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err(AppError::BadRequest(
            "Username may only contain letters, numbers, underscores, and hyphens".into(),
        ));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest("Password must be at least 8 characters".into()));
    }
    if req.password.len() > 256 {
        return Err(AppError::BadRequest("Password must be at most 256 characters".into()));
    }

    let exists: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM users WHERE username = ?",
    )
    .bind(&req.username)
    .fetch_one(&state.db)
    .await?;

    if exists {
        return Err(AppError::Conflict("Username already taken".into()));
    }

    let id = Uuid::new_v4().to_string();
    let password_hash = hash_password(&req.password)?;
    let now = Utc::now().timestamp();
    let is_admin = req.is_admin.unwrap_or(false);

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, is_admin, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(is_admin)
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await?;

    Ok(Json(UserInfo {
        id,
        username: req.username,
        email: req.email,
        is_admin,
        created_at: now,
        updated_at: now,
        file_count: 0,
        device_count: 0,
    }))
}

pub async fn delete_user(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Prevent admin from deleting themselves
    if user_id == claims.sub {
        return Err(AppError::BadRequest("Cannot delete your own account".into()));
    }

    let target_user: Option<(String, bool)> =
        sqlx::query_as("SELECT id, is_admin FROM users WHERE id = ?")
            .bind(&user_id)
            .fetch_optional(&state.db)
            .await?;

    let (_id, target_is_admin) =
        target_user.ok_or_else(|| AppError::NotFound("User not found".into()))?;

    // If target is an admin, check this won't leave the system with no admins
    if target_is_admin {
        let admin_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin = TRUE")
                .fetch_one(&state.db)
                .await?;
        if admin_count <= 1 {
            return Err(AppError::BadRequest(
                "Cannot delete the last admin account".into(),
            ));
        }
    }

    // CASCADE will handle devices, refresh_tokens, files, file_versions, sync_cursors
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&user_id)
        .execute(&state.db)
        .await?;

    // TODO: Clean up blob storage for the deleted user

    Ok(Json(
        serde_json::json!({"message": "User deleted successfully"}),
    ))
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub settings: std::collections::HashMap<String, String>,
}

pub async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, AppError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM server_settings")
            .fetch_all(&state.db)
            .await?;

    let settings: std::collections::HashMap<String, String> =
        rows.into_iter().collect();

    Ok(Json(SettingsResponse { settings }))
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub settings: std::collections::HashMap<String, String>,
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, AppError> {
    for (key, value) in &req.settings {
        sqlx::query(
            "INSERT INTO server_settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = ?",
        )
        .bind(key)
        .bind(value)
        .bind(value)
        .execute(&state.db)
        .await?;
    }

    get_settings(State(state)).await
}
