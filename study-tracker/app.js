// ============================================
// APP STATE & PERSISTENCE (File + localStorage)
// ============================================
const STORAGE_KEY = 'efficientai_tracker';
const HANDLE_DB_NAME = 'TrackerHandleDB';
const HANDLE_STORE = 'handles';
const DATA_FILENAME = 'tracker-data.json';
let state = loadStateFromCache();
let fileHandle = null;
let hasFileConnection = false;
let autoSaveTimer = null;

function defaultState() {
  return {
    sessions: [],
    projectStatus: {},
    gateChecks: {},
    readingChecks: {},
    activities: [],
  };
}

// --- localStorage cache (fast, same-browser only) ---
function loadStateFromCache() {
  try {
    const s = localStorage.getItem(STORAGE_KEY);
    return s ? { ...defaultState(), ...JSON.parse(s) } : defaultState();
  } catch { return defaultState(); }
}

function saveToCacheOnly() {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state)); } catch {}
}

// --- IndexedDB for persisting file handle across page reloads ---
function openHandleDB() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(HANDLE_DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(HANDLE_STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function storeFileHandle(handle) {
  try {
    const db = await openHandleDB();
    const tx = db.transaction(HANDLE_STORE, 'readwrite');
    tx.objectStore(HANDLE_STORE).put(handle, 'dataFile');
    await new Promise((res, rej) => { tx.oncomplete = res; tx.onerror = rej; });
    db.close();
  } catch {}
}

async function retrieveFileHandle() {
  try {
    const db = await openHandleDB();
    const tx = db.transaction(HANDLE_STORE, 'readonly');
    const req = tx.objectStore(HANDLE_STORE).get('dataFile');
    const handle = await new Promise((res, rej) => { req.onsuccess = () => res(req.result); req.onerror = rej; });
    db.close();
    return handle || null;
  } catch { return null; }
}

// --- Auto-load from ./tracker-data.json on startup (read-only, no permissions needed) ---
async function autoLoadFromFetch() {
  if (window.location.protocol === 'file:') return false; // Prevent CORS error on local file domains
  try {
    const resp = await fetch('./' + DATA_FILENAME + '?t=' + Date.now());
    if (!resp.ok) return false;
    const text = await resp.text();
    const parsed = JSON.parse(text);
    // Only use fetched data if it has actual content
    const hasContent = parsed.sessions?.length || Object.keys(parsed.projectStatus || {}).length ||
      Object.keys(parsed.gateChecks || {}).length || Object.keys(parsed.readingChecks || {}).length;
    if (hasContent) {
      state = { ...defaultState(), ...parsed };
      saveToCacheOnly();
      return true;
    }
    return false;
  } catch { return false; }
}

// --- Auto-reconnect stored file handle from IndexedDB ---
async function autoReconnectHandle() {
  if (!supportsFileAccess) return false;
  try {
    const stored = await retrieveFileHandle();
    if (!stored) return false;
    // Verify we still have permission (will return 'granted' if previously allowed)
    const perm = await stored.queryPermission({ mode: 'readwrite' });
    if (perm === 'granted') {
      fileHandle = stored;
      hasFileConnection = true;
      updateSyncUI('connected');
      return true;
    }
    // Permission not yet granted — we'll request it on first user-initiated save
    // Store it so we can request on save
    fileHandle = stored;
    return false;
  } catch { return false; }
}

// --- File System Access API (Chrome/Edge ≥86) ---
const supportsFileAccess = 'showSaveFilePicker' in window;

async function connectDataFile() {
  if (!supportsFileAccess) {
    document.getElementById('file-import-input').click();
    return;
  }
  try {
    let handles = await window.showOpenFilePicker({
      types: [{ description: 'Tracker Data', accept: { 'application/json': ['.json'] } }],
      startIn: 'documents',
    });
    fileHandle = handles[0];
    await storeFileHandle(fileHandle);
    await loadFromFileHandle();
    hasFileConnection = true;
    updateSyncUI('connected');
    showToast('Data file connected ✓');
  } catch (e) {
    if (e.name !== 'AbortError') showToast('Could not open file');
  }
}

async function createDataFile() {
  if (!supportsFileAccess) {
    downloadJSON();
    return;
  }
  try {
    fileHandle = await window.showSaveFilePicker({
      suggestedName: DATA_FILENAME,
      types: [{ description: 'Tracker Data', accept: { 'application/json': ['.json'] } }],
      startIn: 'documents',
    });
    await storeFileHandle(fileHandle);
    await writeToFileHandle();
    hasFileConnection = true;
    updateSyncUI('connected');
    showToast('Data file created & saved ✓');
  } catch (e) {
    if (e.name !== 'AbortError') showToast('Could not create file');
  }
}

async function loadFromFileHandle() {
  if (!fileHandle) return false;
  try {
    const file = await fileHandle.getFile();
    const text = await file.text();
    const parsed = JSON.parse(text);
    state = { ...defaultState(), ...parsed };
    saveToCacheOnly();
    refreshAllViews();
    return true;
  } catch (e) {
    showToast('Error reading data file');
    return false;
  }
}

async function writeToFileHandle() {
  if (!fileHandle) return false;
  try {
    // If we have a handle but no permission yet, request it now (requires user gesture)
    if (!hasFileConnection) {
      const perm = await fileHandle.requestPermission({ mode: 'readwrite' });
      if (perm !== 'granted') {
        showToast('File permission denied — saving locally');
        return false;
      }
      hasFileConnection = true;
      await storeFileHandle(fileHandle);
    }
    const writable = await fileHandle.createWritable();
    await writable.write(JSON.stringify(state, null, 2));
    await writable.close();
    return true;
  } catch (e) {
    showToast('Error writing to data file');
    return false;
  }
}

// --- Fallback: download / upload JSON ---
function downloadJSON() {
  const blob = new Blob([JSON.stringify(state, null, 2)], { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = DATA_FILENAME;
  a.click();
  URL.revokeObjectURL(a.href);
  showToast('Data file downloaded ✓');
}

function handleFileImport(event) {
  const file = event.target.files[0];
  if (!file) return;
  const reader = new FileReader();
  reader.onload = (e) => {
    try {
      const parsed = JSON.parse(e.target.result);
      state = { ...defaultState(), ...parsed };
      saveToCacheOnly();
      refreshAllViews();
      updateSyncUI('imported');
      showToast('Data imported successfully ✓');
    } catch {
      showToast('Invalid JSON file');
    }
  };
  reader.readAsText(file);
  event.target.value = '';
}

// --- Unified save (cache + file) ---
async function saveState() {
  saveToCacheOnly();
  if (fileHandle) {
    const ok = await writeToFileHandle();
    showToast(ok ? 'Saved to file ✓' : 'Saved locally (file write failed)');
    updateSyncUI(ok ? 'connected' : 'error');
  } else if (supportsFileAccess) {
    // No handle yet. Since native dialogs (like confirm) consume the user gesture 
    // needed for the file picker, we directly show the open picker to attach the existing file.
    await connectDataFile();
  } else {
    showToast('Saved locally ✓');
  }
}

function scheduleAutoSave() {
  clearTimeout(autoSaveTimer);
  autoSaveTimer = setTimeout(() => {
    saveToCacheOnly();
    if (hasFileConnection && fileHandle) {
      writeToFileHandle().then(ok => {
        updateSyncUI(ok ? 'connected' : 'error');
      });
    }
  }, 1500);
}

// --- Sync status UI ---
function updateSyncUI(status) {
  const dot = document.getElementById('sync-dot');
  const label = document.getElementById('sync-label');
  if (!dot || !label) return;
  const map = {
    connected: { color: '#34d399', text: 'Synced to file' },
    imported: { color: '#34d399', text: 'Imported from file' },
    error: { color: '#f87171', text: 'Sync error' },
    disconnected: { color: '#6a6a82', text: 'Local only' },
    loading: { color: '#fbbf24', text: 'Loading...' },
  };
  const s = map[status] || map.disconnected;
  dot.style.background = s.color;
  label.textContent = s.text;
}

function refreshAllViews() {
  updateDashboard();
  initWeeklyLog();
  initProjects();
  initGates();
  initReading();
  initTimeline();
}

function showToast(msg) {
  const t = document.getElementById('toast');
  t.textContent = msg;
  t.classList.add('show');
  setTimeout(() => t.classList.remove('show'), 2000);
}

// ============================================
// TAB NAVIGATION
// ============================================
document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
    tab.classList.add('active');
    document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
  });
});

