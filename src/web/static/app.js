// ObsidianCloudSync — Admin SPA
const API = '/api';
let state = {
  token: localStorage.getItem('token'),
  refreshToken: localStorage.getItem('refreshToken'),
  userId: localStorage.getItem('userId'),
  isAdmin: localStorage.getItem('isAdmin') === 'true',
  username: localStorage.getItem('username'),
};

// --- Helpers ---
function $(sel, ctx) { return (ctx || document).querySelector(sel); }
function $$(sel, ctx) { return [...(ctx || document).querySelectorAll(sel)]; }

function setTheme(t) {
  document.documentElement.setAttribute('data-theme', t);
  localStorage.setItem('theme', t);
}
(function initTheme() {
  const saved = localStorage.getItem('theme') || 'dark';
  setTheme(saved);
})();

async function api(path, opts = {}) {
  const headers = { ...(opts.headers || {}) };
  if (state.token) headers['Authorization'] = `Bearer ${state.token}`;
  if (opts.json) {
    headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(opts.json);
    delete opts.json;
  }
  const res = await fetch(API + path, { ...opts, headers });
  if (res.status === 401 && state.refreshToken) {
    const refreshed = await tryRefresh();
    if (refreshed) return api(path, opts);
    logout();
    return null;
  }
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(err.error || 'Request failed');
  }
  if (res.headers.get('content-type')?.includes('json')) return res.json();
  return res;
}

async function tryRefresh() {
  try {
    const res = await fetch(API + '/auth/refresh', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: state.refreshToken }),
    });
    if (!res.ok) return false;
    const data = await res.json();
    saveAuth(data);
    return true;
  } catch { return false; }
}

function saveAuth(data) {
  state.token = data.access_token;
  state.refreshToken = data.refresh_token;
  state.userId = data.user_id;
  state.isAdmin = data.is_admin;
  localStorage.setItem('token', data.access_token);
  localStorage.setItem('refreshToken', data.refresh_token);
  localStorage.setItem('userId', data.user_id);
  localStorage.setItem('isAdmin', data.is_admin);
}

function logout() {
  if (state.refreshToken) {
    fetch(API + '/auth/logout', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: state.refreshToken }),
    }).catch(() => {});
  }
  state = { token: null, refreshToken: null, userId: null, isAdmin: false, username: null };
  localStorage.clear();
  route();
}

