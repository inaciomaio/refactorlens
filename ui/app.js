// RefactorLens front end. Plain JavaScript, no build step, so anyone can hack on it.
"use strict";

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => [...document.querySelectorAll(sel)];

const CATEGORY_LABELS = {
  "readability": "Readability",
  "modern-syntax": "Modern syntax",
  "performance": "Performance",
  "safety": "Safety",
  "error-handling": "Error handling",
  "library": "Library",
  "structure": "Structure",
  "bug-fix": "Bug fix",
};

const PROVIDER_LABELS = {
  ollama: "Ollama",
  anthropic: "Anthropic",
  openai: "OpenAI-compatible",
  demo: "Demo",
};

// highlight.js names for languages people usually type differently.
const LANG_ALIASES = {
  "c++": "cpp", "c#": "csharp", "js": "javascript", "node": "javascript",
  "ts": "typescript", "shell": "bash", "sh": "bash", "golang": "go",
  "py": "python", "rs": "rust", "kt": "kotlin", "objective-c": "objectivec",
};

const FILE_EXT = {
  python: "py", javascript: "js", typescript: "ts", rust: "rs", go: "go", java: "java",
  kotlin: "kt", c: "c", cpp: "cpp", csharp: "cs", swift: "swift", php: "php", ruby: "rb",
  lua: "lua", bash: "sh", sql: "sql",
};

// ---------------------------------------------------------------------------
// Storage. Settings live in localStorage. API keys live in sessionStorage,
// so they disappear when the tab closes.
// ---------------------------------------------------------------------------
const store = {
  get(key, fallback, area = localStorage) {
    try {
      const raw = area.getItem("refactorlens." + key);
      return raw === null ? fallback : JSON.parse(raw);
    } catch { return fallback; }
  },
  set(key, value, area = localStorage) {
    try { area.setItem("refactorlens." + key, JSON.stringify(value)); } catch { /* storage may be blocked */ }
  },
};

const state = {
  config: null,
  provider: store.get("provider", null), // { kind, models: {}, baseUrls: {} }
  keys: store.get("keys", {}, sessionStorage),
  ollamaModels: [],
  ollamaReachable: null,
  result: null,
  active: null,
  view: "split",
  hlCache: new Map(),
  hlLang: null,
  abort: null,
};

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------
function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