// ============================================
// DASHBOARD
// ============================================
function updateDashboard() {
  // KPIs
  const totalProjects = PROJECTS.length;
  const doneProjects = PROJECTS.filter(p => state.projectStatus[p.id] === 'complete').length;
  const inProgress = PROJECTS.filter(p => state.projectStatus[p.id] === 'in-progress').length;

  let totalGateItems = 0, checkedGateItems = 0, gatesPassed = 0;
  GATES.forEach(g => {
    const total = g.items.length;
    const checked = g.items.filter((_, i) => state.gateChecks[g.id + '_' + i]).length;
    totalGateItems += total;
    checkedGateItems += checked;
    if (checked === total) gatesPassed++;
  });

  let totalReading = 0, doneReading = 0;
  READING.forEach(s => {
    s.items.forEach((_, i) => {
      totalReading++;
      if (state.readingChecks[s.section + '_' + i]) doneReading++;
    });
  });

  const totalHours = state.sessions.reduce((a, s) => a + (s.hours || 0), 0);
  const overallItems = totalProjects + totalGateItems + totalReading;
  const overallDone = doneProjects + checkedGateItems + doneReading;
  const overallPct = overallItems ? Math.round((overallDone / overallItems) * 100) : 0;

  document.getElementById('kpi-overall-pct').textContent = overallPct + '%';
  document.getElementById('kpi-projects-done').textContent = doneProjects + ' / ' + totalProjects;
  document.getElementById('kpi-papers-read').textContent = doneReading + ' / ' + totalReading;
  document.getElementById('kpi-gates-passed').textContent = gatesPassed + ' / ' + GATES.length;
  document.getElementById('kpi-study-hours').textContent = totalHours.toFixed(1) + ' hrs';

  // Streak
  const streak = calcStreak();
  document.getElementById('kpi-streak').textContent = streak + ' days';

  // Year bars
  renderYearBars();
  renderPhaseChart();
  renderHeatmap();
  renderActivityFeed();
}

