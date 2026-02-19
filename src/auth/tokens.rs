use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::Config;
use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,       // user_id
    pub device_id: String, // device_id
    pub is_admin: bool,
    pub exp: usize,
    pub iat: usize,
}

pub fn create_access_token(
    user_id: &str,
    device_id: &str,
    is_admin: bool,
    config: &Config,
) -> Result<String, AppError> {
    let now = Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        device_id: device_id.to_string(),
        is_admin,
        exp: now + config.access_token_expiry_secs as usize,
        iat: now,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("Token creation failed: {e}")))
}

pub fn validate_access_token(token: &str, config: &Config) -> Result<Claims, AppError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

pub fn generate_refresh_token() -> String {
    let random_bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(random_bytes)
}

pub fn hash_refresh_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Constant-time comparison of two hex-encoded hashes.
pub fn verify_token_hash(provided_hash: &str, stored_hash: &str) -> bool {
    let a = provided_hash.as_bytes();
    let b = stored_hash.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}
