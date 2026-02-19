mod admin;
mod audit;
mod auth;
mod background;
mod config;
mod db;
mod devices;
mod errors;
mod files;
mod sync;
mod web;
mod ws;

use axum::{
    http::HeaderValue,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use dashmap::DashMap;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use auth::AppState;
use config::Config;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = Config::from_env();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| config.log_level.clone().into());

    if std::env::var("RUST_LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    let pool = db::init_pool(&config)
        .await
        .expect("Failed to initialize database");

    let state = AppState {
        db: pool.clone(),
        config: config.clone(),
        ws_clients: Arc::new(DashMap::new()),
    };

    let max_body = (config.max_upload_size_mb * 1024 * 1024) as usize;

    // Public routes (no auth)
    let public_routes = Router::new()
        .route("/api/auth/register", post(auth::handlers::register))
        .route("/api/auth/login", post(auth::handlers::login))
        .route("/api/auth/refresh", post(auth::handlers::refresh))
        .route("/api/auth/logout", post(auth::handlers::logout))
        .route("/api/health", get(health));

    // Authenticated routes
    let auth_routes = Router::new()
        .route("/api/auth/change-password", post(auth::handlers::change_password))
        .route("/api/sync/delta", post(sync::handlers::delta))
        .route("/api/sync/upload", post(sync::handlers::upload))
        .route("/api/sync/upload/batch", post(sync::handlers::upload_batch))
        .route("/api/sync/download/{id}", get(sync::handlers::download))
        .route("/api/sync/delete/{id}", delete(sync::handlers::delete_file))
        .route("/api/sync/complete", post(sync::handlers::complete))
        .route("/api/sync/fix-hash", post(sync::handlers::fix_hash))
        .route("/api/files", get(files::handlers::list_files))
        .route(
            "/api/files/{id}/versions",
            get(files::handlers::file_versions),
        )
        .route(
            "/api/files/{id}/rollback",
            post(files::handlers::rollback),
        )
        .route("/api/devices", get(devices::handlers::list_devices))
        .route(
            "/api/devices/{id}",
            delete(devices::handlers::revoke_device),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    // Admin routes (auth + admin check)
    let admin_routes = Router::new()
        .route("/api/admin/users", get(admin::handlers::list_users))
        .route("/api/admin/users", post(admin::handlers::create_user))
        .route(
            "/api/admin/users/{id}",
            delete(admin::handlers::delete_user),
        )
        .route("/api/admin/settings", get(admin::handlers::get_settings))
        .route(
            "/api/admin/settings",
            put(admin::handlers::update_settings),
        )
        .route("/api/admin/audit", get(admin::handlers::list_audit))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_admin,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    // WebSocket route (auth via query param)
    let ws_routes = Router::new()
        .route("/api/ws", get(ws::ws_upgrade));

    // Web UI routes
    let web_routes = Router::new()
        .route("/", get(web::index))
        .route("/static/{*path}", get(web::static_file));

    let app = Router::new()
        .merge(admin_routes)
        .merge(auth_routes)
        .merge(public_routes)
        .merge(ws_routes)
        .merge(web_routes)
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(max_body))
        // CSP headers
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(
                    config
                        .cors_origins
                        .iter()
                        .filter_map(|o| o.parse::<HeaderValue>().ok())
                        .collect::<Vec<_>>(),
                )
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::ACCEPT,
                ])
                .allow_credentials(true),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Spawn background maintenance tasks
    let cancel_token = background::spawn_background_tasks(pool, config.clone());

    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .expect("Failed to bind");
    tracing::info!("ObsidianCloudSync listening on {}", config.bind_address);

    // Graceful shutdown
    let shutdown_signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C handler");
        tracing::info!("Shutdown signal received, draining connections...");
        cancel_token.cancel();
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .expect("Server error");

    tracing::info!("Server shut down gracefully");
}

async fn health() -> &'static str {
    "ok"
}