function calcStreak() {
  if (!state.sessions.length) return 0;
  const dates = [...new Set(state.sessions.map(s => s.date))].sort().reverse();
  const today = new Date().toISOString().slice(0, 10);
  let streak = 0;
  let check = new Date(today);
  for (let i = 0; i < 365; i++) {
    const d = check.toISOString().slice(0, 10);
    if (dates.includes(d)) {
      streak++;
    } else if (i > 0) break;
    check.setDate(check.getDate() - 1);
  }
  return streak;
}

function renderYearBars() {
  const container = document.getElementById('year-bars');
  container.innerHTML = '';
  for (let y = 1; y <= 4; y++) {
    const yProjects = PROJECTS.filter(p => PHASES.find(ph => ph.id === p.phase)?.year === y);
    const done = yProjects.filter(p => state.projectStatus[p.id] === 'complete').length;
    const pct = yProjects.length ? Math.round((done / yProjects.length) * 100) : 0;
    const color = YEAR_COLORS[y];
    container.innerHTML += `
      <div class="year-bar-row">
        <span class="year-bar-label">Year ${y}</span>
        <div class="year-bar-track">
          <div class="year-bar-fill" style="width:${Math.max(pct, 2)}%;background:${color}" data-pct="${pct}%"></div>
        </div>
        <span class="year-bar-pct" style="color:${color}">${pct}%</span>
      </div>`;
  }
}

