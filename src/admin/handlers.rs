use axum::{
    extract::{Path, Query, State},
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
    pub total_size: i64,
}

pub async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<UserInfo>>, AppError> {
    // Single aggregate query to avoid N+1
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, String, Option<String>, bool, i64, i64, i64, i64, i64)> =
        sqlx::query_as(
            "SELECT u.id, u.username, u.email, u.is_admin, u.created_at, u.updated_at, \
             COUNT(DISTINCT CASE WHEN d.revoked = FALSE THEN d.id END) as device_count, \
             COUNT(DISTINCT CASE WHEN f.is_deleted = FALSE THEN f.id END) as file_count, \
             COALESCE(SUM(CASE WHEN f.is_deleted = FALSE THEN f.size ELSE 0 END), 0) as total_size \
             FROM users u \
             LEFT JOIN devices d ON d.user_id = u.id \
             LEFT JOIN files f ON f.user_id = u.id \
             GROUP BY u.id \
             ORDER BY u.created_at",
        )
        .fetch_all(&state.db)
        .await?;

    let result: Vec<UserInfo> = rows
        .into_iter()
        .map(
            |(id, username, email, is_admin, created_at, updated_at, device_count, file_count, total_size)| {
                UserInfo {
                    id,
                    username,
                    email,
                    is_admin,
                    created_at,
                    updated_at,
                    file_count,
                    device_count,
                    total_size,
                }
            },
        )
        .collect();

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
    claims: axum::Extension<Claims>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<UserInfo>, AppError> {
    // Validate username: 3-64 chars, alphanumeric plus underscore/hyphen
    if req.username.len() < 3 {
        return Err(AppError::BadRequest(
            "Username must be at least 3 characters".into(),
        ));
    }
    if req.username.len() > 64 {
        return Err(AppError::BadRequest(
            "Username must be at most 64 characters".into(),
        ));
    }
    if !req
        .username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::BadRequest(
            "Username may only contain letters, numbers, underscores, and hyphens".into(),
        ));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".into(),
        ));
    }
    if req.password.len() > 256 {
        return Err(AppError::BadRequest(
            "Password must be at most 256 characters".into(),
        ));
    }

    let exists: bool =
        sqlx::query_scalar("SELECT COUNT(*) > 0 FROM users WHERE username = ?")
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

    // Log audit event
    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "user_create",
        Some("user"),
        Some(&id),
        Some(&format!("username={}", req.username)),
        None,
    )
    .await;

    Ok(Json(UserInfo {
        id,
        username: req.username,
        email: req.email,
        is_admin,
        created_at: now,
        updated_at: now,
        file_count: 0,
        device_count: 0,
        total_size: 0,
    }))
}

pub async fn delete_user(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Prevent admin from deleting themselves
    if user_id == claims.sub {
        return Err(AppError::BadRequest(
            "Cannot delete your own account".into(),
        ));
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

    // Log audit event
    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "user_delete",
        Some("user"),
        Some(&user_id),
        None,
        None,
    )
    .await;

    Ok(Json(
        serde_json::json!({"message": "User deleted successfully"}),
    ))
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub settings: std::collections::HashMap<String, String>,
}

pub async fn get_settings(State(state): State<AppState>) -> Result<Json<SettingsResponse>, AppError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM server_settings")
            .fetch_all(&state.db)
            .await?;

    let settings: std::collections::HashMap<String, String> = rows.into_iter().collect();

    Ok(Json(SettingsResponse { settings }))
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub settings: std::collections::HashMap<String, String>,
}

/// Allowed setting keys with their validation rules.
fn validate_setting(key: &str, value: &str) -> Result<(), AppError> {
    match key {
        "max_versions_per_file" => {
            let v: u32 = value.parse().map_err(|_| {
                AppError::BadRequest("max_versions_per_file must be a number".into())
            })?;
            if !(1..=1000).contains(&v) {
                return Err(AppError::BadRequest(
                    "max_versions_per_file must be between 1 and 1000".into(),
                ));
            }
        }
        "max_version_age_days" => {
            let v: u32 = value.parse().map_err(|_| {
                AppError::BadRequest("max_version_age_days must be a number".into())
            })?;
            if !(1..=3650).contains(&v) {
                return Err(AppError::BadRequest(
                    "max_version_age_days must be between 1 and 3650".into(),
                ));
            }
        }
        "registration_open" => {
            if value != "true" && value != "false" {
                return Err(AppError::BadRequest(
                    "registration_open must be 'true' or 'false'".into(),
                ));
            }
        }
        _ => {
            return Err(AppError::BadRequest(format!(
                "Unknown setting: {key}"
            )));
        }
    }
    Ok(())
}

pub async fn update_settings(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, AppError> {
    // Validate all settings before applying any
    for (key, value) in &req.settings {
        validate_setting(key, value)?;
    }

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

    // Log audit event
    let details = req
        .settings
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    crate::audit::log_event(
        &state.db,
        Some(&claims.sub),
        "settings_change",
        Some("settings"),
        None,
        Some(&details),
        None,
    )
    .await;

    get_settings(State(state)).await
}

// --- Audit Log Endpoint ---

#[derive(Deserialize)]
pub struct AuditQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub user_id: Option<String>,
    pub action: Option<String>,
}

#[derive(Serialize)]
pub struct AuditEntry {
    pub id: String,
    pub user_id: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct AuditResponse {
    pub entries: Vec<AuditEntry>,
    pub total: i64,
    pub page: u32,
    pub limit: u32,
}

pub async fn list_audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditResponse>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = (page - 1) * limit;

    // Build dynamic WHERE clause
    let mut conditions = Vec::new();
    let mut params: Vec<String> = Vec::new();

    if let Some(ref uid) = query.user_id {
        conditions.push("user_id = ?");
        params.push(uid.clone());
    }
    if let Some(ref action) = query.action {
        conditions.push("action = ?");
        params.push(action.clone());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Get total count
    let count_sql = format!("SELECT COUNT(*) FROM audit_log {where_clause}");
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    for p in &params {
        count_query = count_query.bind(p);
    }
    let total = count_query.fetch_one(&state.db).await?;

    // Get entries
    let select_sql = format!(
        "SELECT id, user_id, action, target_type, target_id, details, ip_address, created_at \
         FROM audit_log {where_clause} ORDER BY created_at DESC LIMIT ? OFFSET ?"
    );
    let mut select_query = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
        ),
    >(&select_sql);
    for p in &params {
        select_query = select_query.bind(p);
    }
    select_query = select_query.bind(limit).bind(offset);

    let rows = select_query.fetch_all(&state.db).await?;

    let entries: Vec<AuditEntry> = rows
        .into_iter()
        .map(
            |(id, user_id, action, target_type, target_id, details, ip_address, created_at)| {
                AuditEntry {
                    id,
                    user_id,
                    action,
                    target_type,
                    target_id,
                    details,
                    ip_address,
                    created_at,
                }
            },
        )
        .collect();

    Ok(Json(AuditResponse {
        entries,
        total,
        page,
        limit,
    }))
}
