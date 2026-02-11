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

async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let migration_sql = include_str!("../migrations/001_initial.sql");

    // Check if migrations have already been applied by checking for users table
    let table_exists: bool =
        sqlx::query_scalar("SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='users'")
            .fetch_one(pool)
            .await?;

    if !table_exists {
        tracing::info!("Running initial migration...");
        // Execute each statement separately
        for statement in migration_sql.split(';') {
            let trimmed = statement.trim();
            if !trimmed.is_empty() {
                sqlx::query(trimmed).execute(pool).await?;
            }
        }
        tracing::info!("Migration complete.");
    }

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