function renderPhaseChart() {
  const canvas = document.getElementById('chart-phase');
  const ctx = canvas.getContext('2d');
  const size = 280, cx = size / 2, cy = size / 2, r = 100, r2 = 65;
  ctx.clearRect(0, 0, size, size);

  const phaseData = PHASES.map(ph => {
    const projs = PROJECTS.filter(p => p.phase === ph.id);
    const done = projs.filter(p => state.projectStatus[p.id] === 'complete').length;
    return { ...ph, total: projs.length, done, pct: projs.length ? done / projs.length : 0 };
  });

  const total = phaseData.reduce((a, p) => a + p.total, 0);
  let angle = -Math.PI / 2;

  phaseData.forEach(ph => {
    const slice = (ph.total / total) * Math.PI * 2;
    // Background
    ctx.beginPath();
    ctx.arc(cx, cy, r, angle, angle + slice);
    ctx.arc(cx, cy, r2, angle + slice, angle, true);
    ctx.closePath();
    ctx.fillStyle = ph.color + '30';
    ctx.fill();
    // Fill
    if (ph.pct > 0) {
      ctx.beginPath();
      ctx.arc(cx, cy, r, angle, angle + slice * ph.pct);
      ctx.arc(cx, cy, r2, angle + slice * ph.pct, angle, true);
      ctx.closePath();
      ctx.fillStyle = ph.color;
      ctx.fill();
    }
    angle += slice;
  });

  // Center text
  const totalDone = phaseData.reduce((a, p) => a + p.done, 0);
  ctx.fillStyle = '#e8e8f0';
  ctx.font = '800 28px Inter';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(totalDone + '/' + total, cx, cy - 8);
  ctx.fillStyle = '#6a6a82';
  ctx.font = '500 11px Inter';
  ctx.fillText('PROJECTS', cx, cy + 14);
}

function renderHeatmap() {
  const container = document.getElementById('heatmap-container');
  container.innerHTML = '';
  const today = new Date();
  const start = new Date(today);
  start.setDate(start.getDate() - 182); // ~6 months
  start.setDate(start.getDate() - start.getDay()); // align to Sunday

  const sessionMap = {};
  state.sessions.forEach(s => {
    sessionMap[s.date] = (sessionMap[s.date] || 0) + s.hours;
  });

  let d = new Date(start);
  while (d <= today) {
    const weekDiv = document.createElement('div');
    weekDiv.className = 'heatmap-week';
    for (let i = 0; i < 7 && d <= today; i++) {
      const ds = d.toISOString().slice(0, 10);
      const hrs = sessionMap[ds] || 0;
      const cell = document.createElement('div');
      cell.className = 'heatmap-day';
      cell.dataset.tooltip = ds + ': ' + hrs.toFixed(1) + 'h';
      if (hrs > 0) {
        const intensity = Math.min(hrs / 5, 1);
        cell.style.background = `rgba(99,102,241,${0.15 + intensity * 0.85})`;
      }
      weekDiv.appendChild(cell);
      d.setDate(d.getDate() + 1);
    }
    container.appendChild(weekDiv);
  }
}

function renderActivityFeed() {
  const feed = document.getElementById('activity-feed');
  if (!state.activities.length) {
    feed.innerHTML = '<div class="empty-state"><svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"/></svg><p>No activity yet. Start tracking!</p></div>';
    return;
  }
  const colors = { session: '#818cf8', project: '#34d399', gate: '#fbbf24', reading: '#f472b6' };
  feed.innerHTML = state.activities.slice(-20).reverse().map(a => `
    <div class="activity-item">
      <div class="activity-dot" style="background:${colors[a.type] || '#818cf8'}"></div>
      <div>
        <div class="activity-text">${a.text}</div>
        <div class="activity-time">${timeAgo(a.time)}</div>
      </div>
    </div>`).join('');
}

function timeAgo(ts) {
  const diff = Date.now() - new Date(ts).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return mins + 'm ago';
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return hrs + 'h ago';
  return Math.floor(hrs / 24) + 'd ago';
}

function addActivity(type, text) {
  state.activities.push({ type, text, time: new Date().toISOString() });
  if (state.activities.length > 100) state.activities = state.activities.slice(-100);
}

