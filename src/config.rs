use std::env;

#[derive(Clone)]
pub struct Config {
    pub bind_address: String,
    pub database_url: String,
    pub data_dir: String,
    pub jwt_secret: String,
    pub access_token_expiry_secs: u64,
    pub refresh_token_expiry_days: u64,
    pub max_upload_size_mb: u64,
    pub registration_open: bool,
    pub cors_origins: Vec<String>,
    // Security
    #[allow(dead_code)]
    pub rate_limit_rpm: u32,
    pub lockout_threshold: u32,
    pub lockout_duration_secs: u64,
    // Storage
    pub max_storage_per_user_mb: u64,
    pub max_versions_per_file: u32,
    pub version_retention_days: u32,
    // Encryption
    pub require_encryption: bool,
    // Logging
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8443".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:data/obsidian_sync.db".into()),
            data_dir: env::var("DATA_DIR").unwrap_or_else(|_| "data".into()),
            jwt_secret: env::var("JWT_SECRET").expect(
                "JWT_SECRET must be set (at least 32 characters recommended)",
            ),
            access_token_expiry_secs: env::var("ACCESS_TOKEN_EXPIRY_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            refresh_token_expiry_days: env::var("REFRESH_TOKEN_EXPIRY_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            max_upload_size_mb: env::var("MAX_UPLOAD_SIZE_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            registration_open: env::var("REGISTRATION_OPEN")
                .ok()
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            cors_origins: env::var("CORS_ORIGINS")
                .ok()
                .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_else(|| vec!["http://localhost:8443".to_string()]),
            rate_limit_rpm: env::var("RATE_LIMIT_RPM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            lockout_threshold: env::var("LOCKOUT_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            lockout_duration_secs: env::var("LOCKOUT_DURATION_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            max_storage_per_user_mb: env::var("MAX_STORAGE_PER_USER_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
            max_versions_per_file: env::var("MAX_VERSIONS_PER_FILE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            version_retention_days: env::var("VERSION_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(90),
            require_encryption: env::var("REQUIRE_ENCRYPTION")
                .ok()
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
        }
    }
}
