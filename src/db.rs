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
    ];
    migrations.sort_by(|a, b| a.0.cmp(&b.0));
    migrations
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
