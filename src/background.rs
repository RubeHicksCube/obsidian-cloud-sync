use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

use crate::config::Config;

/// Spawn all background maintenance tasks. Returns a CancellationToken to stop them.
pub fn spawn_background_tasks(
    pool: SqlitePool,
    config: Config,
) -> CancellationToken {
    let token = CancellationToken::new();

    // Token cleanup — every 1 hour
    {
        let pool = pool.clone();
        let cancel = token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {}
                }
                if let Err(e) = cleanup_expired_tokens(&pool).await {
                    tracing::warn!("Token cleanup error: {e}");
                }
            }
        });
    }

    // Version pruning — run once at startup (after 30s), then every 6 hours
    {
        let pool = pool.clone();
        let config = config.clone();
        let cancel = token.clone();
        tokio::spawn(async move {
            // Initial run shortly after startup to clean up any accumulated versions
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
            }
            if let Err(e) = prune_versions(&pool, &config).await {
                tracing::warn!("Version pruning error: {e}");
            }
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)) => {}
                }
                if let Err(e) = prune_versions(&pool, &config).await {
                    tracing::warn!("Version pruning error: {e}");
                }
            }
        });
    }

    // Blob GC — every 6 hours (offset by 1 hour from version pruning)
    {
        let pool = pool.clone();
        let config = config.clone();
        let cancel = token.clone();
        tokio::spawn(async move {
            // Initial delay to offset from version pruning
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {}
            }
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)) => {}
                }
                if let Err(e) = garbage_collect_blobs(&pool, &config).await {
                    tracing::warn!("Blob GC error: {e}");
                }
            }
        });
    }

    // Sync cursor cleanup — every 24 hours
    {
        let pool = pool.clone();
        let cancel = token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)) => {}
                }
                if let Err(e) = cleanup_sync_cursors(&pool).await {
                    tracing::warn!("Sync cursor cleanup error: {e}");
                }
            }
        });
    }

    token
}

/// Delete expired refresh tokens.
async fn cleanup_expired_tokens(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    let count = result.rows_affected();
    if count > 0 {
        tracing::info!("Cleaned up {count} expired refresh tokens");
    }
    Ok(())
}

/// Prune old file versions based on max count and retention days.
async fn prune_versions(pool: &SqlitePool, config: &Config) -> Result<(), sqlx::Error> {
    let max_versions = config.max_versions_per_file as i64;
    let retention_seconds = config.version_retention_days as i64 * 86400;
    let cutoff = chrono::Utc::now().timestamp() - retention_seconds;
    let mut total_pruned: u64 = 0;

    // Get all files
    let files: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, current_version FROM files")
            .fetch_all(pool)
            .await?;

    for (file_id, current_version) in &files {
        // Delete versions exceeding max count (keep newest, never delete current)
        let versions: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT id, version, created_at FROM file_versions WHERE file_id = ? ORDER BY version DESC",
        )
        .bind(file_id)
        .fetch_all(pool)
        .await?;

        for (i, (ver_id, version, created_at)) in versions.iter().enumerate() {
            // Never delete the current version
            if *version == *current_version {
                continue;
            }
            // Delete if beyond max count or older than retention
            let beyond_count = i as i64 >= max_versions;
            let beyond_age = *created_at < cutoff;
            if beyond_count || beyond_age {
                sqlx::query("DELETE FROM file_versions WHERE id = ?")
                    .bind(ver_id)
                    .execute(pool)
                    .await?;
                total_pruned += 1;
            }
        }
    }

    if total_pruned > 0 {
        tracing::info!("Pruned {total_pruned} old file versions");
    }
    Ok(())
}

/// Garbage collect unreferenced blobs from the filesystem.
async fn garbage_collect_blobs(pool: &SqlitePool, config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let blobs_dir = PathBuf::from(&config.data_dir).join("blobs");
    if !blobs_dir.exists() {
        return Ok(());
    }

    // Collect all referenced blob paths from file_versions
    let referenced: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT blob_path FROM file_versions")
            .fetch_all(pool)
            .await?;

    let referenced_set: HashSet<String> = referenced.into_iter().map(|(p,)| p).collect();

    // Walk the blobs directory and find unreferenced files
    let mut removed: u64 = 0;
    let mut user_dirs = tokio::fs::read_dir(&blobs_dir).await?;

    while let Some(user_entry) = user_dirs.next_entry().await? {
        if !user_entry.metadata().await?.is_dir() {
            continue;
        }
        let user_path = user_entry.path();
        let mut prefix1_dirs = tokio::fs::read_dir(&user_path).await?;

        while let Some(p1_entry) = prefix1_dirs.next_entry().await? {
            if !p1_entry.metadata().await?.is_dir() {
                continue;
            }
            let p1_path = p1_entry.path();
            let mut prefix2_dirs = tokio::fs::read_dir(&p1_path).await?;

            while let Some(p2_entry) = prefix2_dirs.next_entry().await? {
                if !p2_entry.metadata().await?.is_dir() {
                    continue;
                }
                let p2_path = p2_entry.path();
                let mut blob_files = tokio::fs::read_dir(&p2_path).await?;

                while let Some(blob_entry) = blob_files.next_entry().await? {
                    let blob_path = blob_entry.path();
                    if blob_entry.metadata().await?.is_file() {
                        let path_str = blob_path.to_string_lossy().to_string();
                        if !referenced_set.contains(&path_str) {
                            tokio::fs::remove_file(&blob_path).await?;
                            removed += 1;
                        }
                    }
                }
            }
        }
    }

    if removed > 0 {
        tracing::info!("Garbage collected {removed} unreferenced blobs");
    }
    Ok(())
}

/// Remove sync cursors for revoked devices.
async fn cleanup_sync_cursors(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM sync_cursors WHERE device_id IN \
         (SELECT id FROM devices WHERE revoked = TRUE)",
    )
    .execute(pool)
    .await?;
    let count = result.rows_affected();
    if count > 0 {
        tracing::info!("Cleaned up {count} sync cursors for revoked devices");
    }
    Ok(())
}