// ============================================
// WEEKLY LOG
// ============================================
function initWeeklyLog() {
  document.getElementById('session-date').value = new Date().toISOString().slice(0, 10);

  document.getElementById('btn-log-session').addEventListener('click', () => {
    const date = document.getElementById('session-date').value;
    const hours = parseFloat(document.getElementById('session-hours').value);
    const type = document.getElementById('session-type').value;
    const phase = document.getElementById('session-phase').value;
    const notes = document.getElementById('session-notes').value;
    if (!date || !hours || hours <= 0) { showToast('Enter a valid date & hours'); return; }
    state.sessions.push({ date, hours, type, phase, notes, id: Date.now() });
    addActivity('session', `Logged <strong>${hours}h</strong> of ${type.replace('-', ' ')} (Phase ${phase})`);
    document.getElementById('session-hours').value = '';
    document.getElementById('session-notes').value = '';
    saveStateQuiet();
    renderSessions();
    renderWeeklySummary();
    updateDashboard();
    showToast('Session logged ✓');
  });

  document.getElementById('filter-week').addEventListener('change', renderSessions);
  renderSessions();
  renderWeeklySummary();
}

function renderWeeklySummary() {
  const grid = document.getElementById('weekly-summary-grid');
  const today = new Date();
  const dayOfWeek = today.getDay();
  const monday = new Date(today);
  monday.setDate(today.getDate() - ((dayOfWeek + 6) % 7));
  const days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  const targets = [3, 3.5, 3.5, 3.5, 2.5, 2, 2];
  let html = '';
  for (let i = 0; i < 7; i++) {
    const d = new Date(monday);
    d.setDate(monday.getDate() + i);
    const ds = d.toISOString().slice(0, 10);
    const hrs = state.sessions.filter(s => s.date === ds).reduce((a, s) => a + s.hours, 0);
    const isToday = ds === today.toISOString().slice(0, 10);
    const color = hrs >= targets[i] ? '#34d399' : hrs > 0 ? '#fbbf24' : 'var(--text-3)';
    html += `<div class="day-card${isToday ? ' today' : ''}">
      <div class="day-name">${days[i]}</div>
      <div class="day-hours" style="color:${color}">${hrs.toFixed(1)}</div>
      <div class="day-target">/ ${targets[i]}h</div>
    </div>`;
  }
  grid.innerHTML = html;
}

function renderSessions() {
  const filter = document.getElementById('filter-week').value;
  const tbody = document.getElementById('sessions-tbody');
  let filtered = [...state.sessions];
  const today = new Date();
  const todayStr = today.toISOString().slice(0, 10);

  if (filter === 'this-week') {
    const mon = new Date(today);
    mon.setDate(today.getDate() - ((today.getDay() + 6) % 7));
    const monStr = mon.toISOString().slice(0, 10);
    filtered = filtered.filter(s => s.date >= monStr && s.date <= todayStr);
  } else if (filter === 'last-week') {
    const mon = new Date(today);
    mon.setDate(today.getDate() - ((today.getDay() + 6) % 7) - 7);
    const sun = new Date(mon);
    sun.setDate(mon.getDate() + 6);
    filtered = filtered.filter(s => s.date >= mon.toISOString().slice(0, 10) && s.date <= sun.toISOString().slice(0, 10));
  } else if (filter === 'this-month') {
    const mStart = todayStr.slice(0, 7);
    filtered = filtered.filter(s => s.date.startsWith(mStart));
  }

  filtered.sort((a, b) => b.date.localeCompare(a.date) || b.id - a.id);
  const typeLabels = { 'paper-reading': '📄 Paper', 'theory-study': '📖 Theory', 'implementation': '💻 Code', 'debugging': '🐛 Debug', 'review-writeup': '✍️ Writeup', 'video-lecture': '🎥 Video', 'other': '🔧 Other' };

  tbody.innerHTML = filtered.length ? filtered.map(s => `
    <tr>
      <td class="mono">${s.date}</td>
      <td class="mono">${s.hours}h</td>
      <td>${typeLabels[s.type] || s.type}</td>
      <td>${s.phase}</td>
      <td>${s.notes || '—'}</td>
      <td><button class="btn btn-danger btn-sm" onclick="deleteSession(${s.id})">✕</button></td>
    </tr>`).join('') : '<tr><td colspan="6" style="text-align:center;color:var(--text-3);padding:2rem">No sessions found</td></tr>';
}