// Turn `backticks` into <code>, after escaping everything else.
function richText(s) {
  return escapeHtml(s || "").replace(/`([^`]+)`/g, "<code>$1</code>");
}

function toast(message) {
  const t = $("#toast");
  t.textContent = message;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { t.hidden = true; }, 2200);
}

async function api(path, options = {}) {
  const res = await fetch("/api" + path, {
    ...options,
    headers: { "content-type": "application/json", "x-refactorlens": "1", ...(options.headers || {}) },
  });
  let body = null;
  try { body = await res.json(); } catch { /* not JSON */ }
  if (!res.ok) throw new Error((body && body.error) || `The server answered ${res.status}.`);
  return body;
}

const reduceMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
// Matches the CSS breakpoint where lessons stack above the code.
const narrow = () => window.matchMedia("(max-width: 1080px)").matches;

// ---------------------------------------------------------------------------
// Provider settings
// ---------------------------------------------------------------------------
function providerModel(kind) {
  const p = state.provider || {};
  const saved = (p.models || {})[kind];
  if (saved) return saved;
  const c = state.config || {};
  if (kind === "ollama") return (c.ollama && c.ollama.model) || state.ollamaModels[0] || "";
  if (kind === "anthropic") return (c.anthropic && c.anthropic.model) || "";
  if (kind === "openai") return (c.openai && c.openai.model) || "";
  return "";
}

function providerBaseUrl(kind) {
  const p = state.provider || {};
  return ((p.baseUrls || {})[kind]) || "";
}

function defaultBaseUrl(kind) {
  const c = state.config || {};
  if (kind === "ollama") return (c.ollama && c.ollama.base_url) || "http://127.0.0.1:11434";
  if (kind === "openai") return (c.openai && c.openai.base_url) || "https://api.openai.com/v1";
  if (kind === "anthropic") return "https://api.anthropic.com";
  return "";
}

function keyIsAvailable(kind) {
  const c = state.config || {};
  if (state.keys[kind]) return true;
  return Boolean(c[kind] && c[kind].key_set);
}

function chooseDefaultProvider() {
  if (state.provider && state.provider.kind) return;
  let kind = "demo";
  if (keyIsAvailable("anthropic")) kind = "anthropic";
  else if (state.ollamaModels.length) kind = "ollama";
  state.provider = { kind, models: {}, baseUrls: {} };
}

function renderModelChip() {
  const kind = state.provider.kind;
  const model = kind === "demo" ? "recorded example" : providerModel(kind) || "no model chosen";
  $("#model-label").textContent = `${PROVIDER_LABELS[kind]}: ${model}`;
  const dot = $("#model-dot");
  let ok = true;
  if (kind === "ollama") ok = state.ollamaReachable !== false && Boolean(providerModel(kind));
  if (kind === "anthropic") ok = keyIsAvailable("anthropic");
  if (kind === "openai") ok = Boolean(providerModel(kind));
  dot.className = "model-dot " + (ok ? "ok" : "warn");
  $("#open-settings").title = ok ? "Change model" : "This model needs setting up";
}

async function refreshOllamaModels(baseUrl) {
  try {
    const q = baseUrl ? "?base_url=" + encodeURIComponent(baseUrl) : "";
    const r = await api("/ollama/models" + q);
    state.ollamaModels = r.models || [];
    state.ollamaReachable = !r.unreachable;
  } catch {
    state.ollamaModels = [];
    state.ollamaReachable = false;
  }
}

// ----- Settings dialog -----
function selectedProviderInDialog() {
  const checked = $$('input[name="provider"]').find((i) => i.checked);
  return checked ? checked.value : "demo";
}

async function updateDialogFields() {
  const kind = selectedProviderInDialog();
  $$("#provider-fields .field").forEach((f) => {
    f.hidden = !f.dataset.for.split(" ").includes(kind);
  });
  const model = $("#model");
  const base = $("#base-url");
  const key = $("#api-key");
  model.value = providerModel(kind);
  base.value = providerBaseUrl(kind);
  base.placeholder = defaultBaseUrl(kind);
  key.value = state.keys[kind] || "";

  const keyNote = $("#key-note");
  const envName = kind === "anthropic" ? "ANTHROPIC_API_KEY" : "OPENAI_API_KEY";
  const envSet = state.config && state.config[kind] && state.config[kind].key_set;
  key.placeholder = envSet ? "Using the key from your environment" : "Paste your key";
  keyNote.textContent = kind === "openai"
    ? `Local servers usually need no key. Otherwise paste one, or set ${envName}. Keys stay in this tab and go only to your local RefactorLens server.`
    : `${envSet ? `Found ${envName} in the environment. ` : `Or set ${envName} before starting the app. `}Keys stay in this tab and go only to your local RefactorLens server.`;

  const note = $("#model-note");
  const list = $("#model-options");
  list.innerHTML = "";
  note.textContent = "";
  model.placeholder = "";
  if (kind === "ollama") {
    note.textContent = "Looking for installed models…";
    await refreshOllamaModels(base.value);
    if (selectedProviderInDialog() !== "ollama") return;
    state.ollamaModels.forEach((m) => {
      const o = document.createElement("option");
      o.value = m;
      list.appendChild(o);
    });
    if (state.ollamaReachable === false) {
      note.textContent = `Ollama isn't answering at ${base.value || base.placeholder}. Start it with "ollama serve".`;
    } else if (!state.ollamaModels.length) {
      note.textContent = "Ollama is running but has no models. Try: ollama pull qwen2.5-coder:7b";
    } else {
      note.textContent = `${state.ollamaModels.length} installed. Coding models such as qwen2.5-coder give the best lessons.`;
      if (!model.value) model.value = state.ollamaModels[0];
    }
    model.placeholder = "e.g. qwen2.5-coder:7b";
  } else if (kind === "anthropic") {
    model.placeholder = "claude-sonnet-5-5";
  } else if (kind === "openai") {
    model.placeholder = "The model name your server expects";
  }
}

