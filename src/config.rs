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
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8443".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:data/obsidian_sync.db".into()),
            data_dir: env::var("DATA_DIR").unwrap_or_else(|_| "data".into()),
            jwt_secret: {
                let secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
                if secret.len() < 32 {
                    panic!("JWT_SECRET must be at least 32 characters for adequate security");
                }
                secret
            },
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
        }
    }
}
