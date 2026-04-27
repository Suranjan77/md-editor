import { PHASES, PROJECTS, GATES, READING, YEAR_COLORS } from './tracker-data.js';

const { invoke } = window.__TAURI__.core;

let state = {
  sessions: [],
  activities: [],
  kv: {},
  curriculum: null, // holds user custom config
  currentTab: 'dashboard'
};

export function initTracker(host) {
  host.addEventListener('click', handleGlobalClicks);
}

// Helper to get active content schema
function getAppSchema() {
  if (state.curriculum) {
    return {
      PHASES: state.curriculum.PHASES || PHASES,
      PROJECTS: state.curriculum.PROJECTS || PROJECTS,
      GATES: state.curriculum.GATES || GATES,
      READING: state.curriculum.READING || READING,
      YEAR_COLORS: state.curriculum.YEAR_COLORS || YEAR_COLORS
    };
  }
  return { PHASES, PROJECTS, GATES, READING, YEAR_COLORS };
}

export async function renderTracker() {
  const host = document.getElementById('tracker-host');
  if (!host) return;

  try {
    const sessions = await invoke('get_tracker_sessions');
    const activities = await invoke('get_tracker_activities');
    const kvArr = await invoke('get_tracker_kv');
    const cfgStr = await invoke('get_sys_config', { key: 'tracker_config' });

    state.sessions = sessions || [];
    state.activities = activities || [];
    state.kv = {};
    if (kvArr) kvArr.forEach(item => { state.kv[item.key] = item.value; });

    if (cfgStr) {
      try { state.curriculum = JSON.parse(cfgStr); } catch (e) { console.error("Bad JSON config", e); }
    }
  } catch (err) {
    console.error("Failed to load tracker data:", err);
  }

  host.innerHTML = `
    <div class="w-full max-w-6xl mx-auto p-8 font-sans animate-in fade-in duration-500 pb-32">
      <!-- Header -->
      <header class="flex items-end justify-between mb-12 border-b border-[var(--border)] pb-6">
        <div>
          <h1 class="text-3xl font-extrabold text-[var(--accent)] tracking-tight mb-2">Efficient AI Curriculum</h1>
          <p class="text-[var(--text-muted)] font-medium tracking-wide uppercase text-[11px]">Study & Progress Tracker</p>
        </div>
      </header>

      <!-- Navigation -->
      <nav class="flex gap-2 mb-10 overflow-x-auto pb-2 border-b border-[var(--border-subtle)] pb-4">
        ${renderTabButton('dashboard', 'grid_view', 'Dashboard')}
        ${renderTabButton('log', 'history', 'Study Log')}
        ${renderTabButton('projects', 'rocket_launch', 'Projects')}
        ${renderTabButton('gates', 'fact_check', 'Evaluation Gates')}
        ${renderTabButton('reading', 'menu_book', 'Reading List')}
        <div class="flex-grow"></div>
        ${renderTabButton('config', 'settings', 'Config')}
      </nav>

      <!-- Tab Content Area -->
      <main id="tracker-content-area" class="relative">
        ${renderCurrentTab()}
      </main>
    </div>
  `;
}

function renderTabButton(id, icon, label) {
  const isActive = state.currentTab === id;
  const activeClass = isActive
    ? 'bg-[var(--accent-dim)] text-[var(--accent)] border-[var(--accent)] ring-1 ring-[var(--accent-glow)] shadow-[0_0_15px_var(--accent-glow)]'
    : 'bg-[var(--bg-secondary)] text-[var(--text-muted)] border-[var(--border-subtle)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent)]';
  return `
    <button data-action="switch-tab" data-target="${id}" class="flex items-center gap-2 px-6 py-2.5 rounded-lg border transition-all duration-300 font-semibold text-[13px] cursor-pointer whitespace-nowrap ${activeClass}">
      <span class="material-symbols-outlined !text-[18px]">${icon}</span>
      ${label}
    </button>
  `;
}

function renderCurrentTab() {
  switch (state.currentTab) {
    case 'dashboard': return renderDashboard();
    case 'log': return renderStudyLog();
    case 'projects': return renderProjects();
    case 'gates': return renderGates();
    case 'reading': return renderReading();
    case 'config': return renderConfig();
    default: return renderDashboard();
  }
}

