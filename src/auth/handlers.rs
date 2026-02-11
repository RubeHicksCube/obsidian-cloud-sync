use axum::{extract::State, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::password::{hash_password, verify_password};
use crate::auth::tokens::{
    create_access_token, generate_refresh_token, hash_refresh_token,
};
use crate::errors::AppError;

use super::AppState;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub device_name: Option<String>,
    pub device_type: Option<String>,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub device_id: String,
    pub is_admin: bool,
}

/// Maximum password length to prevent Argon2 DoS with huge passwords.
const MAX_PASSWORD_LENGTH: usize = 256;

/// Validate username: 3-64 chars, alphanumeric plus underscore/hyphen only.
fn validate_username(username: &str) -> Result<(), AppError> {
    if username.len() < 3 {
        return Err(AppError::BadRequest("Username must be at least 3 characters".into()));
    }
    if username.len() > 64 {
        return Err(AppError::BadRequest("Username must be at most 64 characters".into()));
    }
    if !username.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err(AppError::BadRequest(
            "Username may only contain letters, numbers, underscores, and hyphens".into(),
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 8 {
        return Err(AppError::BadRequest("Password must be at least 8 characters".into()));
    }
    if password.len() > MAX_PASSWORD_LENGTH {
        return Err(AppError::BadRequest(
            format!("Password must be at most {MAX_PASSWORD_LENGTH} characters"),
        ));
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    validate_username(&req.username)?;
    validate_password(&req.password)?;

    // Check if registration is open (or no users yet)
    let user_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&state.db)
            .await?;

    if user_count > 0 {
        // Check server setting first (DB setting overrides env config)
        let reg_open: Option<String> = sqlx::query_scalar(
            "SELECT value FROM server_settings WHERE key = 'registration_open'",
        )
        .fetch_optional(&state.db)
        .await?;

        // Registration is open only if BOTH the DB setting and config allow it
        let db_open = reg_open.as_deref() == Some("true");
        let config_open = state.config.registration_open;
        if !db_open || !config_open {
            return Err(AppError::Forbidden("Registration is closed".into()));
        }
    }

    // Check for existing username
    let exists: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM users WHERE username = ?",
    )
    .bind(&req.username)
    .fetch_one(&state.db)
    .await?;

    if exists {
        return Err(AppError::Conflict("Username already taken".into()));
    }

    let user_id = Uuid::new_v4().to_string();
    let password_hash = hash_password(&req.password)?;
    let now = Utc::now().timestamp();
    let is_admin = user_count == 0; // First user is admin

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, is_admin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(is_admin)
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await?;

    // Create device
    let device_id = Uuid::new_v4().to_string();
    let device_name = req.device_name.unwrap_or_else(|| "Web".into());
    sqlx::query(
        "INSERT INTO devices (id, user_id, name, device_type, last_seen_at, created_at, revoked) VALUES (?, ?, ?, ?, ?, ?, FALSE)",
    )
    .bind(&device_id)
    .bind(&user_id)
    .bind(&device_name)
    .bind("web")
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await?;

    // Create tokens
    let access_token = create_access_token(&user_id, &device_id, is_admin, &state.config)?;
    let refresh_token = generate_refresh_token();
    let refresh_hash = hash_refresh_token(&refresh_token);
    let refresh_expires = now + (state.config.refresh_token_expiry_days * 86400) as i64;

    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, device_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user_id)
    .bind(&device_id)
    .bind(&refresh_hash)
    .bind(refresh_expires)
    .bind(now)
    .execute(&state.db)
    .await?;

    if is_admin {
        tracing::info!("First user '{}' created as admin", req.username);
    }

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
        user_id,
        device_id,
        is_admin,
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let row: Option<(String, String, bool)> = sqlx::query_as(
        "SELECT id, password_hash, is_admin FROM users WHERE username = ?",
    )
    .bind(&req.username)
    .fetch_optional(&state.db)
    .await?;

    let (user_id, password_hash, is_admin) =
        row.ok_or_else(|| AppError::Unauthorized("Invalid credentials".into()))?;

    if !verify_password(&req.password, &password_hash)? {
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }

    let now = Utc::now().timestamp();
    let device_id = Uuid::new_v4().to_string();
    let device_name = req.device_name.unwrap_or_else(|| "Unknown device".into());
    let device_type = req.device_type.as_deref().unwrap_or("unknown");

    sqlx::query(
        "INSERT INTO devices (id, user_id, name, device_type, last_seen_at, created_at, revoked) VALUES (?, ?, ?, ?, ?, ?, FALSE)",
    )
    .bind(&device_id)
    .bind(&user_id)
    .bind(&device_name)
    .bind(device_type)
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await?;

    let access_token = create_access_token(&user_id, &device_id, is_admin, &state.config)?;
    let refresh_token = generate_refresh_token();
    let refresh_hash = hash_refresh_token(&refresh_token);
    let refresh_expires = now + (state.config.refresh_token_expiry_days * 86400) as i64;

    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, device_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user_id)
    .bind(&device_id)
    .bind(&refresh_hash)
    .bind(refresh_expires)
    .bind(now)
    .execute(&state.db)
    .await?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
        user_id,
        device_id,
        is_admin,
    }))
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let token_hash = hash_refresh_token(&req.refresh_token);
    let now = Utc::now().timestamp();

    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT rt.id, rt.user_id, rt.device_id FROM refresh_tokens rt \
         JOIN devices d ON d.id = rt.device_id \
         WHERE rt.token_hash = ? AND rt.expires_at > ? AND d.revoked = FALSE",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_optional(&state.db)
    .await?;

    let (token_id, user_id, device_id) =
        row.ok_or_else(|| AppError::Unauthorized("Invalid or expired refresh token".into()))?;

    // Get user info
    let is_admin: bool =
        sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?")
            .bind(&user_id)
            .fetch_one(&state.db)
            .await?;

    // Delete old refresh token
    sqlx::query("DELETE FROM refresh_tokens WHERE id = ?")
        .bind(&token_id)
        .execute(&state.db)
        .await?;

    // Issue new tokens
    let access_token = create_access_token(&user_id, &device_id, is_admin, &state.config)?;
    let new_refresh = generate_refresh_token();
    let new_refresh_hash = hash_refresh_token(&new_refresh);
    let refresh_expires = now + (state.config.refresh_token_expiry_days * 86400) as i64;

    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, device_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&user_id)
    .bind(&device_id)
    .bind(&new_refresh_hash)
    .bind(refresh_expires)
    .bind(now)
    .execute(&state.db)
    .await?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token: new_refresh,
        user_id,
        device_id,
        is_admin,
    }))
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
}

pub async fn logout(
    State(state): State<AppState>,
    Json(req): Json<LogoutRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let token_hash = hash_refresh_token(&req.refresh_token);

    sqlx::query("DELETE FROM refresh_tokens WHERE token_hash = ?")
        .bind(&token_hash)
        .execute(&state.db)
        .await?;

    Ok(Json(MessageResponse {
        message: "Logged out successfully".into(),
    }))
}
