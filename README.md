# ObsidianCloudSync

A self-hosted sync server for [Obsidian](https://obsidian.md) vaults. Keep your notes synchronized across all your devices without relying on third-party cloud services.

## Features

- **Delta sync** -- only changed files are transferred, not the entire vault
- **End-to-end encryption** -- AES-256-GCM client-side encryption; the server never sees your plaintext notes
- **Version history** -- every file change is versioned with one-click rollback
- **Multi-device** -- sync between desktop, laptop, phone, and tablet
- **Real-time sync** -- WebSocket notifications trigger instant sync when another device pushes changes
- **Web admin panel** -- manage users, devices, files, and settings from your browser
- **Audit logging** -- track logins, uploads, deletions, and admin actions
- **Account security** -- Argon2 password hashing, JWT + refresh token auth, account lockout after failed logins, constant-time token comparison
- **Storage quotas** -- configurable per-user storage limits with automatic version pruning
- **Docker-ready** -- single `docker compose up` with health checks and non-root container

## Quick Start

**1. Clone and configure**

```bash
git clone https://github.com/RubeHicksCube/obsidian-cloud-sync.git
cd obsidian-cloud-sync
cp .env.example .env
```

**2. Set a JWT secret** (required)

```bash
# Generate a random secret
openssl rand -base64 32

# Paste it into .env as JWT_SECRET=<your-secret>
```

**3. Start the server**

```bash
docker compose up -d
```

**4. Verify**

```bash
curl http://localhost:8443/api/health
# ok
```

**5. Register** -- open `http://localhost:8443` in your browser. The first user is automatically admin.

**6. Install the plugin** -- see [Obsidian Plugin](#obsidian-plugin) below.

For detailed setup instructions including building from source, reverse proxy (nginx/Caddy), systemd service, and production configuration, see the **[Setup Guide](SETUP_GUIDE.md)**.

## Obsidian Plugin

The companion Obsidian plugin connects your vault to the server:

**Repository:** [RubeHicksCube/obsidian-cloudsync-plugin](https://github.com/RubeHicksCube/obsidian-cloudsync-plugin)

### Install

```bash
cd /path/to/your/vault/.obsidian/plugins
git clone https://github.com/RubeHicksCube/obsidian-cloudsync-plugin.git obsidian-cloudsync
cd obsidian-cloudsync
npm install && npm run build
```

Restart Obsidian, then enable **CloudSync** in Settings > Community Plugins.

### Configure

In Settings > CloudSync:

| Setting | Value |
|---|---|
| **Server URL** | `http://localhost:8443` or `https://sync.yourdomain.com` |
| **Username / Password** | Your registered credentials |
| **Encryption Passphrase** | A strong passphrase (must be identical on all devices) |
| **Auto-sync Interval** | `5` minutes (or `0` to disable) |

Click **Login**, and the plugin will start syncing.

### Plugin Features

- Automatic and manual sync with delta diffing
- Client-side AES-256-GCM encryption (PBKDF2 key derivation)
- WebSocket real-time sync notifications
- Configurable exclude patterns (glob syntax)
- Conflict detection with side-by-side file preservation
- Sync progress in the status bar
- Passphrase change with automatic full re-encryption

## How Sync Works

```
  Obsidian Plugin                         Server
  ===============                         ======

  1. Send file manifest          --->     /api/sync/delta
     (paths, hashes, timestamps)

  2.                              <---    Instructions:
                                          upload / download / conflict

  3. Upload changed files        --->     /api/sync/upload
     (encrypted, multipart)

  4. Download changed files      <---     /api/sync/download/{id}

  5. Mark sync complete          --->     /api/sync/complete
```

Files are hashed before encryption so the server can compare manifests across devices without seeing file contents.

## Configuration

All settings are environment variables (`.env` file or docker-compose `environment`):

| Variable | Default | Description |
|---|---|---|
| `BIND_ADDRESS` | `0.0.0.0:8443` | Listen address and port |
| `JWT_SECRET` | *(required)* | Secret for signing auth tokens |
| `REGISTRATION_OPEN` | `true` | Allow self-registration |
| `MAX_UPLOAD_SIZE_MB` | `100` | Max single file upload size |
| `MAX_STORAGE_PER_USER_MB` | `5000` | Per-user storage quota |
| `MAX_VERSIONS_PER_FILE` | `50` | Version history depth |
| `VERSION_RETENTION_DAYS` | `90` | Max age of old versions |
| `LOCKOUT_THRESHOLD` | `5` | Failed logins before lockout |
| `LOCKOUT_DURATION_SECS` | `900` | Lockout duration (15 min) |
| `REQUIRE_ENCRYPTION` | `false` | Reject unencrypted uploads |
| `RUST_LOG` | `info` | Log verbosity |

See **[Setup Guide](SETUP_GUIDE.md)** for the full reference and production recommendations.

## Architecture

- **Server**: Rust + Axum, SQLite (WAL mode), content-addressed blob storage
- **Auth**: JWT access tokens (15 min) + rotating SHA-256 hashed refresh tokens (30 days)
- **Encryption**: Client-side AES-256-GCM with PBKDF2 key derivation (server stores only ciphertext)
- **Real-time**: WebSocket broadcast channels notify connected devices of changes
- **Background tasks**: Token cleanup, version pruning, blob garbage collection
- **Web UI**: Embedded SPA for admin management (users, files, devices, audit log, settings)

## License

MIT
