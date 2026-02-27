use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::config::Config;

pub async fn init_pool(config: &Config) -> Result<SqlitePool, sqlx::Error> {
    // Ensure data directory exists
    let data_dir = &config.data_dir;
    tokio::fs::create_dir_all(data_dir).await.ok();

    let options = SqliteConnectOptions::from_str(&config.database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    run_migrations(&pool).await?;
    run_vault_migration(&pool).await?;
    seed_default_settings(&pool).await?;

    Ok(pool)
}

async fn ensure_migrations_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    ensure_migrations_table(pool).await?;

    // Collect migration files sorted by name
    let migration_files = collect_migration_files();

    for (name, sql) in &migration_files {
        // Check if already applied
        let applied: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM _migrations WHERE name = ?",
        )
        .bind(name)
        .fetch_one(pool)
        .await?;

        if applied {
            continue;
        }

        tracing::info!("Running migration: {name}");

        // Execute each statement separately
        for statement in sql.split(';') {
            let trimmed = statement.trim();
            if !trimmed.is_empty() {
                sqlx::query(trimmed).execute(pool).await?;
            }
        }

        // Record migration
        let now = chrono::Utc::now().timestamp();
        sqlx::query("INSERT INTO _migrations (name, applied_at) VALUES (?, ?)")
            .bind(name)
            .bind(now)
            .execute(pool)
            .await?;

        tracing::info!("Migration {name} complete.");
    }

    // Safe column additions (idempotent — silently ignores if column already exists)
    add_column_if_missing(pool, "users", "failed_attempts", "INTEGER NOT NULL DEFAULT 0").await;
    add_column_if_missing(pool, "users", "locked_until", "INTEGER").await;
    // Encryption salt shared across all devices for the same account
    add_column_if_missing(pool, "users", "encryption_salt", "TEXT NOT NULL DEFAULT ''").await;
    // Client-encrypted vault key for cross-device passphrase sharing
    add_column_if_missing(pool, "users", "encrypted_vault_key", "TEXT").await;

    Ok(())
}

/// Collect migration SQL files, sorted by filename.
fn collect_migration_files() -> Vec<(String, String)> {
    let mut migrations = vec![
        (
            "001_initial.sql".to_string(),
            include_str!("../migrations/001_initial.sql").to_string(),
        ),
        (
            "002_indexes_and_audit.sql".to_string(),
            include_str!("../migrations/002_indexes_and_audit.sql").to_string(),
        ),
        (
            "003_vault_key.sql".to_string(),
            include_str!("../migrations/003_vault_key.sql").to_string(),
        ),
        (
            "004_vaults.sql".to_string(),
            include_str!("../migrations/004_vaults.sql").to_string(),
        ),
    ];
    migrations.sort_by(|a, b| a.0.cmp(&b.0));
    migrations
}

async fn add_column_if_missing(pool: &SqlitePool, table: &str, column: &str, col_type: &str) {
    let check = format!(
        "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name = '{}'",
        table, column
    );
    let exists: bool = sqlx::query_scalar(&check)
        .fetch_one(pool)
        .await
        .map(|count: bool| count)
        .unwrap_or(false);

    if !exists {
        let alter = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, col_type);
        if let Err(e) = sqlx::query(&alter).execute(pool).await {
            tracing::warn!("Failed to add column {}.{}: {}", table, column, e);
        } else {
            tracing::info!("Added column {}.{}", table, column);
        }
    }
}

/// Idempotent vault schema migration run after SQL migrations.
///
/// Handles three cases:
///   1. Partial previous execution: `files` was dropped but `files_new` rename failed →
///      renames `files_new` back to `files`.
///   2. `files` exists without `vault_id` column → adds the column.
///   3. Already fully migrated → all operations are no-ops.
async fn run_vault_migration(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Step 1: Detect and recover from partial migration
    let files_exists: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='files'",
    )
    .fetch_one(pool)
    .await?;

    if !files_exists {
        let files_new_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='files_new'",
        )
        .fetch_one(pool)
        .await?;

        if files_new_exists {
            tracing::warn!("Recovering partial vault migration: renaming files_new → files");
            sqlx::query("ALTER TABLE files_new RENAME TO files")
                .execute(pool)
                .await?;
            tracing::info!("Recovery complete: files_new renamed to files");
        } else {
            return Err(sqlx::Error::Configuration(
                "Database corrupt: 'files' table is missing and 'files_new' does not exist".into(),
            ));
        }
    }

    // Step 2: Add vault_id columns if not present (idempotent)
    add_column_if_missing(pool, "files", "vault_id", "TEXT NOT NULL DEFAULT 'default'").await;
    add_column_if_missing(
        pool,
        "file_versions",
        "vault_id",
        "TEXT NOT NULL DEFAULT 'default'",
    )
    .await;

    // Step 3: Ensure index exists (IF NOT EXISTS makes this idempotent)
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_files_user_vault_path ON files(user_id, vault_id, path)",
    )
    .execute(pool)
    .await?;

    // Step 4: Seed "Main Vault" (id='default') for every user that doesn't have one yet
    sqlx::query(
        "INSERT OR IGNORE INTO vaults (id, user_id, name, created_at) \
         SELECT 'default', id, 'Main Vault', unixepoch() FROM users",
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_default_settings(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let defaults = [
        ("max_versions_per_file", "50"),
        ("max_version_age_days", "90"),
        ("registration_open", "true"),
    ];

    for (key, value) in defaults {
        sqlx::query("INSERT OR IGNORE INTO server_settings (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await?;
    }

    Ok(())
}
