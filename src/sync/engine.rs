use sqlx::SqlitePool;

use crate::errors::AppError;
use crate::sync::models::*;

pub async fn compute_delta(
    db: &SqlitePool,
    user_id: &str,
    device_id: &str,
    client_files: &[FileManifestEntry],
) -> Result<Vec<SyncInstruction>, AppError> {
    let mut instructions = Vec::new();

    // Get all server files for this user
    let server_files: Vec<(String, String, String, i64, bool, i64)> = sqlx::query_as(
        "SELECT id, path, hash, size, is_deleted, updated_at FROM files WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    // Build a map of server files by path
    let mut server_map: std::collections::HashMap<String, (String, String, i64, bool, i64)> =
        std::collections::HashMap::new();
    for (id, path, hash, size, is_deleted, updated_at) in &server_files {
        server_map.insert(
            path.clone(),
            (id.clone(), hash.clone(), *size, *is_deleted, *updated_at),
        );
    }

    // Build a set of client paths
    let client_paths: std::collections::HashSet<&str> =
        client_files.iter().map(|f| f.path.as_str()).collect();

    // Check if this device has synced before and retrieve its last known server version.
    // server_version is MAX(updated_at) across all files at the time of the last sync,
    // used to distinguish files added after the device last synced (→ Download) from
    // files the device previously knew about but is no longer reporting (→ Delete).
    let cursor: Option<i64> = sqlx::query_scalar(
        "SELECT server_version FROM sync_cursors WHERE user_id = ? AND device_id = ?",
    )
    .bind(user_id)
    .bind(device_id)
    .fetch_optional(db)
    .await?;

    let has_synced_before = cursor.is_some();
    let last_known_version = cursor.unwrap_or(0);

    // Compare each client file against server state
    for client_file in client_files {
        match server_map.get(&client_file.path) {
            None => {
                // File exists on client but not server → upload
                instructions.push(SyncInstruction {
                    path: client_file.path.clone(),
                    action: SyncAction::Upload,
                    file_id: None,
                    server_hash: None,
                    server_modified_at: None,
                });
            }
            Some((file_id, server_hash, _size, is_deleted, updated_at)) => {
                if *is_deleted {
                    // Server has it deleted, client still has it → upload (client wins)
                    instructions.push(SyncInstruction {
                        path: client_file.path.clone(),
                        action: SyncAction::Upload,
                        file_id: Some(file_id.clone()),
                        server_hash: Some(server_hash.clone()),
                        server_modified_at: Some(*updated_at),
                    });
                } else if client_file.hash != *server_hash {
                    // Different content
                    if client_file.modified_at > *updated_at {
                        // Client is newer → upload
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Upload,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    } else if client_file.modified_at < *updated_at {
                        // Server is newer → download
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Download,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    } else {
                        // Same timestamp but different hash → conflict
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Conflict,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    }
                }
                // If hashes match, file is in sync — no instruction needed
            }
        }
    }

    // Files on server but not on client
    let client_is_empty = client_files.is_empty();

    // Guard against a fresh-install / wiped-vault scenario where the device
    // has an existing sync cursor (e.g. settings copied from another device)
    // but its vault is nearly empty.  If the client reports fewer than 5% of
    // the files this device was expected to know about, treat it as a fresh
    // install and issue Download instructions instead of Deletes.
    let expected_count: i64 = if has_synced_before {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM files WHERE user_id = ? AND is_deleted = FALSE AND updated_at <= ?",
        )
        .bind(user_id)
        .bind(last_known_version)
        .fetch_one(db)
        .await
        .unwrap_or(0)
    } else {
        0
    };
    // Only apply the threshold when the device previously knew about a meaningful
    // number of files (≥ 10) to avoid false positives on small vaults.
    let client_looks_fresh = has_synced_before
        && expected_count >= 10
        && (client_files.len() as i64) * 5 < expected_count; // client has < 20%

    if client_looks_fresh {
        tracing::warn!(
            user_id,
            device_id,
            client_files = client_files.len(),
            expected_files = expected_count,
            "Client manifest is suspiciously small — treating as fresh install, skipping deletes"
        );
    }

    for (path, (file_id, hash, _size, is_deleted, updated_at)) in &server_map {
        if client_paths.contains(path.as_str()) || *is_deleted {
            continue;
        }

        if has_synced_before && !client_is_empty && !client_looks_fresh && *updated_at <= last_known_version {
            // File existed on the server when this device last synced and the device
            // no longer reports it → deleted locally. Mark as deleted on server.
            instructions.push(SyncInstruction {
                path: path.clone(),
                action: SyncAction::Delete,
                file_id: Some(file_id.clone()),
                server_hash: Some(hash.clone()),
                server_modified_at: Some(*updated_at),
            });
        } else {
            // File is new to this device (first sync, empty vault, added after the
            // device last synced, or fresh install detected) → download from server.
            instructions.push(SyncInstruction {
                path: path.clone(),
                action: SyncAction::Download,
                file_id: Some(file_id.clone()),
                server_hash: Some(hash.clone()),
                server_modified_at: Some(*updated_at),
            });
        }
    }

    Ok(instructions)
}