window.deleteSession = function(id) {
  state.sessions = state.sessions.filter(s => s.id !== id);
  saveStateQuiet();
  renderSessions();
  renderWeeklySummary();
  updateDashboard();
};

// ============================================
// PROJECTS
// ============================================
function initProjects() {
  renderProjects();
  document.querySelectorAll('#tab-projects .filter-chip').forEach(chip => {
    chip.addEventListener('click', () => {
      document.querySelectorAll('#tab-projects .filter-chip').forEach(c => c.classList.remove('active'));
      chip.classList.add('active');
      renderProjects(chip.dataset.filter);
    });
  });
}

function renderProjects(filter = 'all') {
  const container = document.getElementById('projects-container');
  let phases = [...PHASES];

  if (filter.startsWith('year-')) {
    const y = parseInt(filter.split('-')[1]);
    phases = phases.filter(p => p.year === y);
  }

  let html = '';
  phases.forEach(ph => {
    let projs = PROJECTS.filter(p => p.phase === ph.id);
    if (filter === 'not-started') projs = projs.filter(p => !state.projectStatus[p.id] || state.projectStatus[p.id] === 'not-started');
    else if (filter === 'in-progress') projs = projs.filter(p => state.projectStatus[p.id] === 'in-progress');
    else if (filter === 'complete') projs = projs.filter(p => state.projectStatus[p.id] === 'complete');

    if (!projs.length && (filter === 'not-started' || filter === 'in-progress' || filter === 'complete')) return;

    const allProjs = PROJECTS.filter(p => p.phase === ph.id);
    const done = allProjs.filter(p => state.projectStatus[p.id] === 'complete').length;

    html += `<div class="phase-group">
      <div class="phase-group-header">
        <span class="phase-badge" style="background:${ph.color}20;color:${ph.color}">${ph.id}</span>
        <span class="phase-group-title">${ph.title}</span>
        <span class="phase-progress">${done}/${allProjs.length}</span>
      </div>`;
    projs.forEach(p => {
      const st = state.projectStatus[p.id] || 'not-started';
      html += `<div class="project-card">
        <span class="project-id">${p.id}</span>
        <div class="project-info">
          <div class="project-name">${p.name}</div>
          <div class="project-meta">Phase ${p.phase} · Months ${PHASES.find(ph2 => ph2.id === p.phase)?.months}</div>
        </div>
        <div class="project-status">
          <select class="status-select" onchange="updateProjectStatus('${p.id}', this.value)">
            <option value="not-started" ${st === 'not-started' ? 'selected' : ''}>⬜ Not Started</option>
            <option value="in-progress" ${st === 'in-progress' ? 'selected' : ''}>🟨 In Progress</option>
            <option value="complete" ${st === 'complete' ? 'selected' : ''}>✅ Complete</option>
          </select>
        </div>
      </div>`;
    });
    html += '</div>';
  });
  container.innerHTML = html || '<div class="empty-state"><p>No projects match this filter.</p></div>';
}

window.updateProjectStatus = function(id, status) {
  const prev = state.projectStatus[id];
  state.projectStatus[id] = status;
  if (status === 'complete' && prev !== 'complete') {
    const p = PROJECTS.find(pr => pr.id === id);
    addActivity('project', `Completed project <strong>${id}</strong>: ${p?.name}`);
  }
  saveStateQuiet();
  updateDashboard();
};

