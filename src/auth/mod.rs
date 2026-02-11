pub mod handlers;
pub mod middleware;
pub mod password;
pub mod tokens;

use crate::config::Config;
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Config,
}