// ==========================================
// CORE DASHBOARD
// ==========================================
function renderDashboard() {
  const schema = getAppSchema();
  const totalHours = state.sessions.reduce((a, s) => a + (s.hours || 0), 0).toFixed(1);

  let doneProjects = 0;
  schema.PROJECTS.forEach(p => { if (state.kv[`proj_${p.id}`] === 'complete') doneProjects++; });
  const projPct = schema.PROJECTS.length ? Math.round((doneProjects / schema.PROJECTS.length) * 100) : 0;

  let totalItems = 0, doneItems = 0;
  schema.GATES.forEach(g => {
    g.items.forEach((_, i) => {
      totalItems++;
      if (state.kv[`gate_${g.id}_${i}`] === 'true') doneItems++;
    });
  });

  return `
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-10 animate-in slide-in-from-bottom-4 duration-500 fade-in">
      ${renderKpiCard('Total Study Hours', `${totalHours} h`, 'schedule')}
      ${renderKpiCard('Projects Finished', `${doneProjects} / ${schema.PROJECTS.length}`, 'rocket_launch')}
      ${renderKpiCard('Gates Passed', `${doneItems} / ${totalItems}`, 'done_all')}
      ${renderKpiCard('Overall Progress', `${projPct}%`, 'trending_up')}
    </div>

    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6 animate-in slide-in-from-bottom-8 duration-700 fade-in">
      <div class="lg:col-span-2 bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] p-8">
        <h3 class="text-lg font-bold text-[var(--text-primary)] mb-6 flex items-center gap-2"><span class="material-symbols-outlined text-[var(--accent)]">bar_chart</span> Weekly Activity</h3>
        <div class="h-64 flex items-end gap-2" id="activity-chart">
          ${renderActivityChartBars()}
        </div>
      </div>
      <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] p-8 flex flex-col">
        <h3 class="text-lg font-bold text-[var(--text-primary)] mb-6 flex items-center gap-2"><span class="material-symbols-outlined text-[var(--success)]">history</span> Recent Feed</h3>
        <div class="flex-grow overflow-y-auto pr-2 space-y-4 custom-scrollbar">
          ${renderActivityFeed()}
        </div>
      </div>
    </div>
  `;
}

function renderActivityChartBars() {
  const days = 14;
  const dataMap = {};
  const today = new Date();

  for (let i = days - 1; i >= 0; i--) {
    let d = new Date(); d.setDate(today.getDate() - i);
    dataMap[d.toISOString().slice(0, 10)] = 0;
  }

  state.sessions.forEach(s => {
    if (dataMap[s.date] !== undefined) dataMap[s.date] += s.hours;
  });

  const maxVal = Math.max(...Object.values(dataMap), 4);
  let html = '';

  Object.keys(dataMap).sort().forEach(dateStr => {
    const hours = dataMap[dateStr];
    const hPct = (hours / maxVal) * 100;
    const label = new Date(dateStr).toLocaleDateString('en-US', { weekday: 'short' });
    const isToday = dateStr === today.toISOString().slice(0, 10);
    html += `
      <div class="flex-1 flex flex-col items-center justify-end h-full group">
        <div class="text-[10px] text-[var(--text-primary)] font-bold mb-2 opacity-0 group-hover:opacity-100 transition-opacity">${hours.toFixed(1)}h</div>
        <div class="w-full max-w-[2rem] bg-[var(--accent)] opacity-80 rounded-t border-t border-[var(--accent)] transition-all duration-300 group-hover:opacity-100 ${isToday ? 'ring-2 ring-[var(--accent)] ring-offset-2 ring-offset-[var(--bg-secondary)]' : ''}" style="height: ${Math.max(hPct, 2)}%"></div>
        <div class="text-[10px] mt-3 uppercase tracking-wider font-mono ${isToday ? 'text-[var(--accent)] font-bold' : 'text-[var(--text-muted)]'}">${label.charAt(0)}</div>
      </div>
    `;
  });
  return html;
}

