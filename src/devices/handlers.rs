use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::auth::tokens::Claims;
use crate::auth::AppState;
use crate::errors::AppError;

#[derive(Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: Option<String>,
    pub last_seen_at: i64,
    pub created_at: i64,
    pub revoked: bool,
}

pub async fn list_devices(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
) -> Result<Json<Vec<DeviceInfo>>, AppError> {
    let devices: Vec<(String, String, Option<String>, i64, i64, bool)> = sqlx::query_as(
        "SELECT id, name, device_type, last_seen_at, created_at, revoked \
         FROM devices WHERE user_id = ? ORDER BY last_seen_at DESC",
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await?;

    let result: Vec<DeviceInfo> = devices
        .into_iter()
        .map(
            |(id, name, device_type, last_seen_at, created_at, revoked)| DeviceInfo {
                id,
                name,
                device_type,
                last_seen_at,
                created_at,
                revoked,
            },
        )
        .collect();

    Ok(Json(result))
}

pub async fn revoke_device(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Verify device belongs to user
    let exists: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM devices WHERE id = ? AND user_id = ?",
    )
    .bind(&device_id)
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await?;

    if !exists {
        return Err(AppError::NotFound("Device not found".into()));
    }

    // Don't allow revoking your own current device
    if device_id == claims.device_id {
        return Err(AppError::BadRequest(
            "Cannot revoke your current device".into(),
        ));
    }

    // Revoke device
    sqlx::query("UPDATE devices SET revoked = TRUE WHERE id = ?")
        .bind(&device_id)
        .execute(&state.db)
        .await?;

    // Delete all refresh tokens for this device
    sqlx::query("DELETE FROM refresh_tokens WHERE device_id = ?")
        .bind(&device_id)
        .execute(&state.db)
        .await?;

    Ok(Json(
        serde_json::json!({"message": "Device revoked successfully"}),
    ))
}