function openSettings() {
  const kind = state.provider.kind;
  $$('input[name="provider"]').forEach((i) => { i.checked = i.value === kind; });
  updateDialogFields();
  $("#settings").showModal();
}

function saveSettings() {
  const kind = selectedProviderInDialog();
  const p = state.provider;
  p.kind = kind;
  p.models = p.models || {};
  p.baseUrls = p.baseUrls || {};
  if (kind !== "demo") {
    p.models[kind] = $("#model").value.trim();
    p.baseUrls[kind] = $("#base-url").value.trim();
    const key = $("#api-key").value.trim();
    if (key) state.keys[kind] = key; else delete state.keys[kind];
  }
  store.set("provider", p);
  store.set("keys", state.keys, sessionStorage);
  renderModelChip();
  toast(`Using ${PROVIDER_LABELS[kind]}`);
}

// ---------------------------------------------------------------------------
// Compose
// ---------------------------------------------------------------------------
function readOptions() {
  return {
    allow_libraries: $("#allow-libraries").getAttribute("aria-checked") === "true",
    level: ($$('input[name="level"]').find((i) => i.checked) || {}).value || "intermediate",
    focus: $$("#focus input:checked").map((i) => i.value),
    target: $("#target").value.trim(),
    language: $("#language").value === "auto" ? "" : $("#language").value,
  };
}

function saveDraft() {
  store.set("draft", { code: $("#code").value, ...readOptions() });
}

function restoreDraft() {
  const d = store.get("draft", null);
  if (!d) return;
  $("#code").value = d.code || "";
  setLibraries(Boolean(d.allow_libraries));
  $$('input[name="level"]').forEach((i) => { i.checked = i.value === (d.level || "intermediate"); });
  $$("#focus input").forEach((i) => { i.checked = (d.focus || []).includes(i.value); });
  $("#target").value = d.target || "";
  $("#language").value = d.language || "auto";
}

function setLibraries(on) {
  $("#allow-libraries").setAttribute("aria-checked", String(on));
  $("#libraries-help").textContent = on
    ? "May suggest well-known packages. Each one is listed with how to install it."
    : "Standard library only. Turn on to let it suggest well-known packages.";
}

// Tab inserts spaces. Esc releases the next Tab so keyboard users can leave.
function wireEditor() {
  const ta = $("#code");
  let escaped = false;
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Escape") { escaped = true; return; }
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") { e.preventDefault(); run(); return; }
    if (e.key === "Tab" && !escaped) {
      e.preventDefault();
      const { selectionStart: s, selectionEnd: end, value } = ta;
      if (e.shiftKey) {
        // Outdent the current line by up to four spaces.
        const lineStart = value.lastIndexOf("\n", s - 1) + 1;
        const m = value.slice(lineStart).match(/^ {1,4}/);
        if (m) {
          ta.setRangeText("", lineStart, lineStart + m[0].length, "preserve");
        }
      } else {
        ta.setRangeText("    ", s, end, "end");
      }
      saveDraft();
      return;
    }
    escaped = false;
  });
  ta.addEventListener("input", saveDraft);
}

function setBusy(busy, label = "") {
  $("#run").disabled = busy;
  $("#run").textContent = busy ? "Improving…" : "Improve my code";
  $("#progress").hidden = !busy;
  clearInterval(setBusy.timer);
  if (busy) {
    const started = Date.now();
    const tick = () => {
      const secs = Math.round((Date.now() - started) / 1000);
      $("#progress-text").textContent = `${label} ${secs}s`;
    };
    tick();
    setBusy.timer = setInterval(tick, 1000);
  }
}