function renderActivityFeed() {
  if (!state.activities.length) return `<div class="text-xs text-[var(--text-muted)] italic">No recent activity.</div>`;
  return state.activities.slice(0, 15).map(a => `
    <div class="flex gap-4 group">
      <div class="flex flex-col items-center">
        <div class="w-2 h-2 rounded-full bg-[var(--accent)] shadow-[0_0_8px_var(--accent-glow)] group-hover:scale-125 transition-transform mt-1.5"></div>
        <div class="w-px h-full bg-[var(--border)] my-1 group-last:hidden"></div>
      </div>
      <div class="pb-4">
        <div class="text-[13px] text-[var(--text-primary)] leading-snug">${a.text}</div>
        <div class="text-[10px] text-[var(--text-muted)] mt-1 font-mono uppercase">${new Date(a.time).toLocaleString()}</div>
      </div>
    </div>
  `).join('');
}

function renderKpiCard(title, value, icon) {
  return `
    <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] p-6 flex items-center gap-5 shadow-md">
      <div class="w-12 h-12 rounded-xl flex items-center justify-center bg-[var(--bg-tertiary)] text-[var(--accent)]">
        <span class="material-symbols-outlined !text-[24px]">${icon}</span>
      </div>
      <div>
        <div class="text-2xl font-black font-mono text-[var(--text-primary)]">${value}</div>
        <div class="text-[var(--text-muted)] text-[11px] uppercase tracking-wider font-semibold mt-1">${title}</div>
      </div>
    </div>
  `;
}

// ==========================================
// STUDY LOG
// ==========================================
function renderStudyLog() {
  return `
    <div class="grid grid-cols-1 xl:grid-cols-3 gap-8 animate-in slide-in-from-right-8 duration-500 fade-in">
      <div class="xl:col-span-1">
        <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] p-8 sticky top-8 shadow-md">
          <h3 class="text-[15px] font-bold text-[var(--text-primary)] mb-6 flex items-center gap-2">
            <span class="material-symbols-outlined text-[var(--accent)]">add_circle</span> Log Session
          </h3>
          <div class="space-y-4">
            <div>
              <label class="block text-[10px] uppercase font-semibold text-[var(--text-muted)] mb-1.5">Date</label>
              <input type="date" id="log-date" class="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent)]" value="${new Date().toISOString().slice(0, 10)}">
            </div>
            <div>
              <label class="block text-[10px] uppercase font-semibold text-[var(--text-muted)] mb-1.5">Hours</label>
              <input type="number" id="log-hours" step="0.25" min="0.25" placeholder="e.g. 2.5" class="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent)]">
            </div>
            <div>
              <label class="block text-[10px] uppercase font-semibold text-[var(--text-muted)] mb-1.5">Type</label>
              <select id="log-type" class="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent)]">
                <option value="theory-study">Theory & Math</option>
                <option value="paper-reading">Paper Reading</option>
                <option value="implementation">Implementation</option>
                <option value="review-writeup">Review & Writeup</option>
              </select>
            </div>
            <div>
              <label class="block text-[10px] uppercase font-semibold text-[var(--text-muted)] mb-1.5">Notes</label>
              <input type="text" id="log-notes" placeholder="Studied FlashAttention..." class="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent)]">
            </div>
            <button data-action="submit-log" class="w-full py-3 mt-2 bg-[var(--accent)] hover:bg-[#cde8e2] text-[#0d0e10] rounded-lg font-bold shadow-md transition-all flex justify-center items-center gap-2">
              <span class="material-symbols-outlined !text-[18px]">save</span> Save Session
            </button>
          </div>
        </div>
      </div>
      
      <div class="xl:col-span-2">
        <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] overflow-hidden shadow-md">
          <table class="w-full text-left text-sm">
            <thead class="bg-[var(--bg-tertiary)] text-[var(--text-muted)] text-[11px] uppercase font-semibold">
              <tr>
                <th class="px-5 py-3">Date</th>
                <th class="px-5 py-3">Hours</th>
                <th class="px-5 py-3">Type</th>
                <th class="px-5 py-3">Notes</th>
                <th class="px-5 py-3 text-right">Delete</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-[var(--border-subtle)] text-[var(--text-primary)]">
              ${state.sessions.length ? state.sessions.map(s => `
                <tr class="hover:bg-[var(--bg-tertiary)] transition-colors group">
                  <td class="px-5 py-4 font-mono text-[11px]">${s.date}</td>
                  <td class="px-5 py-4 font-bold text-[var(--accent)]">${s.hours}h</td>
                  <td class="px-5 py-4 text-[13px] capitalize">${s.activity_type.replace('-', ' ')}</td>
                  <td class="px-5 py-4 text-[var(--text-secondary)] italic text-[13px]">${s.notes || '--'}</td>
                  <td class="px-5 py-4 text-right">
                    <button data-action="delete-log" data-id="${s.id}" class="text-[var(--danger)] opacity-0 group-hover:opacity-100 transition-opacity p-1">
                      <span class="material-symbols-outlined !text-[18px]">delete</span>
                    </button>
                  </td>
                </tr>
              `).join('') : `<tr><td colspan="5" class="py-12 text-center text-[var(--text-muted)] italic">No sessions logged yet.</td></tr>`}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  `;
}

