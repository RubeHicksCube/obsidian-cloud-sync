use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    PayloadTooLarge(String),
    TooManyRequests(String),
    Internal(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(msg) => write!(f, "Bad Request: {msg}"),
            Self::Unauthorized(msg) => write!(f, "Unauthorized: {msg}"),
            Self::Forbidden(msg) => write!(f, "Forbidden: {msg}"),
            Self::NotFound(msg) => write!(f, "Not Found: {msg}"),
            Self::Conflict(msg) => write!(f, "Conflict: {msg}"),
            Self::PayloadTooLarge(msg) => write!(f, "Payload Too Large: {msg}"),
            Self::TooManyRequests(msg) => write!(f, "Too Many Requests: {msg}"),
            Self::Internal(msg) => write!(f, "Internal Error: {msg}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            Self::PayloadTooLarge(msg) => (StatusCode::PAYLOAD_TOO_LARGE, msg.clone()),
            Self::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg.clone()),
            Self::Internal(msg) => {
                tracing::error!("Internal error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".into(),
                )
            }
        };

        let body = json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        tracing::debug!("JWT validation error: {err}");
        Self::Unauthorized("Invalid or expired token".into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(err.to_string())
    }
}
