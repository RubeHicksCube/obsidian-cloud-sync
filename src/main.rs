mod admin;
mod auth;
mod config;
mod db;
mod devices;
mod errors;
mod files;
mod sync;
mod web;

use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    trace::TraceLayer,
};

use auth::AppState;
use config::Config;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::from_env();
    let pool = db::init_pool(&config).await.expect("Failed to initialize database");

    let state = AppState {
        db: pool,
        config: config.clone(),
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
        .route("/api/sync/delta", post(sync::handlers::delta))
        .route("/api/sync/upload", post(sync::handlers::upload))
        .route("/api/sync/upload/batch", post(sync::handlers::upload_batch))
        .route("/api/sync/download/{id}", get(sync::handlers::download))
        .route("/api/sync/complete", post(sync::handlers::complete))
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
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_admin,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    // Web UI routes
    let web_routes = Router::new()
        .route("/", get(web::index))
        .route("/static/{*path}", get(web::static_file));

    let app = Router::new()
        .merge(admin_routes)
        .merge(auth_routes)
        .merge(public_routes)
        .merge(web_routes)
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(max_body))
        .layer(
            CorsLayer::new()
                .allow_origin(
                    config.cors_origins.iter()
                        .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
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

    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .expect("Failed to bind");
    tracing::info!("ObsidianCloudSync listening on {}", config.bind_address);
    axum::serve(listener, app).await.expect("Server error");
}

async fn health() -> &'static str {
    "ok"
}
