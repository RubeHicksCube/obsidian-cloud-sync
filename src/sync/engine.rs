use chrono::Utc;
use sqlx::SqlitePool;

use crate::errors::AppError;
use crate::sync::models::*;

/// Compute the set of sync instructions for a device.
///
/// Design principles:
/// - A file missing from the client manifest is NEVER treated as a deletion.
///   Missing = "device doesn't have it yet" → Download.
/// - Deletions are signalled explicitly via `deleted_paths` (paths the device
///   confirmed it deleted locally since its last sync).
/// - Deletion propagation: if another device deleted a file (server marks
///   is_deleted) and the current device still has it unmodified, the device
///   gets a Delete instruction. If the device modified the file after the
///   server deletion it wins → Upload.
pub async fn compute_delta(
    db: &SqlitePool,
    user_id: &str,
    vault_id: &str,
    client_files: &[FileManifestEntry],
    deleted_paths: &[String],
) -> Result<Vec<SyncInstruction>, AppError> {
    let mut instructions = Vec::new();
    let now = Utc::now().timestamp();

    // ── Step 1: Apply explicit local deletions ────────────────────────────────
    // These are paths the plugin confirmed the user deleted from their vault
    // since the last successful sync. Mark them as soft-deleted on the server
    // so other devices can receive Delete instructions for them.
    for path in deleted_paths {
        sqlx::query(
            "UPDATE files SET is_deleted = TRUE, updated_at = ? \
             WHERE user_id = ? AND vault_id = ? AND path = ? AND is_deleted = FALSE",
        )
        .bind(now)
        .bind(user_id)
        .bind(vault_id)
        .bind(path)
        .execute(db)
        .await?;
    }

    // ── Step 2: Load server state (after applying deletions above) ────────────
    let server_files: Vec<(String, String, String, i64, bool, i64)> = sqlx::query_as(
        "SELECT id, path, hash, size, is_deleted, updated_at FROM files \
         WHERE user_id = ? AND vault_id = ?",
    )
    .bind(user_id)
    .bind(vault_id)
    .fetch_all(db)
    .await?;

    let mut server_map: std::collections::HashMap<String, (String, String, i64, bool, i64)> =
        std::collections::HashMap::new();
    for (id, path, hash, size, is_deleted, updated_at) in &server_files {
        server_map.insert(
            path.clone(),
            (id.clone(), hash.clone(), *size, *is_deleted, *updated_at),
        );
    }

    let client_paths: std::collections::HashSet<&str> =
        client_files.iter().map(|f| f.path.as_str()).collect();

    // ── Step 3: For each file the client reports ──────────────────────────────
    for client_file in client_files {
        match server_map.get(&client_file.path) {
            None => {
                // Client has a file the server has never seen → Upload.
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
                    // Server has the file soft-deleted (deleted by another device or
                    // by this device in a previous sync via deleted_paths).
                    //
                    // Timestamp resolution:
                    //   client modified AFTER server deletion → client wins (Upload)
                    //   client not modified since deletion    → propagate deletion (Delete)
                    if client_file.modified_at > *updated_at {
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Upload,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    } else {
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Delete,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    }
                } else if client_file.hash != *server_hash {
                    // Content differs — resolve by modification timestamp.
                    if client_file.modified_at > *updated_at {
                        // Client is newer → Upload.
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Upload,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    } else if client_file.modified_at < *updated_at {
                        // Server is newer → Download.
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Download,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    } else {
                        // Same timestamp, different hash → Conflict.
                        instructions.push(SyncInstruction {
                            path: client_file.path.clone(),
                            action: SyncAction::Conflict,
                            file_id: Some(file_id.clone()),
                            server_hash: Some(server_hash.clone()),
                            server_modified_at: Some(*updated_at),
                        });
                    }
                }
                // Hashes match and not deleted → already in sync, no instruction needed.
            }
        }
    }

    // ── Step 4: Files on server that the client does NOT have ─────────────────
    // A missing file means the device doesn't have it yet → Download.
    // We NEVER infer deletion from "missing from manifest". Deletions only
    // come from explicit deleted_paths (Step 1) or from the delete API endpoint.
    // Soft-deleted files are skipped — they don't exist from the client's
    // perspective.
    for (path, (file_id, hash, _size, is_deleted, updated_at)) in &server_map {
        if client_paths.contains(path.as_str()) || *is_deleted {
            continue;
        }
        instructions.push(SyncInstruction {
            path: path.clone(),
            action: SyncAction::Download,
            file_id: Some(file_id.clone()),
            server_hash: Some(hash.clone()),
            server_modified_at: Some(*updated_at),
        });
    }

    Ok(instructions)
}