// ==========================================
// PROJECTS & GATES
// ==========================================
function renderProjects() {
  const schema = getAppSchema();
  let html = `<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6 animate-in slide-in-from-left-8 duration-500 fade-in">`;
  schema.PHASES.forEach(phase => {
    const phaseProjs = schema.PROJECTS.filter(p => p.phase === phase.id);
    if (!phaseProjs.length) return;

    html += `
      <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] shadow-md overflow-hidden flex flex-col relative">
        <div class="p-5 border-b border-[var(--border-subtle)] bg-[var(--bg-tertiary)]">
          <div class="text-[10px] uppercase font-bold text-[var(--text-muted)] mb-1">Phase ${phase.id} &bull; Year ${phase.year}</div>
          <h3 class="font-bold text-[15px] text-[var(--accent)] leading-tight">${phase.title}</h3>
        </div>
        <div class="p-4 flex-grow overflow-y-auto custom-scrollbar max-h-96 space-y-3">
          ${phaseProjs.map(proj => {
      const status = state.kv[`proj_${proj.id}`] || 'not_started';
      const isDone = status === 'complete';
      const isProg = status === 'in_progress';
      let selClass = isDone ? 'bg-[var(--success)]/10 text-[var(--success)] border-[var(--success)]/30' :
        isProg ? 'bg-[var(--warning)]/10 text-[var(--warning)] border-[var(--warning)]/30' :
          'bg-[var(--bg-primary)] text-[var(--text-muted)] border-[var(--border-subtle)]';

      return `
              <div class="p-3.5 rounded-xl border ${isDone ? 'border-[var(--success)]/20 bg-[var(--success)]/5' : 'border-[var(--border)] bg-[var(--bg-primary)]'} hover:border-[var(--accent)] transition-colors">
                <div class="text-[13px] font-medium text-[var(--text-primary)] mb-2.5 leading-snug">${proj.name}</div>
                <select data-action="update-proj" data-id="${proj.id}" class="w-full text-[11px] font-bold rounded-md px-2 py-1.5 border outline-none cursor-pointer ${selClass}">
                  <option value="not_started" ${status === 'not_started' ? 'selected' : ''}>Not Started</option>
                  <option value="in_progress" ${status === 'in_progress' ? 'selected' : ''}>⏳ In Progress</option>
                  <option value="complete" ${status === 'complete' ? 'selected' : ''}>✅ Complete</option>
                </select>
              </div>
            `;
    }).join('')}
        </div>
      </div>
    `;
  });
  html += `</div>`;
  return html;
}