function formatBytes(b) {
  if (b === 0) return '0 B';
  const k = 1024, sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(b) / Math.log(k));
  return parseFloat((b / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function formatTime(ts) {
  if (!ts) return '—';
  return new Date(ts * 1000).toLocaleString();
}

function esc(str) {
  if (str == null) return '';
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// Mark a string as safe HTML (won't be escaped by the html template tag)
function raw(str) {
  const s = new String(str ?? '');
  s.__raw = true;
  return s;
}

function html(strings, ...vals) {
  return strings.reduce((acc, s, i) => {
    const v = vals[i] ?? '';
    return acc + s + (v.__raw ? String(v) : esc(v));
  }, '');
}

// --- Router ---
function route() {
  const app = $('#app');
  if (!state.token) {
    renderLogin(app);
    return;
  }
  const hash = location.hash.slice(1) || 'dashboard';
  const [page, ...params] = hash.split('/');
  renderApp(app, page, params);
}

window.addEventListener('hashchange', route);
window.addEventListener('load', route);

// --- Login/Register ---
function renderLogin(el) {
  let isRegister = false;
  function render() {
    el.innerHTML = html`
      <div class="auth-page">
        <div class="auth-card">
          <h1>ObsidianCloudSync</h1>
          <p>${isRegister ? 'Create your account' : 'Sign in to your server'}</p>
          <div id="auth-error"></div>
          <form id="auth-form">
            <div class="form-group">
              <label>Username</label>
              <input name="username" type="text" required autocomplete="username">
            </div>
            ${isRegister ? raw(html`
            <div class="form-group">
              <label>Email (optional)</label>
              <input name="email" type="email" autocomplete="email">
            </div>`) : ''}
            <div class="form-group">
              <label>Password</label>
              <input name="password" type="password" required autocomplete="${isRegister ? 'new-password' : 'current-password'}">
            </div>
            <button class="btn btn-primary btn-block" type="submit">
              ${isRegister ? 'Create Account' : 'Sign In'}
            </button>
          </form>
          <p style="margin-top:16px">
            ${isRegister ? 'Already have an account?' : "Don't have an account?"}
            <a href="#" id="toggle-auth">${isRegister ? 'Sign in' : 'Register'}</a>
          </p>
        </div>
      </div>`;
    $('#toggle-auth').onclick = (e) => { e.preventDefault(); isRegister = !isRegister; render(); };
    $('#auth-form').onsubmit = async (e) => {
      e.preventDefault();
      const fd = new FormData(e.target);
      const body = { username: fd.get('username'), password: fd.get('password') };
      if (isRegister && fd.get('email')) body.email = fd.get('email');
      body.device_name = 'Web Admin';
      body.device_type = 'web';
      try {
        const data = await api(isRegister ? '/auth/register' : '/auth/login', { method: 'POST', json: body });
        state.username = body.username;
        localStorage.setItem('username', body.username);
        saveAuth(data);
        location.hash = '#dashboard';
        route();
      } catch (err) {
        $('#auth-error').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
      }
    };
  }
  render();
}

// --- App Shell ---
function renderApp(el, page, params) {
  el.innerHTML = html`
    <div class="app-layout">
      <div class="sidebar">
        <div class="sidebar-header">OCS</div>
        <nav class="sidebar-nav">
          <a href="#dashboard" class="${page === 'dashboard' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
            <span>Dashboard</span>
          </a>
          <a href="#files" class="${page === 'files' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M13 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V9z"/><polyline points="13 2 13 9 20 9"/></svg>
            <span>Files</span>
          </a>
          <a href="#devices" class="${page === 'devices' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="3" width="20" height="14" rx="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>
            <span>Devices</span>
          </a>
          <a href="#archive" class="${page === 'archive' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
            <span>Archive</span>
          </a>
          ${state.isAdmin ? raw(html`
          <a href="#users" class="${page === 'users' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 00-3-3.87"/><path d="M16 3.13a4 4 0 010 7.75"/></svg>
            <span>Users</span>
          </a>
          <a href="#settings" class="${page === 'settings' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
            <span>Settings</span>
          </a>
          <a href="#audit" class="${page === 'audit' ? 'active' : ''}">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/></svg>
            <span>Audit Log</span>
          </a>`) : ''}
        </nav>
        <div class="sidebar-footer">
          <span class="user-name">${state.username || 'User'}</span>
          <button class="theme-toggle" onclick="toggleTheme()">
            ${document.documentElement.getAttribute('data-theme') === 'dark' ? '\u2600' : '\u263E'}
          </button>
        </div>
      </div>
      <div class="main-content" id="page-content"></div>
    </div>`;

  const content = $('#page-content');
  switch (page) {
    case 'dashboard': renderDashboard(content); break;
    case 'files': renderFiles(content, params); break;
    case 'devices': renderDevices(content); break;
    case 'archive': renderArchive(content); break;
    case 'users': state.isAdmin ? renderUsers(content) : renderDashboard(content); break;
    case 'settings': state.isAdmin ? renderSettings(content) : renderDashboard(content); break;
    case 'audit': state.isAdmin ? renderAudit(content) : renderDashboard(content); break;
    default: renderDashboard(content);
  }
}

window.toggleTheme = () => {
  const cur = document.documentElement.getAttribute('data-theme');
  setTheme(cur === 'dark' ? 'light' : 'dark');
  route();
};

// --- Dashboard ---
async function renderDashboard(el) {
  el.innerHTML = html`
    <div class="page-header"><h2>Dashboard</h2>
      <button class="btn btn-outline btn-sm" onclick="logout()">Sign Out</button>
    </div>
    <div class="stats-grid" id="stats"><div class="spinner"></div></div>`;
  window.logout = logout;
  try {
    const [files, devices] = await Promise.all([
      api('/files'),
      api('/devices'),
    ]);
    const totalSize = files.reduce((s, f) => s + f.size, 0);
    $('#stats').innerHTML = html`
      <div class="stat-card">
        <div class="stat-label">Files</div>
        <div class="stat-value">${files.length}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Storage Used</div>
        <div class="stat-value">${formatBytes(totalSize)}</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Devices</div>
        <div class="stat-value">${devices.length}</div>
      </div>`;
  } catch (err) {
    $('#stats').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }
}

// --- Files ---
async function renderFiles(el, params) {
  if (params[0]) {
    return renderFileVersions(el, params[0]);
  }
  el.innerHTML = html`
    <div class="page-header"><h2>Files</h2></div>
    <div id="files-content"><div class="spinner"></div></div>`;
  try {
    const files = await api('/files');
    if (files.length === 0) {
      $('#files-content').innerHTML = html`<div class="empty-state"><p>No files synced yet.</p></div>`;
      return;
    }
    $('#files-content').innerHTML = html`
      <div class="table-wrap"><table>
        <thead><tr><th>Path</th><th>Size</th><th>Version</th><th>Updated</th><th></th></tr></thead>
        <tbody>${raw(files.map(f => html`
          <tr>
            <td>${f.path}</td>
            <td>${formatBytes(f.size)}</td>
            <td>v${f.current_version}</td>
            <td>${formatTime(f.updated_at)}</td>
            <td><a href="#files/${f.id}" class="btn btn-outline btn-sm">Versions</a></td>
          </tr>`).join(''))}
        </tbody>
      </table></div>`;
  } catch (err) {
    $('#files-content').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }
}

async function renderFileVersions(el, fileId) {
  el.innerHTML = html`
    <div class="page-header">
      <h2>Version History</h2>
      <a href="#files" class="btn btn-outline btn-sm">Back to Files</a>
    </div>
    <div id="versions-content"><div class="spinner"></div></div>`;
  try {
    const versions = await api(`/files/${fileId}/versions`);
    if (versions.length === 0) {
      $('#versions-content').innerHTML = html`<div class="empty-state"><p>No versions found.</p></div>`;
      return;
    }
    $('#versions-content').innerHTML = html`
      <div class="table-wrap"><table>
        <thead><tr><th>Version</th><th>Hash</th><th>Size</th><th>Created</th><th></th></tr></thead>
        <tbody>${raw(versions.map(v => html`
          <tr>
            <td>v${v.version}</td>
            <td style="font-family:monospace;font-size:12px">${v.hash.slice(0, 12)}...</td>
            <td>${formatBytes(v.size)}</td>
            <td>${formatTime(v.created_at)}</td>
            <td><button class="btn btn-outline btn-sm" onclick="rollback('${fileId}', ${v.version})">Rollback</button></td>
          </tr>`).join(''))}
        </tbody>
      </table></div>`;
    window.rollback = async (fid, ver) => {
      if (!confirm(`Rollback to version ${ver}?`)) return;
      try {
        await api(`/files/${fid}/rollback`, { method: 'POST', json: { version: ver } });
        renderFileVersions(el, fid);
      } catch (err) { alert(err.message); }
    };
  } catch (err) {
    $('#versions-content').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }
}

// --- Archive ---
async function renderArchive(el) {
  let archived = [];
  let selected = new Set();

  function syncButtons() {
    const hasSel = selected.size > 0;
    const hasAny = archived.length > 0;
    const rSel = $('#restore-sel-btn');
    const dSel = $('#delete-sel-btn');
    const rAll = $('#restore-all-btn');
    const wipe = $('#wipe-archive-btn');
    if (rSel) rSel.disabled = !hasSel;
    if (dSel) dSel.disabled = !hasSel;
    if (rAll) rAll.style.display = hasAny ? '' : 'none';
    if (wipe) wipe.style.display = hasAny ? '' : 'none';
  }

  function renderTable() {
    const wrap = $('#archive-table-wrap');
    if (!wrap) return;
    if (archived.length === 0) {
      wrap.innerHTML = html`<div class="empty-state"><p>Archive is empty.</p></div>`;
      syncButtons();
      return;
    }
    const allChecked = archived.every(f => selected.has(f.id));
    wrap.innerHTML = html`
      <div class="table-wrap"><table>
        <thead><tr>
          <th><input type="checkbox" id="sel-all-archive" ${allChecked ? 'checked' : ''}></th>
          <th>Path</th><th>Size</th><th>Deleted At</th><th></th>
        </tr></thead>
        <tbody>${raw(archived.map(f => html`
          <tr>
            <td><input type="checkbox" class="archive-cb" data-id="${f.id}" ${selected.has(f.id) ? 'checked' : ''}></td>
            <td>${f.path}</td>
            <td>${formatBytes(f.size)}</td>
            <td>${formatTime(f.updated_at)}</td>
            <td><button class="btn btn-outline btn-sm" onclick="restoreOne('${f.id}')">Restore</button></td>
          </tr>`).join(''))}
        </tbody>
      </table></div>`;
    const selAllEl = $('#sel-all-archive');
    if (selAllEl) {
      selAllEl.onchange = () => {
        if (selAllEl.checked) archived.forEach(f => selected.add(f.id));
        else selected.clear();
        renderTable();
      };
    }
    $$('.archive-cb').forEach(cb => {
      cb.onchange = () => {
        if (cb.checked) selected.add(cb.dataset.id);
        else selected.delete(cb.dataset.id);
        const selAll = $('#sel-all-archive');
        if (selAll) selAll.checked = archived.every(f => selected.has(f.id));
        syncButtons();
      };
    });
    syncButtons();
  }

  el.innerHTML = html`
    <div class="page-header">
      <h2>Archive</h2>
      <div style="display:flex;gap:8px;flex-wrap:wrap;align-items:center">
        <button class="btn btn-outline btn-sm" id="restore-sel-btn" disabled>Restore Selected</button>
        <button class="btn btn-danger btn-sm" id="delete-sel-btn" disabled>Delete Selected</button>
        <button class="btn btn-outline btn-sm" id="restore-all-btn" style="display:none">Restore All</button>
        <button class="btn btn-danger btn-sm" id="wipe-archive-btn" style="display:none">Wipe Archive</button>
      </div>
    </div>
    <div id="archive-table-wrap"><div class="spinner"></div></div>`;

  window.restoreOne = async (id) => {
    if (!confirm('Restore this file?')) return;
    try {
      await api(`/files/${id}/restore`, { method: 'POST' });
      archived = archived.filter(f => f.id !== id);
      selected.delete(id);
      renderTable();
    } catch (err) { alert(err.message); }
  };

  $('#restore-sel-btn').onclick = async () => {
    const ids = [...selected];
    if (!confirm(`Restore ${ids.length} file(s)?`)) return;
    for (const id of ids) {
      try {
        await api(`/files/${id}/restore`, { method: 'POST' });
        archived = archived.filter(f => f.id !== id);
        selected.delete(id);
      } catch (err) { alert(`Failed to restore: ${err.message}`); break; }
    }
    renderTable();
  };

  $('#delete-sel-btn').onclick = async () => {
    const ids = [...selected];
    if (!confirm(`Permanently delete ${ids.length} file(s)? This cannot be undone.`)) return;
    for (const id of ids) {
      try {
        await api(`/files/${id}`, { method: 'DELETE' });
        archived = archived.filter(f => f.id !== id);
        selected.delete(id);
      } catch (err) { alert(`Failed to delete: ${err.message}`); break; }
    }
    renderTable();
  };

  $('#restore-all-btn').onclick = async () => {
    if (!confirm(`Restore all ${archived.length} archived file(s)?`)) return;
    try {
      await api('/files/archive/restore', { method: 'POST' });
      archived = [];
      selected.clear();
      renderTable();
    } catch (err) { alert(err.message); }
  };

  $('#wipe-archive-btn').onclick = async () => {
    if (!confirm('Permanently delete all archived files? This cannot be undone.')) return;
    try {
      await api('/files/archive', { method: 'DELETE' });
      archived = [];
      selected.clear();
      renderTable();
    } catch (err) { alert(err.message); }
  };

  try {
    const allFiles = await api('/files?include_deleted=true');
    archived = allFiles.filter(f => f.is_deleted);
    renderTable();
  } catch (err) {
    $('#archive-table-wrap').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }
}

// --- Devices ---
async function renderDevices(el) {
  let devices = [];
  let selected = new Set();

  function syncButtons() {
    const rSel = $('#remove-sel-btn');
    const rAll = $('#remove-all-btn');
    if (rSel) rSel.disabled = selected.size === 0;
    if (rAll) rAll.style.display = devices.length > 0 ? '' : 'none';
  }

  function renderTable() {
    const wrap = $('#devices-table-wrap');
    if (!wrap) return;
    if (devices.length === 0) {
      wrap.innerHTML = html`<div class="empty-state"><p>No devices linked.</p></div>`;
      syncButtons();
      return;
    }
    const allChecked = devices.length > 0 && devices.every(d => selected.has(d.id));
    wrap.innerHTML = html`
      <div class="table-wrap"><table>
        <thead><tr>
          <th><input type="checkbox" id="sel-all-devices" ${allChecked ? 'checked' : ''}></th>
          <th>Name</th><th>Type</th><th>Last Seen</th><th>Created</th><th></th>
        </tr></thead>
        <tbody>${raw(devices.map(d => html`
          <tr>
            <td><input type="checkbox" class="device-cb" data-id="${d.id}" ${selected.has(d.id) ? 'checked' : ''}></td>
            <td>${d.name}</td>
            <td>${d.device_type || '—'}</td>
            <td>${formatTime(d.last_seen_at)}</td>
            <td>${formatTime(d.created_at)}</td>
            <td><button class="btn btn-danger btn-sm" onclick="revokeOne('${d.id}')">Remove</button></td>
          </tr>`).join(''))}
        </tbody>
      </table></div>`;
    const selAllEl = $('#sel-all-devices');
    if (selAllEl) {
      selAllEl.onchange = () => {
        if (selAllEl.checked) devices.forEach(d => selected.add(d.id));
        else selected.clear();
        renderTable();
      };
    }
    $$('.device-cb').forEach(cb => {
      cb.onchange = () => {
        if (cb.checked) selected.add(cb.dataset.id);
        else selected.delete(cb.dataset.id);
        const selAll = $('#sel-all-devices');
        if (selAll) selAll.checked = devices.every(d => selected.has(d.id));
        syncButtons();
      };
    });
    syncButtons();
  }

  el.innerHTML = html`
    <div class="page-header">
      <h2>Devices</h2>
      <div style="display:flex;gap:8px;align-items:center">
        <button class="btn btn-danger btn-sm" id="remove-sel-btn" disabled>Remove Selected</button>
        <button class="btn btn-danger btn-sm" id="remove-all-btn" style="display:none">Remove All Others</button>
      </div>
    </div>
    <div id="devices-table-wrap"><div class="spinner"></div></div>`;

  window.revokeOne = async (id) => {
    if (!confirm('Remove this device? It will be signed out and forgotten.')) return;
    try {
      await api(`/devices/${id}`, { method: 'DELETE' });
      devices = devices.filter(d => d.id !== id);
      selected.delete(id);
      renderTable();
    } catch (err) { alert(err.message); }
  };

  $('#remove-sel-btn').onclick = async () => {
    const ids = [...selected];
    if (!confirm(`Remove ${ids.length} device(s)?`)) return;
    for (const id of ids) {
      try {
        await api(`/devices/${id}`, { method: 'DELETE' });
        devices = devices.filter(d => d.id !== id);
        selected.delete(id);
      } catch (err) { alert(`Failed to remove device: ${err.message}`); break; }
    }
    renderTable();
  };

  $('#remove-all-btn').onclick = async () => {
    if (!confirm('Remove all other devices? They will be signed out and forgotten.')) return;
    try {
      await api('/devices', { method: 'DELETE' });
      devices = await api('/devices');
      selected.clear();
      renderTable();
    } catch (err) { alert(err.message); }
  };

  try {
    devices = await api('/devices');
    renderTable();
  } catch (err) {
    $('#devices-table-wrap').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }
}

// --- Users (Admin) ---
async function renderUsers(el) {
  el.innerHTML = html`
    <div class="page-header">
      <h2>Users</h2>
      <button class="btn btn-primary btn-sm" onclick="showCreateUser()">Create User</button>
    </div>
    <div id="users-content"><div class="spinner"></div></div>
    <div id="modal-root"></div>`;
  try {
    const users = await api('/admin/users');
    $('#users-content').innerHTML = html`
      <div class="table-wrap"><table>
        <thead><tr><th>Username</th><th>Email</th><th>Role</th><th>Files</th><th>Devices</th><th>Created</th><th></th></tr></thead>
        <tbody>${raw(users.map(u => html`
          <tr>
            <td>${u.username}</td>
            <td>${u.email || '—'}</td>
            <td>${raw(u.is_admin ? '<span class="badge badge-warning">Admin</span>' : '<span class="badge badge-success">User</span>')}</td>
            <td>${u.file_count}</td>
            <td>${u.device_count}</td>
            <td>${formatTime(u.created_at)}</td>
            <td>${raw(`<button class="btn btn-danger btn-sm" onclick="deleteUser('${esc(u.id)}','${esc(u.username)}')">Delete</button>`)}</td>
          </tr>`).join(''))}
        </tbody>
      </table></div>`;
  } catch (err) {
    $('#users-content').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }

  window.showCreateUser = () => {
    $('#modal-root').innerHTML = html`
      <div class="modal-overlay" onclick="if(event.target===this)closeModal()">
        <div class="modal">
          <h3>Create User</h3>
          <div id="create-error"></div>
          <form id="create-user-form">
            <div class="form-group"><label>Username</label><input name="username" required></div>
            <div class="form-group"><label>Password</label><input name="password" type="password" required></div>
            <div class="form-group"><label>Email</label><input name="email" type="email"></div>
            <div class="form-group"><label><input name="is_admin" type="checkbox"> Admin</label></div>
            <div class="modal-actions">
              <button type="button" class="btn btn-outline" onclick="closeModal()">Cancel</button>
              <button type="submit" class="btn btn-primary">Create</button>
            </div>
          </form>
        </div>
      </div>`;
    $('#create-user-form').onsubmit = async (e) => {
      e.preventDefault();
      const fd = new FormData(e.target);
      try {
        await api('/admin/users', { method: 'POST', json: {
          username: fd.get('username'),
          password: fd.get('password'),
          email: fd.get('email') || null,
          is_admin: fd.has('is_admin'),
        }});
        closeModal();
        renderUsers(el);
      } catch (err) {
        $('#create-error').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
      }
    };
  };
  window.closeModal = () => { $('#modal-root').innerHTML = ''; };
  window.deleteUser = async (id, name) => {
    if (!confirm(`Delete user "${name}" and all their data?`)) return;
    try {
      await api(`/admin/users/${id}`, { method: 'DELETE' });
      renderUsers(el);
    } catch (err) { alert(err.message); }
  };
}

// --- Settings (Admin) ---
async function renderSettings(el) {
  el.innerHTML = html`
    <div class="page-header"><h2>Settings</h2></div>
    <div id="settings-content"><div class="spinner"></div></div>`;
  try {
    const data = await api('/admin/settings');
    const s = data.settings;
    $('#settings-content').innerHTML = html`
      <div style="background:var(--bg-secondary);border:1px solid var(--border);border-radius:var(--radius);padding:24px;">
        <form id="settings-form">
          <div class="form-group">
            <label>Max Versions Per File</label>
            <input name="max_versions_per_file" type="number" min="1" max="1000" value="${s.max_versions_per_file || 50}">
          </div>
          <div class="form-group">
            <label>Max Version Age (days) — set to 0 to never expire</label>
            <input name="max_version_age_days" type="number" min="0" max="3650" value="${s.max_version_age_days !== undefined ? s.max_version_age_days : 90}">
          </div>
          <div class="form-group">
            <label>Registration Open</label>
            <select name="registration_open">
              <option value="true" ${s.registration_open === 'true' ? 'selected' : ''}>Open</option>
              <option value="false" ${s.registration_open === 'false' ? 'selected' : ''}>Closed</option>
            </select>
          </div>
          <div class="form-group">
            <label>Data Preservation</label>
            <div style="display:flex;flex-direction:column;gap:10px;margin-top:8px">
              <label style="display:flex;align-items:flex-start;gap:10px;cursor:default;opacity:0.6">
                <input type="checkbox" checked disabled style="margin-top:3px;flex-shrink:0">
                <span>Current version of every file is always preserved — the pruner never deletes the active version (enforced in server code, cannot be disabled)</span>
              </label>
              <label style="display:flex;align-items:flex-start;gap:10px;cursor:pointer">
                <input type="checkbox" name="keep_archive_versions" ${s.keep_archive_versions === 'true' ? 'checked' : ''} style="margin-top:3px;flex-shrink:0">
                <span>Preserve all versions of archived (deleted) files — disables automatic version pruning for files in the Archive</span>
              </label>
            </div>
          </div>
          <div id="settings-msg"></div>
          <button type="submit" class="btn btn-primary">Save Settings</button>
        </form>
      </div>`;
    $('#settings-form').onsubmit = async (e) => {
      e.preventDefault();
      const fd = new FormData(e.target);
      try {
        await api('/admin/settings', { method: 'PUT', json: {
          settings: {
            max_versions_per_file: fd.get('max_versions_per_file'),
            max_version_age_days: fd.get('max_version_age_days'),
            registration_open: fd.get('registration_open'),
            keep_archive_versions: fd.has('keep_archive_versions') ? 'true' : 'false',
          }
        }});
        $('#settings-msg').innerHTML = '<div style="color:var(--success);margin:8px 0">Settings saved.</div>';
      } catch (err) {
        $('#settings-msg').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
      }
    };
  } catch (err) {
    $('#settings-content').innerHTML = `<div class="error-msg">${esc(err.message)}</div>`;
  }
}

// --- Audit Log (Admin) ---
async function renderAudit(el) {
  el.innerHTML = html`
    <div class="page-header"><h2>Audit Log</h2></div>
    <div style="margin-bottom:16px;display:flex;gap:8px;align-items:center">
      <select id="audit-action-filter" style="padding:6px 10px;border-radius:var(--radius);border:1px solid var(--border);background:var(--bg-secondary);color:var(--text)">
        <option value="">All actions</option>
        <option value="login">Login</option>
        <option value="login_failed">Login Failed</option>
        <option value="logout">Logout</option>
        <option value="register">Register</option>
        <option value="password_change">Password Change</option>
        <option value="file_upload">File Upload</option>
        <option value="file_delete">File Delete</option>
        <option value="file_rollback">File Rollback</option>
        <option value="device_revoke">Device Revoke</option>
        <option value="user_create">User Create</option>
        <option value="user_delete">User Delete</option>
        <option value="settings_change">Settings Change</option>
      </select>
      <button class="btn btn-outline btn-sm" id="audit-refresh-btn">Refresh</button>
    </div>
    <div id="audit-content"><div class="spinner"></div></div>
    <div id="audit-pagination" style="margin-top:12px;display:flex;gap:8px;align-items:center"></div>`;

  let currentPage = 1;

  async function loadAudit(page) {
    currentPage = page;
    const filter = $('#audit-action-filter').value;
    let url = '/admin/audit?page=' + page + '&limit=50';
    if (filter) url += '&action=' + encodeURIComponent(filter);
    try {
      const data = await api(url);
      if (!data || !data.entries || data.entries.length === 0) {
        $('#audit-content').innerHTML = '<div class="empty-state"><p>No audit entries found.</p></div>';
        $('#audit-pagination').innerHTML = '';
        return;
      }
      $('#audit-content').innerHTML = html`
        <div class="table-wrap"><table>
          <thead><tr><th>Time</th><th>Action</th><th>User</th><th>Target</th><th>Details</th></tr></thead>
          <tbody>${raw(data.entries.map(e => html`
            <tr>
              <td style="white-space:nowrap">${formatTime(e.created_at)}</td>
              <td><span class="badge">${e.action}</span></td>
              <td style="font-family:monospace;font-size:12px">${e.user_id ? e.user_id.slice(0, 8) + '...' : '\u2014'}</td>
              <td>${e.target_type ? e.target_type + (e.target_id ? ':' + e.target_id.slice(0, 8) : '') : '\u2014'}</td>
              <td style="font-size:12px">${e.details || '\u2014'}</td>
            </tr>`).join(''))}</tbody>
        </table></div>`;

      const totalPages = Math.ceil(data.total / data.limit);
      let paginationHtml = '';
      if (totalPages > 1) {
        if (currentPage > 1) paginationHtml += '<button class="btn btn-outline btn-sm" onclick="auditPage(' + (currentPage - 1) + ')">Prev</button>';
        paginationHtml += '<span>Page ' + currentPage + ' of ' + totalPages + '</span>';
        if (currentPage < totalPages) paginationHtml += '<button class="btn btn-outline btn-sm" onclick="auditPage(' + (currentPage + 1) + ')">Next</button>';
      }
      $('#audit-pagination').innerHTML = paginationHtml;
    } catch (err) {
      $('#audit-content').innerHTML = '<div class="error-msg">' + esc(err.message) + '</div>';
    }
  }

  window.auditPage = (p) => loadAudit(p);
  $('#audit-action-filter').onchange = () => loadAudit(1);
  $('#audit-refresh-btn').onclick = () => loadAudit(currentPage);
  loadAudit(1);
}
