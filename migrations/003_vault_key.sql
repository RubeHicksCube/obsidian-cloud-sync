-- Encrypted vault passphrase, wrapped client-side with a key derived from
-- the user's login password (PBKDF2). The server stores this opaque blob
-- but cannot decrypt it — only a client that knows the account password can.
-- This allows any device that logs in to auto-configure encryption without
-- the user having to manually enter the vault passphrase on each device.
ALTER TABLE users ADD COLUMN encrypted_vault_key TEXT;