function renderGates() {
  const schema = getAppSchema();
  let html = `<div class="grid grid-cols-1 xl:grid-cols-2 gap-8 animate-in slide-in-from-bottom-8 duration-500 fade-in">`;
  schema.GATES.forEach(gate => {
    let done = 0;
    gate.items.forEach((_, i) => { if (state.kv[`gate_${gate.id}_${i}`] === 'true') done++; });
    const isComplete = done === gate.items.length;

    html += `
      <div class="bg-[var(--bg-secondary)] rounded-2xl border ${isComplete ? 'border-[var(--success)]/50' : 'border-[var(--border)]'} shadow-md p-6 relative overflow-hidden transition-colors">
        <div class="flex items-center justify-between mb-5">
          <h3 class="text-[16px] font-bold text-[var(--text-primary)] flex items-center gap-2">
            <span class="material-symbols-outlined !text-[20px] ${isComplete ? 'text-[var(--success)]' : 'text-[var(--accent)]'}">${isComplete ? 'verified' : 'fact_check'}</span> 
            ${gate.title}
          </h3>
          <span class="px-2 py-0.5 rounded text-[11px] font-bold font-mono ${isComplete ? 'bg-[var(--success)]/20 text-[var(--success)]' : 'bg-[var(--bg-tertiary)] text-[var(--text-muted)]'}">${done}/${gate.items.length}</span>
        </div>
        <div class="space-y-2">
          ${gate.items.map((item, i) => {
      const checked = state.kv[`gate_${gate.id}_${i}`] === 'true';
      return `
              <label class="flex items-start gap-3 p-3 rounded-xl border border-transparent transition-all cursor-pointer ${checked ? 'bg-[var(--success)]/5 border-[var(--success)]/20' : 'bg-[var(--bg-primary)] hover:border-[var(--border)]'}">
                <input type="checkbox" data-action="toggle-gate" data-gate="${gate.id}" data-item="${i}" ${checked ? 'checked' : ''} class="mt-0.5 w-4 h-4 accent-[var(--accent)] cursor-pointer flex-shrink-0 relative top-0.5">
                <span class="text-[13.5px] leading-relaxed ${checked ? 'text-[var(--text-muted)] line-through' : 'text-[var(--text-primary)]'}">${item}</span>
              </label>
            `;
    }).join('')}
        </div>
      </div>
    `;
  });
  html += `</div>`;
  return html;
}

function renderReading() {
  const schema = getAppSchema();
  let html = `<div class="grid grid-cols-1 lg:grid-cols-2 gap-6 animate-in slide-in-from-bottom-8 duration-500 fade-in">`;
  schema.READING.forEach(section => {
    html += `
      <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] shadow-md overflow-hidden flex flex-col">
        <div class="p-5 border-b border-[var(--border-subtle)] bg-[var(--bg-tertiary)]">
          <div class="text-[10px] uppercase font-bold text-[var(--text-muted)] mb-1">${section.type}</div>
          <h3 class="font-bold text-[15px] text-[var(--accent)] leading-tight">${section.section}</h3>
        </div>
        <div class="p-3 flex-grow overflow-y-auto custom-scrollbar max-h-96 space-y-1">
          ${section.items.map((item, i) => {
      const checked = state.kv[`read_${section.section.replace(/\\s+/g, '')}_${i}`] === 'true';
      const icon = item.pri === 'critical' ? '🔴' : item.pri === 'important' ? '🟡' : '🔵';
      return `
              <label class="flex items-start gap-3 p-2.5 rounded-lg transition-all cursor-pointer hover:bg-[var(--bg-tertiary)] group">
                <input type="checkbox" data-action="toggle-read" data-sec="${section.section.replace(/\\s+/g, '')}" data-item="${i}" ${checked ? 'checked' : ''} class="mt-0.5 w-3.5 h-3.5 accent-[var(--accent)] cursor-pointer flex-shrink-0 relative top-[3px]">
                <span class="text-[10px] flex-shrink-0 mt-1">${icon}</span>
                <span class="text-[13px] leading-snug ${checked ? 'text-[var(--text-muted)] line-through' : 'text-[var(--text-primary)]'}">${item.text}</span>
              </label>
            `;
    }).join('')}
        </div>
      </div>
    `;
  });
  html += `</div>`;
  return html;
}

// ==========================================
// CONFIGURATION EDITOR
// ==========================================
function renderConfig() {
  const activeSchemaJSON = JSON.stringify(getAppSchema(), null, 2);

  return `
    <div class="animate-in slide-in-from-bottom-8 duration-500 fade-in max-w-4xl">
      <div class="bg-[var(--bg-secondary)] rounded-2xl border border-[var(--border)] p-8 shadow-md">
        <div class="flex items-center justify-between mb-6">
          <h3 class="text-[16px] font-bold text-[var(--text-primary)] flex items-center gap-2">
            <span class="material-symbols-outlined text-[var(--warning)]">data_object</span> JSON Schema Editor
          </h3>
          <span class="text-[11px] text-[var(--text-muted)] uppercase">Curriculum Content</span>
        </div>
        
        <p class="text-[13px] text-[var(--text-muted)] mb-5">
          Modify the underlying data structure of your tracker. Changing IDs may orphan your existing checkboxes and logged progress, so proceed with caution. Click "Save Configuration" to commit changes to the local SQLite database.
        </p>

        <textarea 
          id="config-json-editor" 
          class="w-full h-[500px] bg-[var(--bg-primary)] text-[var(--text-primary)] border border-[var(--border)] rounded-lg p-4 font-mono text-[12px] leading-relaxed focus:outline-none focus:border-[var(--accent)] whitespace-pre custom-scrollbar"
        >${activeSchemaJSON}</textarea>
        
        <div class="flex justify-end mt-6">
          <button data-action="save-config" class="px-6 py-2.5 bg-[var(--warning)] hover:brightness-110 text-[#0d0e10] rounded-lg font-bold shadow-md transition-all flex items-center gap-2 text-[13px]">
            <span class="material-symbols-outlined !text-[16px]">save</span> Save Configuration
          </button>
        </div>
      </div>
    </div>
  `;
}