// ============================================
// GATES
// ============================================
function initGates() {
  const container = document.getElementById('gates-container');
  let html = '';
  GATES.forEach(g => {
    const checked = g.items.filter((_, i) => state.gateChecks[g.id + '_' + i]).length;
    const complete = checked === g.items.length;
    const phase = PHASES.find(p => p.id === g.phase);
    html += `<div class="gate-card">
      <div class="gate-header">
        <span class="gate-status-icon">${complete ? '✅' : checked > 0 ? '🟨' : '⬜'}</span>
        <span class="gate-title" style="color:${phase?.color || 'inherit'}">${g.title}</span>
        <span class="gate-progress-badge ${complete ? 'complete' : ''}">${checked}/${g.items.length}</span>
      </div>
      <div class="gate-items">
        ${g.items.map((item, i) => `
          <div class="gate-item">
            <input type="checkbox" class="gate-checkbox" ${state.gateChecks[g.id + '_' + i] ? 'checked' : ''} onchange="toggleGate('${g.id}', ${i}, this.checked)">
            <span>${item}</span>
          </div>`).join('')}
      </div>
    </div>`;
  });
  container.innerHTML = html;
}

window.toggleGate = function(gateId, idx, checked) {
  state.gateChecks[gateId + '_' + idx] = checked;
  if (checked) {
    const g = GATES.find(g2 => g2.id === gateId);
    addActivity('gate', `Checked gate item in <strong>${g?.title}</strong>`);
  }
  saveStateQuiet();
  initGates();
  updateDashboard();
};

// ============================================
// READING
// ============================================
function initReading() {
  renderReading();
  document.querySelectorAll('#tab-reading .filter-chip').forEach(chip => {
    chip.addEventListener('click', () => {
      document.querySelectorAll('#tab-reading .filter-chip').forEach(c => c.classList.remove('active'));
      chip.classList.add('active');
      renderReading(chip.dataset.filter);
    });
  });
}

function renderReading(filter = 'all') {
  const container = document.getElementById('reading-container');
  const priIcon = { critical: '🔴', important: '🟡', reference: '🔵' };
  let html = '';
  READING.forEach(section => {
    if (filter === 'textbook' && section.type !== 'textbook') return;
    if (filter === 'paper' && section.type !== 'paper') return;

    let items = section.items;
    if (filter === 'critical') items = items.filter(it => it.pri === 'critical');
    else if (filter === 'important') items = items.filter(it => it.pri === 'important');
    else if (filter === 'reference') items = items.filter(it => it.pri === 'reference');
    if (!items.length) return;

    html += `<div class="reading-section"><div class="reading-section-title">${section.section}</div>`;
    items.forEach((item, i) => {
      const origIdx = section.items.indexOf(item);
      const key = section.section + '_' + origIdx;
      const checked = state.readingChecks[key];
      html += `<div class="reading-item">
        <input type="checkbox" class="reading-checkbox" ${checked ? 'checked' : ''} onchange="toggleReading('${escapeAttr(section.section)}', ${origIdx}, this.checked)">
        <span class="reading-priority">${priIcon[item.pri] || ''}</span>
        <span class="reading-text ${checked ? 'checked' : ''}">${item.text}</span>
      </div>`;
    });
    html += '</div>';
  });
  container.innerHTML = html || '<div class="empty-state"><p>No items match this filter.</p></div>';
}

function escapeAttr(s) { return s.replace(/'/g, "\\'").replace(/"/g, '&quot;'); }

window.toggleReading = function(section, idx, checked) {
  state.readingChecks[section + '_' + idx] = checked;
  if (checked) addActivity('reading', `Finished reading an item in <strong>${section}</strong>`);
  saveStateQuiet();
  renderReading(document.querySelector('#tab-reading .filter-chip.active')?.dataset.filter || 'all');
  updateDashboard();
};

