use axum::{extract::State, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::tokens::Claims;
use crate::auth::AppState;
use crate::errors::AppError;

#[derive(Debug, Serialize)]
pub struct VaultInfo {
    pub id: String,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateVaultRequest {
    pub name: String,
}

pub async fn list(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
) -> Result<Json<Vec<VaultInfo>>, AppError> {
    let vaults: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT id, name, created_at FROM vaults WHERE user_id = ? ORDER BY created_at",
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await?;

    let result = vaults
        .into_iter()
        .map(|(id, name, created_at)| VaultInfo { id, name, created_at })
        .collect();

    Ok(Json(result))
}

pub async fn create(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<CreateVaultRequest>,
) -> Result<Json<VaultInfo>, AppError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Vault name cannot be empty".into()));
    }
    if name.len() > 100 {
        return Err(AppError::BadRequest(
            "Vault name too long (max 100 chars)".into(),
        ));
    }

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO vaults (id, user_id, name, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&claims.sub)
    .bind(&name)
    .bind(now)
    .execute(&state.db)
    .await?;

    Ok(Json(VaultInfo { id, name, created_at: now }))
}
