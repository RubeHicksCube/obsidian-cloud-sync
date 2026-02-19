use sqlx::SqlitePool;
use uuid::Uuid;

/// Log an audit event. Failures are logged but never propagated to callers.
pub async fn log_event(
    pool: &SqlitePool,
    user_id: Option<&str>,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<&str>,
    details: Option<&str>,
    ip_address: Option<&str>,
) {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    let result = sqlx::query(
        "INSERT INTO audit_log (id, user_id, action, target_type, target_id, details, ip_address, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(details)
    .bind(ip_address)
    .bind(now)
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!("Failed to write audit log: {e}");
    }
}
