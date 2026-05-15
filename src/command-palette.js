/**
 * Command palette — fuzzy-filtered quick actions (Ctrl+P).
 */

let overlay = null;
let input = null;
let list = null;
let commands = [];
let filtered = [];
let selectedIndex = 0;
let onRun = null;
let isOpen = false;

export function initCommandPalette({ onExecute }) {
  overlay = document.getElementById("command-palette");
  input = document.getElementById("command-palette-input");
  list = document.getElementById("command-palette-list");
  onRun = onExecute;

  if (!overlay || !input || !list) return;

  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) hideCommandPalette();
  });

  input.addEventListener("input", () => {
    selectedIndex = 0;
    renderList();
  });

  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, filtered.length - 1);
      renderList();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
      renderList();
    } else if (e.key === "Enter") {
      e.preventDefault();
      runSelected();
    } else if (e.key === "Escape") {
      e.preventDefault();
      hideCommandPalette();
    }
  });
}

export function setCommandPaletteCommands(cmds) {
  commands = cmds;
}

export function showCommandPalette() {
  if (!overlay || !input) return;
  isOpen = true;
  overlay.classList.remove("hidden");
  input.value = "";
  selectedIndex = 0;
  renderList();
  requestAnimationFrame(() => input.focus());
}

export function hideCommandPalette() {
  if (!overlay) return;
  isOpen = false;
  overlay.classList.add("hidden");
  input.value = "";
}

export function toggleCommandPalette() {
  if (isOpen) hideCommandPalette();
  else showCommandPalette();
}

export function isCommandPaletteOpen() {
  return isOpen;
}

function scoreMatch(query, cmd) {
  const q = query.toLowerCase();
  if (!q) return 1;
  const label = cmd.label.toLowerCase();
  const keywords = (cmd.keywords || "").toLowerCase();
  if (label.startsWith(q)) return 100;
  if (label.includes(q)) return 60;
  if (keywords.includes(q)) return 40;
  const parts = q.split(/\s+/).filter(Boolean);
  if (parts.every((p) => label.includes(p) || keywords.includes(p))) return 30;
  return 0;
}

function renderList() {
  const query = input.value.trim();
  filtered = commands
    .filter((c) => !c.when || c.when())
    .map((c) => ({ cmd: c, score: scoreMatch(query, c) }))
    .filter((x) => x.score > 0 || !query)
    .sort((a, b) => b.score - a.score)
    .map((x) => x.cmd);

  if (selectedIndex >= filtered.length) selectedIndex = Math.max(0, filtered.length - 1);

  list.innerHTML = "";
  if (!filtered.length) {
    list.innerHTML = '<div class="command-palette-empty">No matching commands</div>';
    return;
  }

  filtered.forEach((cmd, i) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = `command-palette-item${i === selectedIndex ? " is-selected" : ""}`;
    row.innerHTML = `
      <span class="material-symbols-outlined command-palette-icon">${cmd.icon}</span>
      <span class="command-palette-label">${cmd.label}</span>
      ${cmd.shortcut ? `<kbd class="command-palette-kbd">${cmd.shortcut}</kbd>` : ""}
    `;
    row.addEventListener("click", () => {
      selectedIndex = i;
      runSelected();
    });
    row.addEventListener("mouseenter", () => {
      selectedIndex = i;
      renderList();
    });
    list.appendChild(row);
  });

  const selected = list.querySelector(".is-selected");
  selected?.scrollIntoView({ block: "nearest" });
}

function runSelected() {
  const cmd = filtered[selectedIndex];
  if (!cmd) return;
  hideCommandPalette();
  onRun?.(cmd.id);
}
