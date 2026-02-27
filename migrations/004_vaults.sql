-- Create vaults table.
-- vault_id column additions and files table recreation are handled
-- in db.rs (run_vault_migration) to ensure atomicity and recovery from
-- partial execution.
CREATE TABLE IF NOT EXISTS vaults (
    id         TEXT    NOT NULL,
    user_id    TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (id, user_id)
)