function showError(message) {
  const e = $("#error");
  e.textContent = message;
  e.hidden = !message;
}

async function run() {
  const code = $("#code").value;
  if (!code.trim()) {
    showError("Paste some code first.");
    $("#code").focus();
    return;
  }
  showError("");
  const kind = state.provider.kind;
  const body = {
    code,
    ...readOptions(),
    provider: {
      kind,
      model: providerModel(kind),
      base_url: providerBaseUrl(kind),
      api_key: state.keys[kind] || "",
    },
  };
  const who = kind === "demo" ? "Loading the recorded example." : `Waiting for ${providerModel(kind) || PROVIDER_LABELS[kind]}.`;
  setBusy(true, who);
  state.abort = new AbortController();
  try {
    const result = await api("/improve", { method: "POST", body: JSON.stringify(body), signal: state.abort.signal });
    showResult(result);
  } catch (err) {
    if (err.name === "AbortError") showError("Cancelled.");
    else showError(err.message);
  } finally {
    setBusy(false);
    state.abort = null;
  }
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------
function resolveLanguage(result) {
  if (!window.hljs) return null;
  const tryName = (name) => {
    if (!name) return null;
    const n = LANG_ALIASES[name.toLowerCase()] || name.toLowerCase();
    return hljs.getLanguage(n) ? n : null;
  };
  return tryName(result.language)
    || tryName($("#language").value === "auto" ? "" : $("#language").value)
    || hljs.highlightAuto(result.original_code).language
    || null;
}

// Highlight one line. Multi-line strings and comments may colour imperfectly;
// that's the trade-off for a clean line-by-line diff.
function hl(text) {
  if (!state.hlLang || !window.hljs) return escapeHtml(text);
  const cached = state.hlCache.get(text);
  if (cached !== undefined) return cached;
  let out;
  try { out = hljs.highlight(text, { language: state.hlLang, ignoreIllegals: true }).value; }
  catch { out = escapeHtml(text); }
  state.hlCache.set(text, out);
  return out;
}

function showResult(r) {
  state.result = r;
  state.hlCache = new Map();
  state.hlLang = resolveLanguage(r);
  state.active = null;

  $("#summary").textContent = r.summary || "Here is the improved version.";
  const n = r.changes.length;
  const secs = (r.elapsed_ms / 1000).toFixed(1);
  $("#stats").innerHTML =
    `${n} ${n === 1 ? "change" : "changes"}, <span class="plus">+${r.diff.added}</span> <span class="minus">−${r.diff.removed}</span> lines. ` +
    `${escapeHtml(r.model)}, ${secs}s.`;

  renderCallouts(r);
  renderLessons(r);
  renderDiff();

  $("#compose").hidden = true;
  $("#result").hidden = false;
  window.scrollTo({ top: 0 });
  $("#result-title").focus({ preventScroll: true });
  if (r.changes.length) select(r.changes[0].id, { scroll: false });
}

function renderCallouts(r) {
  const box = $("#callouts");
  box.innerHTML = "";
  if (r.behavior_changes && r.behavior_changes.length) {
    const el = document.createElement("section");
    el.className = "callout warn";
    el.innerHTML = `<h3>Check before you use it</h3>
      <p class="option-help" style="margin:0 0 6px">The new version behaves differently in these ways:</p>
      <ul>${r.behavior_changes.map((b) => `<li>${richText(b)}</li>`).join("")}</ul>`;
    box.appendChild(el);
  }
  if (r.dependencies && r.dependencies.length) {
    const el = document.createElement("section");
    el.className = "callout";
    el.innerHTML = `<h3>New libraries</h3><ul>${r.dependencies.map((d) => `
      <li><span class="dep-name">${escapeHtml(d.name)}</span>: ${richText(d.purpose)}
      ${d.install ? `<br><code class="dep-install">${escapeHtml(d.install)}</code>` : ""}</li>`).join("")}</ul>`;
    box.appendChild(el);
  }
}

function renderLessons(r) {
  const list = $("#lesson-list");
  list.innerHTML = "";
  $("#lessons-title").textContent = r.changes.length ? `Lessons (${r.changes.length})` : "Lessons";
  if (!r.changes.length) {
    list.innerHTML = `<li class="empty-lessons">No changes needed. Your code is already in good shape for these settings.</li>`;
    updateStepper();
    return;
  }
  for (const c of r.changes) {
    const li = document.createElement("li");
    li.className = "lesson";
    li.dataset.id = c.id;
    const cat = c.category in CATEGORY_LABELS ? c.category : "readability";
    const noLoc = !c.before_lines && !c.after_lines
      ? `<p class="lesson-noloc">Couldn't pin this change to exact lines. Look for it in the diff.</p>` : "";
    const concept = c.concept
      ? `<div class="concept"><p class="concept-name">${escapeHtml(c.concept.name)}</p><p>${richText(c.concept.explanation)}</p></div>` : "";
    li.innerHTML = `
      <button class="lesson-btn" type="button" aria-expanded="false" style="--cat: var(--c-${cat})">
        <span class="lesson-num">${c.id}</span>
        <span class="lesson-title">${escapeHtml(c.title || "Change")}</span>
        <span class="lesson-cat">${CATEGORY_LABELS[cat]}</span>
      </button>
      <div class="lesson-body">
        ${c.what ? `<p class="lesson-what">${richText(c.what)}</p>` : ""}
        ${c.why ? `<p class="lesson-why">${richText(c.why)}</p>` : ""}
        ${concept}${noLoc}
      </div>`;
    li.querySelector(".lesson-btn").addEventListener("click", () => {
      // On narrow screens only the active lesson is visible, so never close it.
      if (narrow()) return;
      select(state.active === c.id ? null : c.id);
    });
    list.appendChild(li);
  }
}

function inRange(no, range) {
  return range && no >= range[0] && no <= range[1];
}

// Build the table for the current view. Every code cell carries the line
// numbers it shows, so marking a lesson is just a matter of matching ranges.
function renderDiff() {
  const r = state.result;
  if (!r) return;
  const view = state.view;
  const table = $("#diff");
  const heads = $("#code-heads");
  heads.classList.toggle("single", view !== "split");
  heads.firstElementChild.textContent = view === "after" ? "Improved" : view === "unified" ? "Your code, with changes inline" : "Your code";

  // Which line gets each lesson's badge.
  const badgeAt = new Map(); // "l:12" or "r:5" -> [ids]
  const addBadge = (key, id) => badgeAt.set(key, [...(badgeAt.get(key) || []), id]);
  for (const c of r.changes) {
    // Prefer the new code; fall back to the old lines for pure removals.
    if (c.after_lines) addBadge("r:" + c.after_lines[0], c.id);
    else if (c.before_lines && view !== "after") addBadge("l:" + c.before_lines[0], c.id);
  }
  const badges = (key) => (badgeAt.get(key) || [])
    .map((id, i) => `<button class="badge" type="button" data-id="${id}" style="top:${3 + i * 20}px" aria-label="Lesson ${id}">${id}</button>`)
    .join("");

  const cellPair = (side, line) => {
    if (!line) return `<td class="no filler"></td><td class="txt filler"></td>`;
    const k = line.kind === "equal" ? "" : line.kind;
    const key = side + ":" + line.no;
    return `<td class="no ${k}" data-${side}="${line.no}">${badges(key)}${line.no}</td>` +
      `<td class="txt ${k} ${side === "l" ? "split-left" : ""}" data-${side}="${line.no}">${hl(line.text)}</td>`;
  };

  let html = "";
  if (view === "split") {
    html += `<colgroup><col class="no"><col><col class="no"><col></colgroup>`;
    for (const row of r.diff.rows) html += `<tr>${cellPair("l", row.left)}${cellPair("r", row.right)}</tr>`;
  } else if (view === "after") {
    html += `<colgroup><col class="no"><col></colgroup>`;
    for (const row of r.diff.rows) if (row.right) html += `<tr>${cellPair("r", row.right)}</tr>`;
  } else {
    // Inline view: unchanged lines once, then each block's removals before its additions.
    html += `<colgroup><col class="no"><col></colgroup>`;
    const rows = r.diff.rows;
    for (let i = 0; i < rows.length;) {
      const row = rows[i];
      if (row.left && row.right && row.left.kind === "equal") {
        const key = "r:" + row.right.no;
        html += `<tr><td class="no" data-l="${row.left.no}" data-r="${row.right.no}">${badges(key)}${row.right.no}</td>` +
          `<td class="txt" data-l="${row.left.no}" data-r="${row.right.no}">${hl(row.right.text)}</td></tr>`;
        i++;
        continue;
      }
      const block = [];
      while (i < rows.length && !(rows[i].left && rows[i].right && rows[i].left.kind === "equal")) block.push(rows[i++]);
      for (const b of block) if (b.left) html += `<tr>${cellPair("l", b.left)}</tr>`;
      for (const b of block) if (b.right) html += `<tr>${cellPair("r", b.right)}</tr>`;
    }
  }
  table.innerHTML = html;
  table.querySelectorAll(".badge").forEach((b) =>
    b.addEventListener("click", () => select(Number(b.dataset.id))));
  applyMarks(false);
}

function applyMarks(scroll) {
  const r = state.result;
  $$("#diff .marked").forEach((el) => el.classList.remove("marked"));
  $$("#diff .badge").forEach((b) => b.classList.toggle("active", Number(b.dataset.id) === state.active));
  const c = r && r.changes.find((x) => x.id === state.active);
  if (!c) return;
  let first = null;
  $$("#diff td[data-l], #diff td[data-r]").forEach((td) => {
    // A cell showing new code (including unchanged lines in the inline view,
    // which carry both numbers) is matched against the "after" range.
    // A cell showing only old code is matched against the "before" range.
    const hit = "r" in td.dataset
      ? inRange(Number(td.dataset.r), c.after_lines)
      : inRange(Number(td.dataset.l), c.before_lines);
    if (hit) {
      td.classList.add("marked");
      if (!first) first = td;
    }
  });
  if (scroll && first) first.scrollIntoView({ block: "center", behavior: reduceMotion() ? "auto" : "smooth" });
}

function select(id, { scroll = true } = {}) {
  state.active = id;
  $$(".lesson").forEach((li) => {
    const on = Number(li.dataset.id) === id;
    li.classList.toggle("active", on);
    li.querySelector(".lesson-btn").setAttribute("aria-expanded", String(on));
  });
  // On narrow screens the lesson sits above the code; jumping away would hide it.
  applyMarks(scroll && !narrow());
  updateStepper();
  if (id !== null) {
    const li = $(`.lesson[data-id="${id}"]`);
    if (li && scroll) li.scrollIntoView({ block: "nearest", behavior: reduceMotion() ? "auto" : "smooth" });
  }
}

function step(delta) {
  const ids = state.result ? state.result.changes.map((c) => c.id) : [];
  if (!ids.length) return;
  const at = ids.indexOf(state.active);
  const next = at === -1 ? (delta > 0 ? 0 : ids.length - 1) : Math.min(ids.length - 1, Math.max(0, at + delta));
  select(ids[next]);
}

function updateStepper() {
  const ids = state.result ? state.result.changes.map((c) => c.id) : [];
  const at = ids.indexOf(state.active);
  $("#prev-lesson").disabled = !ids.length || at === 0;
  $("#next-lesson").disabled = !ids.length || at === ids.length - 1;
}

async function copyImproved() {
  if (!state.result) return;
  try {
    await navigator.clipboard.writeText(state.result.improved_code + "\n");
    toast("Copied improved code");
  } catch {
    toast("Couldn't copy. Use the Improved only view and select the text.");
  }
}

function downloadImproved() {
  if (!state.result) return;
  const ext = FILE_EXT[state.hlLang] || "txt";
  const blob = new Blob([state.result.improved_code + "\n"], { type: "text/plain" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `improved.${ext}`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

// ---------------------------------------------------------------------------
// Start up
// ---------------------------------------------------------------------------
async function init() {
  restoreDraft();
  wireEditor();

  $("#allow-libraries").addEventListener("click", () => {
    setLibraries($("#allow-libraries").getAttribute("aria-checked") !== "true");
    saveDraft();
  });
  $$("#focus input, input[name=level]").forEach((i) => i.addEventListener("change", saveDraft));
  $("#target").addEventListener("input", saveDraft);
  $("#language").addEventListener("change", saveDraft);

  $("#run").addEventListener("click", run);
  $("#cancel").addEventListener("click", () => state.abort && state.abort.abort());
  $("#load-example").addEventListener("click", () => {
    if (!state.config) return;
    $("#code").value = state.config.demo_code;
    $("#language").value = "Python";
    saveDraft();
    if (state.provider.kind !== "demo" && !hasWorkingModel()) {
      state.provider.kind = "demo";
      store.set("provider", state.provider);
      renderModelChip();
    }
    toast(state.provider.kind === "demo" ? "Example loaded. Press Improve my code." : "Example loaded");
  });

  $("#edit-again").addEventListener("click", () => {
    $("#result").hidden = true;
    $("#compose").hidden = false;
    $("#code").focus();
  });
  $("#prev-lesson").addEventListener("click", () => step(-1));
  $("#next-lesson").addEventListener("click", () => step(1));
  $$('input[name="view"]').forEach((i) => i.addEventListener("change", () => {
    state.view = i.value;
    renderDiff();
  }));
  $("#copy").addEventListener("click", copyImproved);
  $("#download").addEventListener("click", downloadImproved);

  // j / k step through lessons, like many code review tools.
  document.addEventListener("keydown", (e) => {
    if ($("#result").hidden || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target.closest("input, textarea, select, dialog")) return;
    if (e.key === "j") step(1);
    if (e.key === "k") step(-1);
  });

  $("#open-settings").addEventListener("click", openSettings);
  $$('input[name="provider"]').forEach((i) => i.addEventListener("change", updateDialogFields));
  $("#base-url").addEventListener("change", () => { if (selectedProviderInDialog() === "ollama") updateDialogFields(); });
  $("#settings").addEventListener("close", () => {
    if ($("#settings").returnValue === "save") saveSettings();
  });

  // On narrow screens the side-by-side view is cramped; start inline.
  if (window.matchMedia("(max-width: 760px)").matches) {
    state.view = "unified";
    $$('input[name="view"]').forEach((i) => { i.checked = i.value === "unified"; });
  }

  try {
    state.config = await api("/config");
  } catch (err) {
    showError("Can't reach the RefactorLens server. Is it still running?");
    return;
  }
  await refreshOllamaModels(providerBaseUrl("ollama"));
  chooseDefaultProvider();
  renderModelChip();
}

// Keep a configured real model when the user loads the example; demo is only
// the fallback when nothing real is set up.
function hasWorkingModel() {
  const kind = state.provider.kind;
  if (kind === "anthropic") return keyIsAvailable("anthropic");
  if (kind === "ollama") return state.ollamaReachable !== false && Boolean(providerModel("ollama"));
  if (kind === "openai") return Boolean(providerModel("openai"));
  return false;
}

init();