// ==========================================
// ACTIONS & EVENT BINDING
// ==========================================
async function handleGlobalClicks(e) {
  const target = e.target.closest('[data-action]');
  if (!target) return;

  const action = target.getAttribute('data-action');

  if (action === 'switch-tab') {
    state.currentTab = target.getAttribute('data-target');
    renderTracker();
  }
  else if (action === 'submit-log') {
    const date = document.getElementById('log-date').value;
    const hours = parseFloat(document.getElementById('log-hours').value);
    const type = document.getElementById('log-type').value;
    const notes = document.getElementById('log-notes').value;

    if (!date || isNaN(hours) || hours <= 0) return alert('Invalid date or hours');

    await invoke('add_tracker_session', {
      session: { date, hours, activity_type: type, phase: '1A', notes: notes || null }
    });

    await logActivity('session', `Logged <strong>${hours}h</strong> of ${type.replace('-', ' ')}`);
    await renderTracker();
  }
  else if (action === 'delete-log') {
    const id = parseInt(target.getAttribute('data-id'), 10);
    await invoke('delete_tracker_session', { id });
    await renderTracker();
  }
  else if (action === 'save-config') {
    const rawJson = document.getElementById('config-json-editor').value;
    try {
      // Validate schema
      const parsed = JSON.parse(rawJson);
      if (!parsed.PHASES || !parsed.PROJECTS) throw new Error("Missing required schema roots (PHASES, PROJECTS)");

      await invoke('set_sys_config', { key: 'tracker_config', value: rawJson });
      state.curriculum = parsed;
      await renderTracker();

      const toastContainer = document.getElementById('toast-container');
      if (toastContainer) {
        showToast("Tracker configuration saved to SQLite.", "success");
      } else {
        alert("Configuration Saved!");
      }
    } catch (e) {
      alert("Invalid JSON data: " + e.message);
    }
  }
}

// Binding change events primarily for updating tracker KV toggles
document.addEventListener('change', async (e) => {
  if (!document.getElementById('tracker-host')?.contains(e.target)) return;
  const target = e.target;
  const action = target.getAttribute('data-action');

  if (action === 'update-proj') {
    const projId = target.getAttribute('data-id');
    const val = target.value;
    await invoke('set_tracker_kv', { key: `proj_${projId}`, value: val });
    if (val === 'complete') await logActivity('project', `Completed project <strong>${projId}</strong>`);
    renderTracker();
  }
  else if (action === 'toggle-gate') {
    const gateObj = target.getAttribute('data-gate');
    const itemIdx = target.getAttribute('data-item');
    const checked = target.checked.toString();
    await invoke('set_tracker_kv', { key: `gate_${gateObj}_${itemIdx}`, value: checked });
    if (checked === 'true') await logActivity('gate', `Checked off an item in gate <strong>${gateObj}</strong>`);
    renderTracker();
  }
  else if (action === 'toggle-read') {
    const sec = target.getAttribute('data-sec');
    const itemIdx = target.getAttribute('data-item');
    const checked = target.checked.toString();
    await invoke('set_tracker_kv', { key: `read_${sec}_${itemIdx}`, value: checked });
    renderTracker();
  }
});

async function logActivity(type, text) {
  try {
    await invoke('add_tracker_activity', {
      activity: { type, text, time: new Date().toISOString() }
    });
  } catch (err) { console.error(err); }
}

// Optional helper hook assuming md-editor showToast exists in global scope if strictly needed
function showToast(msg, type) {
  if (window.showToast) window.showToast(msg, type);
}