// ============================================
// TIMELINE
// ============================================
function initTimeline() {
  const container = document.getElementById('timeline-container');
  let html = '';
  PHASES.forEach(ph => {
    const projs = PROJECTS.filter(p => p.phase === ph.id);
    const done = projs.filter(p => state.projectStatus[p.id] === 'complete').length;
    const pct = projs.length ? Math.round((done / projs.length) * 100) : 0;
    const dotClass = pct === 100 ? 'complete' : pct > 0 ? 'in-progress' : '';

    html += `<div class="timeline-item">
      <div class="timeline-dot ${dotClass}"></div>
      <div class="timeline-card">
        <div class="timeline-months">Months ${ph.months} · Year ${ph.year}</div>
        <div class="timeline-title" style="color:${ph.color}">Phase ${ph.id} — ${ph.title}</div>
        <div class="timeline-desc">${done}/${projs.length} projects · ${pct}% complete</div>
        <div class="timeline-bar">
          <div class="timeline-bar-fill" style="width:${pct}%;background:${ph.color}"></div>
        </div>
      </div>
    </div>`;
  });
  container.innerHTML = html;
}

// ============================================
// CSV EXPORT
// ============================================
function exportCSV() {
  let csv = 'Section,ID,Item,Status,Phase,Year\n';

  // Projects
  PROJECTS.forEach(p => {
    const ph = PHASES.find(ph2 => ph2.id === p.phase);
    const st = state.projectStatus[p.id] || 'Not Started';
    csv += `Project,${p.id},"${p.name}",${st},${p.phase},${ph?.year}\n`;
  });

  // Gates
  GATES.forEach(g => {
    g.items.forEach((item, i) => {
      const checked = state.gateChecks[g.id + '_' + i] ? 'Checked' : 'Unchecked';
      csv += `Gate,${g.id}-${i + 1},"${item}",${checked},${g.phase},${PHASES.find(p => p.id === g.phase)?.year}\n`;
    });
  });

  // Reading
  READING.forEach(sec => {
    sec.items.forEach((item, i) => {
      const checked = state.readingChecks[sec.section + '_' + i] ? 'Read' : 'Unread';
      csv += `Reading,${sec.type},"${item.text}",${checked},${item.pri},\n`;
    });
  });

  // Sessions
  csv += '\n\nDate,Hours,Type,Phase,Notes\n';
  state.sessions.forEach(s => {
    csv += `${s.date},${s.hours},${s.type},${s.phase},"${s.notes || ''}"\n`;
  });

  const blob = new Blob([csv], { type: 'text/csv' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'efficient_ai_tracker_' + new Date().toISOString().slice(0, 10) + '.csv';
  a.click();
  showToast('CSV exported ✓');
}

// ============================================
// SAVE (quiet = no toast, triggers auto-save to file)
// ============================================
function saveStateQuiet() {
  saveToCacheOnly();
  scheduleAutoSave();
}

// ============================================
// INIT
// ============================================
document.getElementById('btn-save').addEventListener('click', saveState);
document.getElementById('btn-export-csv').addEventListener('click', exportCSV);
document.getElementById('btn-connect-file').addEventListener('click', connectDataFile);
document.getElementById('btn-create-file').addEventListener('click', createDataFile);
document.getElementById('btn-download-json').addEventListener('click', downloadJSON);
document.getElementById('file-import-input').addEventListener('change', handleFileImport);

if (!supportsFileAccess) {
  document.getElementById('btn-connect-file').title = 'Load data from a JSON file';
  document.getElementById('btn-create-file').title = 'Download data as JSON file';
}

// Initial render with cached data
updateDashboard();
initWeeklyLog();
initProjects();
initGates();
initReading();
initTimeline();

// Auto-load from tracker-data.json and reconnect file handle
(async function autoInit() {
  updateSyncUI('loading');
  
  // 1. Try to load data from the JSON file in the same directory
  const loaded = await autoLoadFromFetch();
  if (loaded) {
    refreshAllViews();
  }
  
  // 2. Try to reconnect a previously stored file handle (for auto-saving)
  const reconnected = await autoReconnectHandle();
  
  if (reconnected) {
    // Also refresh from the file handle (may be newer than fetch cache)
    await loadFromFileHandle();
    updateSyncUI('connected');
  } else if (loaded) {
    updateSyncUI('disconnected');
    // We loaded data but don't have write access — show hint
    showToast('Data loaded. Click "Open Data File" or "Save" to enable auto-sync.');
  } else {
    updateSyncUI('disconnected');
  }
})();

