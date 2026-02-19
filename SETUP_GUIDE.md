# ObsidianCloudSync Setup Guide

---

## Table of Contents

1. [Overview](#1-overview)
2. [Server Setup](#2-server-setup)
   - [Option A: Docker (Recommended)](#option-a-docker-recommended)
   - [Option B: Build from Source](#option-b-build-from-source)
3. [Server Configuration](#3-server-configuration)
4. [Creating Your Account](#4-creating-your-account)
5. [Installing the Obsidian Plugin](#5-installing-the-obsidian-plugin)
6. [Your First Sync](#6-your-first-sync)
7. [Syncing Multiple Devices](#7-syncing-multiple-devices)
8. [Web Admin Panel](#8-web-admin-panel)
9. [Troubleshooting](#9-troubleshooting)
10. [Security Notes](#10-security-notes)

---

## 1. Overview

### What ObsidianCloudSync Does

ObsidianCloudSync is a self-hosted sync server for [Obsidian](https://obsidian.md) vaults. It lets you keep your notes synchronized across all of your devices -- your laptop, your phone, your desktop at work -- without relying on a third-party cloud service. You run the server on your own hardware (a home server, a VPS, a Raspberry Pi), and the Obsidian plugin on each device connects to it to push and pull changes.

Because you host everything yourself, you stay in full control of your data. The server stores encrypted file blobs in content-addressed storage, keeps a full version history of every file, and provides a web-based admin panel for managing users, devices, and files.

### How It Works

The sync process follows a simple request-response cycle:

```
 Your Device (Obsidian Plugin)                    Your Server
 ==============================                   ===========

 1. "Here is a list of my files             --->   /api/sync/delta
     and their hashes."

 2.                                          <---   "Upload these files.
                                                     Download these files.
                                                     These files are in conflict."

 3. Upload changed files                    --->   /api/sync/upload
    (multipart, encrypted bytes)

 4. Download changed files                  <---   /api/sync/download/{id}
    (encrypted bytes)

 5. "Sync is complete."                     --->   /api/sync/complete
                                                   (server records a sync cursor)
```

**Delta sync** means the plugin only sends a manifest of file paths, hashes, and timestamps. The server compares these against its records and tells the plugin exactly which files need to be uploaded and which need to be downloaded. Only the files that have actually changed are transferred.

### What You Need Before Starting

- A machine to run the server (Linux, macOS, or Windows; a VPS with 512 MB of RAM is sufficient)
- **Docker** and **Docker Compose** installed (for the recommended setup path), OR **Rust 1.75+** (for building from source)
- A terminal application (Terminal on macOS/Linux, PowerShell or WSL on Windows)
- Obsidian installed on at least one device
- About 10 minutes

---

## 2. Server Setup

### Option A: Docker (Recommended)

This is the simplest way to get started. Docker handles all dependencies for you.

**Step 1.** Make sure Docker and Docker Compose are installed.

```bash
docker --version
docker compose version
```

You should see version numbers for both. If not, install Docker from [https://docs.docker.com/get-docker/](https://docs.docker.com/get-docker/).

Expected output:

```
Docker version 27.x.x, build xxxxxxx
Docker Compose version v2.x.x
```

**Step 2.** Clone the repository (or copy the project files to your server).

```bash
git clone https://github.com/RubeHicksCube/obsidian-cloud-sync.git
cd obsidian-cloud-sync
```

**Step 3.** Generate a secure JWT secret. This is the cryptographic key the server uses to sign authentication tokens. It must be random and kept private.

```bash
openssl rand -base64 32
```

Expected output (yours will differ -- this is random):

```
kG7vYp2xNm3Qf8aR1bWzLc4dJe5hTu6iXo9sPw0qDr=
```

Copy this value. You will use it in the next step.

**Step 4.** Create a `.env` file in the project root with your JWT secret.

```bash
cp .env.example .env
```

Now open `.env` in a text editor and replace `change-me-to-a-random-secret` with the value you generated:

```
BIND_ADDRESS=0.0.0.0:8443
DATABASE_URL=sqlite:data/obsidian_sync.db
DATA_DIR=data
JWT_SECRET=kG7vYp2xNm3Qf8aR1bWzLc4dJe5hTu6iXo9sPw0qDr=
ACCESS_TOKEN_EXPIRY_SECS=900
REFRESH_TOKEN_EXPIRY_DAYS=30
MAX_UPLOAD_SIZE_MB=100
REGISTRATION_OPEN=true
RUST_LOG=info
```

> **Warning:** Never share your JWT secret or commit it to a public repository. Anyone with this value can forge authentication tokens for your server.

**Step 5.** Start the server.

```bash
docker compose up -d
```

Expected output:

```
[+] Building ...
[+] Running 1/1
 ✔ Container obsidian-cloud-sync-obsidian-sync-1  Started
```

The first build will take a few minutes while Rust compiles the project inside the Docker container. Subsequent starts will be nearly instant.

**Step 6.** Verify the server is running.

```bash
curl http://localhost:8443/api/health
```

Expected output:

```
ok
```

If you see `ok`, your server is running and ready to accept connections.

**Step 7.** Check the logs to confirm everything started cleanly.

```bash
docker compose logs -f
```

Expected output:

```
obsidian-sync-1  | 2025-01-15T10:30:00.000Z  INFO obsidian_cloud_sync: ObsidianCloudSync listening on 0.0.0.0:8443
```

Press `Ctrl+C` to stop following the logs (the server continues running in the background).

---

### Option B: Build from Source

Use this method if you prefer not to use Docker or want to run the binary directly.

**Step 1.** Install the Rust toolchain.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts to complete the installation, then restart your terminal or run:

```bash
source "$HOME/.cargo/env"
```

Verify:

```bash
rustc --version
```

Expected output:

```
rustc 1.75.0 (or newer)
```

**Step 2.** Clone the repository.

```bash
git clone https://github.com/RubeHicksCube/obsidian-cloud-sync.git
cd obsidian-cloud-sync
```

**Step 3.** Create and configure the `.env` file (same as Docker Step 3 and Step 4 above).

```bash
cp .env.example .env
```

Edit `.env` and set a secure `JWT_SECRET` (see the `openssl rand -base64 32` command in Option A, Step 3).

**Step 4.** Build the project in release mode.

```bash
cargo build --release
```

This will take 2-5 minutes the first time. Expected output ends with:

```
   Compiling obsidian-cloud-sync v0.1.0
    Finished `release` profile [optimized] target(s) in 2m 30s
```

**Step 5.** Create the data directory and run the server.

```bash
mkdir -p data
./target/release/obsidian-cloud-sync
```

Expected output:

```
2025-01-15T10:30:00.000Z  INFO obsidian_cloud_sync: ObsidianCloudSync listening on 0.0.0.0:8443
```

**Step 6.** Verify (in a separate terminal):

```bash
curl http://localhost:8443/api/health
```

Expected output:

```
ok
```

> **Note:** If you want the server to run in the background and survive reboots, set it up as a systemd service. See the [Running as a systemd service](#running-as-a-systemd-service) note at the end of this section.

#### Running as a systemd service

Create the file `/etc/systemd/system/obsidian-cloud-sync.service`:

```ini
[Unit]
Description=ObsidianCloudSync Server
After=network.target

[Service]
Type=simple
User=your-username
WorkingDirectory=/path/to/obsidian-cloud-sync
EnvironmentFile=/path/to/obsidian-cloud-sync/.env
ExecStart=/path/to/obsidian-cloud-sync/target/release/obsidian-cloud-sync
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Then enable and start it:

```bash
sudo systemctl daemon-reload
sudo systemctl enable obsidian-cloud-sync
sudo systemctl start obsidian-cloud-sync
sudo systemctl status obsidian-cloud-sync
```

---

## 3. Server Configuration

All configuration is done through environment variables, set either in your `.env` file or in the `docker-compose.yml` `environment` section.

### Environment Variable Reference

| Variable | Default | Description |
|---|---|---|
| `BIND_ADDRESS` | `0.0.0.0:8443` | The address and port the server listens on. `0.0.0.0` means it accepts connections from any network interface. Change the port number if `8443` is already in use on your machine. |
| `DATABASE_URL` | `sqlite:data/obsidian_sync.db` | Path to the SQLite database file. The `sqlite:` prefix is required. The database is created automatically on first run. |
| `DATA_DIR` | `data` | Directory where uploaded file blobs are stored. This is where your vault data lives on disk. Make sure this directory has enough free space for all your vaults. |
| `JWT_SECRET` | *(required)* | A random string used to sign authentication tokens. **Must be set before starting the server.** If you change this after users have logged in, all existing sessions will be invalidated and everyone will need to log in again. |
| `ACCESS_TOKEN_EXPIRY_SECS` | `900` | How long an access token is valid, in seconds. The default of 900 seconds (15 minutes) is a good balance between security and convenience. The plugin refreshes tokens automatically, so users will not notice short expiry times. |
| `REFRESH_TOKEN_EXPIRY_DAYS` | `30` | How long a refresh token is valid, in days. After this many days without any sync activity, the device will need to log in again. |
| `MAX_UPLOAD_SIZE_MB` | `100` | Maximum size of a single file upload in megabytes. If you have very large attachments (videos, PDFs), increase this value. |
| `REGISTRATION_OPEN` | `true` | Whether new users can create accounts through the web UI or API. Set to `false` after creating your account if you want a private server. The first user can always register regardless of this setting. |
| `RUST_LOG` | `info` | Controls how much detail appears in the server logs. Options: `error` (least), `warn`, `info` (recommended), `debug`, `trace` (most). Use `debug` when troubleshooting issues. |

### Recommended Production Values

For a server exposed to the internet:

```
BIND_ADDRESS=0.0.0.0:8443
DATABASE_URL=sqlite:data/obsidian_sync.db
DATA_DIR=data
JWT_SECRET=<your-64-character-random-string>
ACCESS_TOKEN_EXPIRY_SECS=900
REFRESH_TOKEN_EXPIRY_DAYS=30
MAX_UPLOAD_SIZE_MB=100
REGISTRATION_OPEN=false
RUST_LOG=info
```

> **Note:** Set `REGISTRATION_OPEN=true` for your initial setup so you can create your account, then change it to `false` and restart the server to lock down registration.

### Generating a Secure JWT Secret

Use one of these commands to generate a cryptographically secure random string:

```bash
# Option 1: OpenSSL (most systems)
openssl rand -base64 32

# Option 2: /dev/urandom (Linux/macOS)
head -c 32 /dev/urandom | base64

# Option 3: Python (if installed)
python3 -c "import secrets; print(secrets.token_urlsafe(32))"
```

Any of these will produce a string like:

```
a4Bf9cKm2xNqPw7vYz1dGhJi3eLr5tOu8sSn0wXj6bQ=
```

### Setting Up Behind a Reverse Proxy (nginx)

If you are running your server on a VPS and want to access it through a domain name with HTTPS (which you should for production use), place nginx in front of ObsidianCloudSync.

**Step 1.** Install nginx and Certbot.

```bash
# Debian/Ubuntu
sudo apt install nginx certbot python3-certbot-nginx
```

**Step 2.** Create an nginx configuration file at `/etc/nginx/sites-available/obsidian-sync`:

```nginx
server {
    listen 80;
    server_name sync.yourdomain.com;

    # Redirect HTTP to HTTPS (Certbot will handle this,
    # but this is a safety net)
    location / {
        return 301 https://$server_name$request_uri;
    }
}

server {
    listen 443 ssl http2;
    server_name sync.yourdomain.com;

    # Certbot will fill in the certificate paths automatically.
    # ssl_certificate /etc/letsencrypt/live/sync.yourdomain.com/fullchain.pem;
    # ssl_certificate_key /etc/letsencrypt/live/sync.yourdomain.com/privkey.pem;

    client_max_body_size 100M;

    location / {
        proxy_pass http://127.0.0.1:8443;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

**Step 3.** Enable the site and obtain an SSL certificate.

```bash
sudo ln -s /etc/nginx/sites-available/obsidian-sync /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
sudo certbot --nginx -d sync.yourdomain.com
```

Follow the Certbot prompts. Once complete, your server will be accessible at `https://sync.yourdomain.com`.

**Step 4.** Verify HTTPS is working.

```bash
curl https://sync.yourdomain.com/api/health
```

Expected output:

```
ok
```

> **Note:** Make sure `client_max_body_size` in your nginx config matches or exceeds your `MAX_UPLOAD_SIZE_MB` setting. Otherwise, large file uploads will be rejected by nginx before they reach your server.

### Enabling HTTPS Without a Reverse Proxy

ObsidianCloudSync itself does not handle TLS termination. For HTTPS you need either:

1. A reverse proxy like nginx (described above) -- **recommended**
2. A TLS-terminating load balancer (e.g., Caddy, Traefik, or a cloud load balancer)

Caddy is a particularly simple alternative to nginx. A complete `Caddyfile` would be:

```
sync.yourdomain.com {
    reverse_proxy localhost:8443
}
```

Caddy automatically obtains and renews HTTPS certificates from Let's Encrypt.

---

## 4. Creating Your Account

### Registering the First User (Admin)

The very first user to register on a fresh server is automatically granted **admin** privileges. This happens regardless of the `REGISTRATION_OPEN` setting, so you can always create the first account.

**Step 1.** Open your browser and go to your server's address:

- Local: `http://localhost:8443`
- With domain: `https://sync.yourdomain.com`

You will see the ObsidianCloudSync login page with a heading that says **"ObsidianCloudSync"** and a subtitle **"Sign in to your server"**.

**Step 2.** Click the **"Register"** link below the sign-in form. The page switches to the registration form.

**Step 3.** Fill in the form:

- **Username**: Choose a username (at least 3 characters)
- **Email**: Optional; enter your email if you want it stored for account recovery purposes
- **Password**: Choose a strong password (at least 8 characters)

**Step 4.** Click **"Create Account"**.

If registration succeeds, you will be signed in immediately and taken to the **Dashboard**. Because you are the first user, you are now the admin.

The dashboard shows four statistic cards:

- **Files**: 0 (no files synced yet)
- **Storage Used**: 0 B
- **Active Devices**: 1 (the web browser you just registered from)
- **Total Devices**: 1

The left sidebar shows navigation links: **Dashboard**, **Files**, **Devices**, **Users** (admin only), and **Settings** (admin only).

> **Note:** After creating your admin account, you may want to close registration by going to **Settings** in the sidebar and changing **Registration Open** to **Closed**, then clicking **Save Settings**. This prevents anyone else from creating accounts on your server.

### Creating Additional User Accounts

There are two ways to add more users:

**Method 1: Self-registration (if registration is open)**

Share your server URL with the person. They can click "Register" on the login page and create their own account. Their account will be a standard user (not admin).

**Method 2: Admin creates the account**

1. Log in to the web panel as an admin
2. Click **Users** in the sidebar
3. Click the **"Create User"** button in the top right
4. Fill in the username, password, and optionally email
5. Check the **Admin** checkbox if you want them to have admin access
6. Click **"Create"**

The new user will appear in the users table. Share the username and password with them.

---

## 5. Installing the Obsidian Plugin

### Finding and Installing the Plugin

The CloudSync plugin is not yet available in the Obsidian Community Plugins directory. You need to install it manually.

**Step 1.** Find your Obsidian vault's plugin directory. Open Obsidian, then:

1. Open **Settings** (gear icon in the bottom-left corner)
2. Go to **Community plugins**
3. If prompted, click **"Turn on community plugins"**
4. Click the folder icon next to "Installed plugins" or navigate manually to your vault's `.obsidian/plugins/` directory

The path will look something like:

- **macOS**: `~/Documents/MyVault/.obsidian/plugins/`
- **Linux**: `~/Documents/MyVault/.obsidian/plugins/`
- **Windows**: `C:\Users\YourName\Documents\MyVault\.obsidian\plugins\`

**Step 2.** Create the plugin directory.

```bash
mkdir -p /path/to/your/vault/.obsidian/plugins/obsidian-cloudsync
```

**Step 3.** Copy the plugin files into the directory. You need three files:

- `main.js` (the compiled plugin code)
- `manifest.json` (plugin metadata)
- `styles.css` (if provided)

If you are building from the plugin source repository:

```bash
cd /path/to/obsidian-cloudsync-plugin
npm install
npm run build
cp main.js manifest.json /path/to/your/vault/.obsidian/plugins/obsidian-cloudsync/
```

**Step 4.** Restart Obsidian or reload plugins. Go to **Settings > Community plugins** and you should see **CloudSync** listed. Toggle the switch to enable it.

### Configuring the Plugin Settings

After enabling the plugin, open its settings: **Settings > CloudSync** (listed under "Community plugins" in the sidebar).

You will need to configure these settings:

| Setting | What to enter | Explanation |
|---|---|---|
| **Server URL** | `https://sync.yourdomain.com` or `http://localhost:8443` | The full URL of your ObsidianCloudSync server. Use `https://` if you set up a reverse proxy with SSL. Use `http://` only for local testing. |
| **Username** | Your username | The username you registered in the web panel. |
| **Password** | Your password | The password for your account. This is used to authenticate and obtain tokens. The plugin stores tokens locally and does not send your password after the initial login. |
| **Encryption Passphrase** | A strong passphrase | Used to encrypt your vault files before they leave your device. The server never sees your unencrypted data. **You must use the same passphrase on every device.** |
| **Auto-sync Interval** | `5` (minutes) | How often the plugin automatically checks for changes and syncs. Set to `0` to disable auto-sync (manual only). |
| **Sync on Startup** | Enabled | Whether to trigger a sync when Obsidian opens. Recommended for most users. |

> **Warning:** Your encryption passphrase is critical. If you lose it, your synced files cannot be decrypted. The server stores only encrypted blobs and has no way to recover your data without the passphrase. Write it down and store it somewhere safe.

> **Warning:** All devices syncing the same vault **must** use the **exact same encryption passphrase**. If one device uses a different passphrase, it will not be able to decrypt files uploaded by the other devices, resulting in corrupted data.

### What You Should See After Connecting

After entering your settings and saving:

1. The plugin will attempt to connect to your server
2. A status indicator will appear in the Obsidian status bar (bottom of the window)
3. If the connection is successful, you will see a status like **"CloudSync: Connected"** or a sync icon
4. If there is an error, check the Obsidian developer console (`Ctrl+Shift+I` or `Cmd+Option+I`) for detailed error messages

---

## 6. Your First Sync

### Step-by-Step

**Step 1.** Make sure your server is running and the plugin is configured (Sections 2-5).

**Step 2.** Trigger a manual sync. You can do this by:

- Using the command palette: press `Ctrl+P` (or `Cmd+P` on macOS), then type **"CloudSync"** and select **"CloudSync: Sync now"**
- Clicking the sync icon in the status bar (if the plugin provides one)

**Step 3.** The plugin sends a **delta request** to the server. This is a manifest listing every file in your vault along with its hash (a fingerprint of its contents) and last-modified timestamp.

**Step 4.** The server compares the manifest against its records and responds with **sync instructions**:

- **Upload**: Files that exist on your device but not on the server (all files on your first sync)
- **Download**: Files that exist on the server but not on your device (none on your first sync)
- **Conflict**: Files that were changed in both places since the last sync

**Step 5.** The plugin uploads all files marked for upload. Each file is encrypted on your device before being sent to the server. The server stores the encrypted bytes in content-addressed blob storage (files are deduplicated by their hash, so identical files are stored only once).

**Step 6.** When all uploads and downloads are complete, the plugin sends a **sync complete** request. The server records a sync cursor for your device, so the next sync can be incremental.

### Expected Results

After your first sync completes:

- The status bar should show something like **"CloudSync: Synced"** with a timestamp
- In the **web admin panel**, navigate to **Files** in the sidebar. You should see a table listing every file from your vault, including:
  - The file path (e.g., `Daily Notes/2025-01-15.md`)
  - File size
  - Version number (v1 for all files after the first sync)
  - Last updated timestamp
- The **Dashboard** should now show the number of files and total storage used

### Verifying via the Web Panel

1. Open your server in a browser: `https://sync.yourdomain.com`
2. Log in with your credentials
3. Click **Files** in the sidebar
4. You should see your vault files listed in a table
5. Click **"Versions"** next to any file to see its version history (one entry, v1, after the first sync)

---

## 7. Syncing Multiple Devices

### Setting Up a Second Device

**Step 1.** Install Obsidian on your second device (phone, tablet, second computer).

**Step 2.** Install the CloudSync plugin on the second device (same process as Section 5).

**Step 3.** Configure the plugin settings on the second device:

- **Server URL**: Same server URL as your first device
- **Username**: Same username
- **Password**: Same password
- **Encryption Passphrase**: **The exact same passphrase as your first device**
- **Auto-sync Interval**: Same or different, your choice

> **Warning:** Using the same encryption passphrase on all devices is **critical**. This is not optional. If one device has a different passphrase, it will encrypt files with a different key, and your other devices will not be able to read those files. The result will be corrupted, unreadable notes.

**Step 4.** Trigger a sync on the second device. Since the server already has your vault from the first device, the delta response will instruct the second device to **download** all files. After the sync completes, your full vault will appear on the second device.

### Expected Behavior

Once both devices are syncing:

1. **Edit a note on Device A**. The next time Device A syncs, the changed file is uploaded to the server.
2. **Device B syncs**. The server tells Device B to download the updated file. The file appears on Device B with the latest changes.
3. This works in both directions. Changes on Device B flow to Device A the same way.

Sync is **not real-time**. It happens on the auto-sync interval you configured (e.g., every 5 minutes) or when you manually trigger a sync. There may be a short delay between making a change on one device and seeing it on another.

### Handling Conflicts

If you edit the same file on two devices before either syncs, a **conflict** will occur. The sync engine detects this when:

- Both devices have a different hash for the same file
- Both devices have the same last-modified timestamp (meaning the server cannot determine which version is newer)

When a conflict is detected, the plugin will typically keep both versions: the server's version and your local version (saved with a conflict suffix like `filename (conflict).md`). You can then manually merge the changes.

To minimize conflicts:

- Sync frequently (set a short auto-sync interval)
- Avoid editing the same file on two devices at the same time
- Sync before switching between devices

---

## 8. Web Admin Panel

The web admin panel is built into the server and accessible at your server's root URL (`https://sync.yourdomain.com` or `http://localhost:8443`).

### Dashboard

The dashboard is the first page you see after logging in. It displays four summary cards:

- **Files**: Total number of synced files (excluding deleted files)
- **Storage Used**: Total size of all synced files
- **Active Devices**: Number of devices that have not been revoked
- **Total Devices**: Total devices ever registered (including revoked)

A **Sign Out** button is in the top-right corner of the page.

### Files

The **Files** page shows a table of all synced files belonging to your account:

| Column | Description |
|---|---|
| **Path** | The file's path within your vault (e.g., `Folder/Subfolder/Note.md`) |
| **Size** | The file's size in human-readable format (KB, MB) |
| **Version** | The current version number (v1, v2, v3, etc.) |
| **Updated** | When the file was last modified |
| **Versions** | A link to view the file's version history |

#### Version History

Clicking **"Versions"** on any file shows a chronological list of every version that has been saved:

| Column | Description |
|---|---|
| **Version** | The version number |
| **Hash** | A truncated SHA-256 hash (content fingerprint) |
| **Size** | File size at that version |
| **Created** | When that version was uploaded |
| **Rollback** | A button to restore the file to that version |

#### Rolling Back to a Previous Version

1. Navigate to **Files** and click **"Versions"** next to the file
2. Find the version you want to restore
3. Click **"Rollback"** next to that version
4. Confirm the rollback in the dialog that appears

The server will create a **new** version (e.g., v5) that has the same content as the old version you selected (e.g., v2). The version history is preserved -- nothing is deleted. On the next sync, your devices will download the rolled-back content.

### Devices

The **Devices** page shows all devices linked to your account:

| Column | Description |
|---|---|
| **Name** | The device name (e.g., "Web Admin", "MacBook Pro", "iPhone") |
| **Type** | The device type (web, desktop, mobile) |
| **Status** | Active (green badge) or Revoked (red badge) |
| **Last Seen** | When the device last contacted the server |
| **Created** | When the device first logged in |
| **Revoke** | A button to revoke the device (only shown for active devices) |

#### Revoking a Device

If a device is lost, stolen, or you simply want to disconnect it:

1. Navigate to **Devices**
2. Click **"Revoke"** next to the device
3. Confirm the action

Revoking a device:

- Immediately invalidates all of that device's authentication tokens
- The device will be signed out and unable to sync until it logs in again
- You cannot revoke the device you are currently using (the web session)
- Revoked devices remain in the list with a "Revoked" badge for your records

### Users (Admin Only)

This page is only visible to admin users. It shows a table of all registered users:

| Column | Description |
|---|---|
| **Username** | The user's login name |
| **Email** | Their email address (if provided) |
| **Role** | Admin (yellow badge) or User (green badge) |
| **Files** | Number of files they have synced |
| **Devices** | Number of active (non-revoked) devices |
| **Created** | When the account was created |
| **Delete** | A button to permanently delete the user |

**Creating a new user**: Click the **"Create User"** button. A modal dialog appears where you enter a username, password, optional email, and whether the new user should be an admin.

**Deleting a user**: Click **"Delete"** and confirm. This permanently removes the user and all of their associated data (files, file versions, devices, sync cursors). This action cannot be undone.

### Settings (Admin Only)

The settings page lets admins configure server-wide options:

| Setting | Description |
|---|---|
| **Max Versions Per File** | How many old versions to retain per file. Default: 50. Older versions beyond this limit may be pruned. |
| **Max Version Age (days)** | How many days to keep old file versions. Default: 90. Versions older than this may be pruned. |
| **Registration Open** | Whether new users can self-register. Set to **Closed** to prevent new signups. |

Click **"Save Settings"** to apply changes. A green confirmation message appears on success.

### Theme Toggle

A theme toggle button is in the bottom-left corner of the sidebar (sun/moon icon). The panel supports both **dark** and **light** themes. Your preference is saved in your browser.

---

## 9. Troubleshooting

### Connection Refused

**Symptom**: The plugin shows a connection error, or `curl http://localhost:8443/api/health` fails with "Connection refused."

**Causes and fixes**:

1. **The server is not running.**
   ```bash
   # Docker:
   docker compose ps

   # If the container is stopped:
   docker compose up -d

   # Native:
   # Check if the process is running
   ps aux | grep obsidian-cloud-sync
   ```

2. **The port is wrong or blocked by a firewall.**
   ```bash
   # Check what port the server is listening on
   docker compose logs | grep "listening on"

   # Check if the port is open
   ss -tlnp | grep 8443
   ```

3. **You are connecting from a remote machine but the server is bound to `127.0.0.1` instead of `0.0.0.0`.**
   Check your `BIND_ADDRESS` in `.env`. It must be `0.0.0.0:8443` (not `127.0.0.1:8443`) to accept connections from other machines.

### Authentication Errors (401 Unauthorized)

**Symptom**: The plugin or web UI shows "Invalid credentials" or "Unauthorized."

**Causes and fixes**:

1. **Wrong username or password.** Double-check your credentials. Passwords are case-sensitive.

2. **Expired tokens.** If the plugin has not synced in longer than `REFRESH_TOKEN_EXPIRY_DAYS` (default 30 days), the refresh token has expired. Log out and log back in.

3. **JWT secret was changed.** If you changed the `JWT_SECRET` in your `.env` file and restarted the server, all existing tokens are now invalid. Every user and device needs to log in again.

4. **Device was revoked.** Check the Devices page in the web panel. If the device shows "Revoked", it cannot authenticate. The user must log in again to create a new device session.

### Registration Closed (403 Forbidden)

**Symptom**: You try to register a new account and see "Registration is closed."

**Fix**: An admin needs to either:
- Open the web admin panel, go to **Settings**, and change **Registration Open** to **Open**
- Or create the user account manually from the **Users** page

### Sync Conflicts

**Symptom**: After syncing, you see duplicate files with "conflict" in the name.

**Explanation**: This happens when the same file was edited on two devices before either synced. The sync engine could not automatically determine which version was correct.

**Fix**:
1. Open both the original file and the conflict file
2. Manually merge any differences
3. Delete the conflict file
4. Sync again

### Large File Upload Fails

**Symptom**: Uploading a large file (attachment, image, PDF) fails.

**Causes and fixes**:

1. **File exceeds `MAX_UPLOAD_SIZE_MB`.** The default is 100 MB. If you need to sync larger files, increase this value in your `.env` file and restart the server.

2. **Reverse proxy is rejecting the request.** If you are using nginx, make sure `client_max_body_size` is set high enough in your nginx config (see Section 3).

### How to Check Server Logs

Server logs are the best place to diagnose issues.

```bash
# Docker: view live logs
docker compose logs -f

# Docker: view the last 100 lines
docker compose logs --tail=100

# Native (if running in foreground): logs print to your terminal

# Native (if running as systemd service):
sudo journalctl -u obsidian-cloud-sync -f
```

For more detailed logs, set `RUST_LOG=debug` in your `.env` file and restart the server:

```bash
# Docker:
docker compose down && docker compose up -d

# Native:
# Stop the server (Ctrl+C), edit .env, restart
```

### How to Reset If Something Goes Wrong

If you need to start over completely:

> **Warning:** This will delete **all** data including user accounts, files, and version history. Only do this as a last resort.

```bash
# Docker:
docker compose down
rm -rf data/
docker compose up -d

# Native:
# Stop the server
rm -rf data/
# Restart the server
```

The server will create a fresh database on the next startup. You will need to register a new account.

To reset only the database (keeping file blobs):

```bash
rm data/obsidian_sync.db
# Restart the server
```

---

## 10. Security Notes

### Client-Side Encryption

When you set an encryption passphrase in the Obsidian plugin, your files are encrypted **on your device** before being sent to the server. The server receives and stores only encrypted data.

**What the server can see**:
- File paths (filenames and folder structure)
- File sizes
- When files were last modified
- Which device uploaded each file
- File hashes (content fingerprints, used for deduplication and delta sync)

**What the server cannot see**:
- The actual content of your notes
- The text, images, or data inside your files
- Your encryption passphrase

This means that even if someone gains access to your server, they cannot read your notes without your encryption passphrase. However, they can see the file paths, which may reveal information about the structure and topics of your vault.

### Importance of the Encryption Passphrase

- Your passphrase is **never sent to the server**. It is used only on your devices to encrypt and decrypt files.
- If you **forget** your passphrase, there is no way to recover your data from the server. The encrypted blobs are useless without the key.
- If you **change** your passphrase, you must re-encrypt and re-upload all files. This effectively means performing a fresh full sync.
- **All devices** sharing a vault must use the **exact same passphrase**. There is no way around this.

### Password Security

- Passwords are hashed on the server using **Argon2**, one of the strongest password-hashing algorithms available. Even if the database is compromised, passwords cannot be easily recovered.
- Usernames must be at least 3 characters, and passwords must be at least 8 characters. Use a longer password for better security.

### Token Security

- **Access tokens** are short-lived JWTs (default: 15 minutes). They are signed using the `JWT_SECRET` and contain the user ID, device ID, and admin status.
- **Refresh tokens** are random strings stored as SHA-256 hashes in the database. They last 30 days by default and are automatically rotated: each time a refresh token is used, the old one is deleted and a new one is issued.
- Revoking a device deletes all of its refresh tokens and prevents it from obtaining new access tokens.

### Backup Recommendations

1. **Back up the `data/` directory regularly.** This contains your SQLite database and all encrypted file blobs. A simple `rsync` or `cp -a` of this directory is sufficient.

   ```bash
   # Example: back up to an external drive
   rsync -av /path/to/obsidian-cloud-sync/data/ /mnt/backup/obsidian-sync-data/
   ```

2. **Back up your `.env` file** (especially the `JWT_SECRET`). If you lose this file, you will need to generate a new secret, which will invalidate all sessions.

3. **Keep a local copy of your vault.** The sync server is not a backup. If the server fails and you have no backups, you still have your vault on your devices. But if all devices and the server are lost, you need an independent backup.

4. **Record your encryption passphrase** somewhere safe (a password manager, a physical note in a secure location). Without it, the server-side data is unrecoverable.

### Network Security

- Always use **HTTPS** in production. Without it, your authentication tokens and encrypted file data travel over the network in plain text, making them vulnerable to interception.
- The server enables **CORS** with an open policy (`allow_origin: Any`) so that the web panel and plugins from any origin can connect. If you want to restrict this, you would need to modify the source code.
- The server uses **gzip compression** for responses to reduce bandwidth usage.

---

*ObsidianCloudSync v0.1.0*
