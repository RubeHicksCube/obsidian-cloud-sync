use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::auth::tokens::{validate_access_token, Claims};
use crate::errors::AppError;

use super::AppState;

pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing authorization header".into()))?;

    let claims = validate_access_token(token, &state.config)?;

    // Check device is not revoked
    let revoked: Option<bool> = sqlx::query_scalar(
        "SELECT revoked FROM devices WHERE id = ? AND user_id = ?",
    )
    .bind(&claims.device_id)
    .bind(&claims.sub)
    .fetch_optional(&state.db)
    .await?;

    match revoked {
        Some(true) => return Err(AppError::Unauthorized("Device has been revoked".into())),
        None => return Err(AppError::Unauthorized("Device not found".into())),
        _ => {}
    }

    // Update last_seen_at for the device
    let now = chrono::Utc::now().timestamp();
    sqlx::query("UPDATE devices SET last_seen_at = ? WHERE id = ?")
        .bind(now)
        .bind(&claims.device_id)
        .execute(&state.db)
        .await?;

    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

pub async fn require_admin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".into()))?;

    // Always verify admin status from the database, not just the JWT claim
    let is_admin: Option<bool> =
        sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?")
            .bind(&claims.sub)
            .fetch_optional(&state.db)
            .await?;

    match is_admin {
        Some(true) => {}
        Some(false) => return Err(AppError::Forbidden("Admin access required".into())),
        None => return Err(AppError::Unauthorized("User not found".into())),
    }

    Ok(next.run(request).await)
}
