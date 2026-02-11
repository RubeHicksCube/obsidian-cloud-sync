use std::net::SocketAddr;

async fn spawn_server() -> (SocketAddr, reqwest::Client) {
    // Use a temporary directory for each test's isolated data
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().to_string_lossy().to_string();
    let db_path = format!("sqlite:{}/test.db", data_dir);

    let config = obsidian_cloud_sync::config::Config {
        bind_address: "127.0.0.1:0".into(),
        database_url: db_path,
        data_dir,
        jwt_secret: "test-secret-key-for-testing-at-least-32-chars".into(),
        access_token_expiry_secs: 900,
        refresh_token_expiry_days: 30,
        max_upload_size_mb: 100,
        registration_open: true,
        cors_origins: vec!["http://localhost:0".into()],
    };

    let pool = obsidian_cloud_sync::db::init_pool(&config).await.unwrap();

    let state = obsidian_cloud_sync::auth::AppState {
        db: pool,
        config: config.clone(),
    };

    use axum::{
        middleware,
        routing::{delete, get, post},
        Router,
    };
    use tower_http::cors::{Any, CorsLayer};

    let public_routes = Router::new()
        .route(
            "/api/auth/register",
            post(obsidian_cloud_sync::auth::handlers::register),
        )
        .route(
            "/api/auth/login",
            post(obsidian_cloud_sync::auth::handlers::login),
        )
        .route(
            "/api/auth/refresh",
            post(obsidian_cloud_sync::auth::handlers::refresh),
        )
        .route(
            "/api/auth/logout",
            post(obsidian_cloud_sync::auth::handlers::logout),
        )
        .route("/api/health", get(|| async { "ok" }));

    let auth_routes = Router::new()
        .route(
            "/api/files",
            get(obsidian_cloud_sync::files::handlers::list_files),
        )
        .route(
            "/api/devices",
            get(obsidian_cloud_sync::devices::handlers::list_devices),
        )
        .route(
            "/api/devices/{id}",
            delete(obsidian_cloud_sync::devices::handlers::revoke_device),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            obsidian_cloud_sync::auth::middleware::require_auth,
        ));

    let admin_routes = Router::new()
        .route(
            "/api/admin/users",
            get(obsidian_cloud_sync::admin::handlers::list_users),
        )
        .route(
            "/api/admin/settings",
            get(obsidian_cloud_sync::admin::handlers::get_settings),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            obsidian_cloud_sync::auth::middleware::require_admin,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            obsidian_cloud_sync::auth::middleware::require_auth,
        ));

    let app = Router::new()
        .merge(admin_routes)
        .merge(auth_routes)
        .merge(public_routes)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Keep tmp alive by leaking it (tests are short-lived)
    std::mem::forget(tmp);

    let client = reqwest::Client::new();
    (addr, client)
}

#[tokio::test]
async fn test_health() {
    let (addr, client) = spawn_server().await;
    let res = client
        .get(format!("http://{}/api/health", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn test_register_and_login() {
    let (addr, client) = spawn_server().await;
    let base = format!("http://{}", addr);

    // Register first user (becomes admin)
    let res = client
        .post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({
            "username": "admin",
            "password": "password123",
            "device_name": "test"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["access_token"].is_string());
    assert!(body["is_admin"].as_bool().unwrap());

    // Login
    let res = client
        .post(format!("{}/api/auth/login", base))
        .json(&serde_json::json!({
            "username": "admin",
            "password": "password123",
            "device_name": "test2"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
}

#[tokio::test]
async fn test_auth_required() {
    let (addr, client) = spawn_server().await;
    let res = client
        .get(format!("http://{}/api/files", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn test_list_files_and_devices() {
    let (addr, client) = spawn_server().await;
    let base = format!("http://{}", addr);

    // Register
    let res = client
        .post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({
            "username": "testuser",
            "password": "password123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let token = body["access_token"].as_str().unwrap();

    // List files (empty)
    let res = client
        .get(format!("{}/api/files", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let files: Vec<serde_json::Value> = res.json().await.unwrap();
    assert!(files.is_empty());

    // List devices (should have 1)
    let res = client
        .get(format!("{}/api/devices", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let devices: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(devices.len(), 1);
}

#[tokio::test]
async fn test_admin_endpoints() {
    let (addr, client) = spawn_server().await;
    let base = format!("http://{}", addr);

    // Register admin
    let res = client
        .post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({
            "username": "admin",
            "password": "password123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let token = body["access_token"].as_str().unwrap();

    // List users
    let res = client
        .get(format!("{}/api/admin/users", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let users: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(users.len(), 1);

    // Get settings
    let res = client
        .get(format!("{}/api/admin/settings", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn test_refresh_token() {
    let (addr, client) = spawn_server().await;
    let base = format!("http://{}", addr);

    // Register
    let res = client
        .post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({
            "username": "refreshuser",
            "password": "password123"
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let refresh_token = body["refresh_token"].as_str().unwrap();

    // Refresh
    let res = client
        .post(format!("{}/api/auth/refresh", base))
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["access_token"].is_string());
    // Old refresh token should now be invalid (rotated)
    assert_ne!(body["refresh_token"].as_str().unwrap(), refresh_token);
}
