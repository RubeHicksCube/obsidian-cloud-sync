-- Create vaults table
CREATE TABLE IF NOT EXISTS vaults (
    id         TEXT    NOT NULL,
    user_id    TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (id, user_id)
);

-- Seed existing users with a default vault ("Main Vault")
INSERT OR IGNORE INTO vaults (id, user_id, name)
SELECT 'default', user_id, 'Main Vault'
FROM (SELECT DISTINCT user_id FROM files);

-- Recreate files table to add vault_id and update unique constraint
-- from UNIQUE(user_id, path) to UNIQUE(user_id, vault_id, path)
CREATE TABLE files_new (
    id              TEXT    NOT NULL PRIMARY KEY,
    user_id         TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    vault_id        TEXT    NOT NULL DEFAULT 'default',
    path            TEXT    NOT NULL,
    current_version INTEGER NOT NULL DEFAULT 1,
    hash            TEXT    NOT NULL,
    size            INTEGER NOT NULL,
    is_deleted      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE(user_id, vault_id, path)
);

INSERT INTO files_new (id, user_id, vault_id, path, current_version, hash, size, is_deleted, created_at, updated_at)
SELECT id, user_id, 'default', path, current_version, hash, size, is_deleted, created_at, updated_at
FROM files;

DROP TABLE files;

ALTER TABLE files_new RENAME TO files;

-- Recreate indexes for the new files table (old ones were on the dropped table)
CREATE INDEX IF NOT EXISTS idx_files_user_id ON files(user_id);
CREATE INDEX IF NOT EXISTS idx_files_user_vault_path ON files(user_id, vault_id, path);

-- Add vault_id to file_versions
ALTER TABLE file_versions ADD COLUMN vault_id TEXT NOT NULL DEFAULT 'default';
