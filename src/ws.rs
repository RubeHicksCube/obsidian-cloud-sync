use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::auth::tokens::validate_access_token;
use crate::auth::{AppState, WsMessage};
use crate::errors::AppError;

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: String,
}

pub async fn ws_upgrade(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    // Authenticate via query parameter token
    let claims = validate_access_token(&query.token, &state.config)?;

    let user_id = claims.sub.clone();
    let device_id = claims.device_id.clone();

    // Get or create broadcast channel for this user
    let rx = {
        let entry = state
            .ws_clients
            .entry(user_id.clone())
            .or_insert_with(|| broadcast::channel(64).0);
        entry.subscribe()
    };

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, user_id, device_id, rx)))
}

async fn handle_ws(
    socket: WebSocket,
    user_id: String,
    device_id: String,
    mut rx: broadcast::Receiver<WsMessage>,
) {
    let (mut sender, mut receiver): (SplitSink<WebSocket, Message>, SplitStream<WebSocket>) = socket.split();

    tracing::info!("WebSocket connected: user={}, device={}", user_id, &device_id[..8.min(device_id.len())]);

    // Heartbeat ping interval
    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            // Forward broadcast messages to the WebSocket client
            msg = rx.recv() => {
                match msg {
                    Ok(ws_msg) => {
                        // Don't send updates back to the device that caused them
                        if ws_msg.source_device_id == device_id {
                            continue;
                        }
                        let json = serde_json::json!({
                            "type": ws_msg.msg_type,
                            "file_path": ws_msg.file_path,
                            "action": ws_msg.action,
                        });
                        if sender.send(Message::Text(json.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("WebSocket client lagged by {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }

            // Handle incoming messages from client (just keep-alive pongs)
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // Ignore other messages
                }
            }

            // Send periodic pings
            _ = ping_interval.tick() => {
                if sender.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        }
    }

    tracing::info!("WebSocket disconnected: user={}, device={}", user_id, &device_id[..8.min(device_id.len())]);
}
