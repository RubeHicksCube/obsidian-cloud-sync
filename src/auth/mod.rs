pub mod handlers;
pub mod middleware;
pub mod password;
pub mod tokens;

use crate::config::Config;
use dashmap::DashMap;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Message sent through WebSocket broadcast channels.
#[derive(Clone, Debug)]
pub struct WsMessage {
    pub msg_type: String,
    pub file_path: String,
    pub action: String,
    pub source_device_id: String,
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Config,
    /// Map of user_id -> broadcast sender for WebSocket notifications.
    pub ws_clients: Arc<DashMap<String, broadcast::Sender<WsMessage>>>,
}

impl AppState {
    /// Notify all WebSocket clients for a user about a sync update.
    pub fn notify_sync_update(
        &self,
        user_id: &str,
        source_device_id: &str,
        file_path: &str,
        action: &str,
    ) {
        if let Some(tx) = self.ws_clients.get(user_id) {
            let _ = tx.send(WsMessage {
                msg_type: "sync_update".to_string(),
                file_path: file_path.to_string(),
                action: action.to_string(),
                source_device_id: source_device_id.to_string(),
            });
        }
    }
}
