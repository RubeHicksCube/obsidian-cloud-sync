use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct FileManifestEntry {
    pub path: String,
    pub hash: String,
    pub size: i64,
    pub modified_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct DeltaRequest {
    pub files: Vec<FileManifestEntry>,
    /// If provided, used to determine if missing files were deleted locally
    #[serde(default)]
    pub device_id: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum SyncAction {
    Upload,
    Download,
    Delete,
    Conflict,
}

#[derive(Debug, Serialize)]
pub struct SyncInstruction {
    pub path: String,
    pub action: SyncAction,
    pub file_id: Option<String>,
    pub server_hash: Option<String>,
    pub server_modified_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DeltaResponse {
    pub instructions: Vec<SyncInstruction>,
    pub server_time: i64,
}

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub file_id: String,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
pub struct CompleteRequest {
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct FixHashRequest {
    pub file_id: String,
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct CompleteResponse {
    pub message: String,
    pub server_version: i64,
}
