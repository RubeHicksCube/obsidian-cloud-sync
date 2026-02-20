use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::errors::AppError;

/// Validates that a string is a valid lowercase hex SHA-256 hash (64 hex chars).
fn is_valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Validates that a user_id is a valid UUID (prevents path traversal via user_id).
fn is_valid_user_id(user_id: &str) -> bool {
    uuid::Uuid::parse_str(user_id).is_ok()
}

/// Content-addressed blob storage.
/// Files stored at: data/{user_id}/{hash[0:2]}/{hash[2:4]}/{hash}
pub struct BlobStorage {
    base_dir: PathBuf,
}

impl BlobStorage {
    pub fn new(data_dir: &str) -> Self {
        Self {
            base_dir: PathBuf::from(data_dir).join("blobs"),
        }
    }

    pub fn blob_path(&self, user_id: &str, hash: &str) -> Result<PathBuf, AppError> {
        if !is_valid_user_id(user_id) {
            return Err(AppError::BadRequest("Invalid user ID".into()));
        }
        if !is_valid_hash(hash) {
            return Err(AppError::BadRequest("Invalid hash".into()));
        }
        let prefix1 = &hash[..2];
        let prefix2 = &hash[2..4];
        let path = self.base_dir
            .join(user_id)
            .join(prefix1)
            .join(prefix2)
            .join(hash);

        // Defense in depth: verify the constructed path is under base_dir.
        // We use a lexical starts_with check instead of canonicalize so this
        // works even when the subdirectory does not exist yet (new blobs).
        // Path traversal is already prevented above by the UUID and hex-hash
        // validators, so this is an extra sanity guard.
        if !path.starts_with(&self.base_dir) {
            return Err(AppError::BadRequest("Invalid path".into()));
        }

        Ok(path)
    }

    pub async fn store(&self, user_id: &str, data: &[u8]) -> Result<(String, PathBuf), AppError> {
        let hash = hash_bytes(data);
        let path = self.blob_path(user_id, &hash)?;

        // Skip if blob already exists (deduplication)
        if path.exists() {
            return Ok((hash, path));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&path, data).await?;
        Ok((hash, path))
    }

    pub async fn read(&self, user_id: &str, hash: &str) -> Result<Vec<u8>, AppError> {
        let path = self.blob_path(user_id, hash)?;
        if !path.exists() {
            return Err(AppError::NotFound("Blob not found".into()));
        }
        Ok(fs::read(&path).await?)
    }

    #[allow(dead_code)]
    pub async fn exists(&self, user_id: &str, hash: &str) -> bool {
        match self.blob_path(user_id, hash) {
            Ok(path) => path.exists(),
            Err(_) => false,
        }
    }

    #[allow(dead_code)]
    pub async fn delete(&self, user_id: &str, hash: &str) -> Result<(), AppError> {
        let path = self.blob_path(user_id, hash)?;
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }

    /// Calculate total storage used by a user
    #[allow(dead_code)]
    pub async fn user_storage_bytes(&self, user_id: &str) -> Result<u64, AppError> {
        let user_dir = self.base_dir.join(user_id);
        if !user_dir.exists() {
            return Ok(0);
        }
        dir_size(&user_dir).await
    }
}

pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[allow(dead_code)]
async fn dir_size(path: &Path) -> Result<u64, AppError> {
    let mut total = 0u64;
    let mut entries = fs::read_dir(path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if metadata.is_dir() {
            total += Box::pin(dir_size(&entry.path())).await?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}
