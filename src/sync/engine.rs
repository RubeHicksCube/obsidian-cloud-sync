use sqlx::SqlitePool;

use crate::errors::AppError;
use crate::sync::models::*;

pub async fn compute_delta(
    db: &SqlitePool,
    user_id: &str,
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
                    // Server has it deleted, client still has it → conflict
                    instructions.push(SyncInstruction {
                        path: client_file.path.clone(),
                        action: SyncAction::Conflict,
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

    // Files on server but not on client → download (or delete if client should remove)
    for (path, (file_id, hash, _size, is_deleted, updated_at)) in &server_map {
        if !client_paths.contains(path.as_str()) && !is_deleted {
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
