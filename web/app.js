// ShivaDecompiler frontend - AriaNg-style download manager over the Axum API.
const API = "";
let currentView = "baixando";
let tasks = [];
let filter = "";
let refreshTimer = null;

// ---------- settings ----------
const DEFAULTS = { refreshMs: 1500, defaultConn: 4, lang: "pt", showSpeedInTitle: true, confirmRemove: false, confirmClear: true, gotoDownloadingOnAdd: true };
const settings = { ...DEFAULTS };
function loadSettings() { try { Object.assign(settings, JSON.parse(localStorage.getItem("shiva.settings") || "{}")); } catch {} }
function saveSettings() { try { localStorage.setItem("shiva.settings", JSON.stringify(settings)); } catch {} }

// ---------- i18n ----------
const I18N = {
  pt: {
    nav_tasks: "Tarefas", nav_downloading: "Baixando", nav_waiting: "Aguardando", nav_stopped: "Parados",
    nav_tools: "Ferramentas", nav_terminal: "Terminal", nav_system: "Sistema", nav_status: "Status do engine",
    nav_settings: "Configuracoes", h_new: "Novo download",
    new_hint: "Uma URL por linha. Enderecos .onion vao pelo Tor automaticamente.",
    new_conn: "Conexoes por arquivo", new_out: "Nome de saida (opcional)", btn_download: "Baixar",
    btn_new: "Novo", btn_start: "Iniciar", btn_pause: "Pausar", btn_retry: "Repetir com erro",
    btn_remove: "Remover", btn_clear: "Limpar concluidos", btn_refresh: "Atualizar", empty: "Nenhuma tarefa aqui.",
    h_term: "Terminal do cofre", btn_analyze: "Analisar", h_status: "Status do engine", h_settings: "Configuracoes",
    set_refresh: "Intervalo de atualizacao (ms)", set_conn: "Conexoes padrao por arquivo", btn_save: "Salvar", btn_close: "Fechar",
    set_interface: "Interface", set_lang: "Idioma", set_title: "Mostrar velocidade no titulo da aba",
    set_confirm: "Confirmar antes de remover tarefa", set_goto: "Ir para Baixando ao adicionar",
    set_downloads: "Downloads", set_confirm_clear: "Confirmar ao limpar concluidos", set_engine: "Engine (somente leitura)",
    btn_reset: "Restaurar padroes",
  },
  en: {
    nav_tasks: "Tasks", nav_downloading: "Downloading", nav_waiting: "Waiting", nav_stopped: "Stopped",
    nav_tools: "Tools", nav_terminal: "Terminal", nav_system: "System", nav_status: "Engine status",
    nav_settings: "Settings", h_new: "New download",
    new_hint: "One URL per line. .onion addresses are routed through Tor automatically.",
    new_conn: "Connections per file", new_out: "Output name (optional)", btn_download: "Download",
    btn_new: "New", btn_start: "Start", btn_pause: "Pause", btn_retry: "Retry errored",
    btn_remove: "Remove", btn_clear: "Clear completed", btn_refresh: "Refresh", empty: "No tasks here.",
    h_term: "Vault terminal", btn_analyze: "Analyze", h_status: "Engine status", h_settings: "Settings",
    set_refresh: "Refresh interval (ms)", set_conn: "Default connections per file", btn_save: "Save", btn_close: "Close",
    set_interface: "Interface", set_lang: "Language", set_title: "Show speed in tab title",
    set_confirm: "Confirm before removing a task", set_goto: "Switch to Downloading on add",
    set_downloads: "Downloads", set_confirm_clear: "Confirm before clearing completed", set_engine: "Engine (read-only)",
    btn_reset: "Reset to defaults",
  },
};
function tr(k) { return (I18N[settings.lang] || I18N.pt)[k] || k; }
function applyI18n() {
  const d = I18N[settings.lang] || I18N.pt;
  document.querySelectorAll("[data-i18n]").forEach((el) => { const v = d[el.dataset.i18n]; if (v != null) el.textContent = v; });
  document.documentElement.lang = settings.lang === "en" ? "en" : "pt-BR";
}

// ---------- http + format ----------
async function jget(p) { return (await fetch(API + p)).json(); }
async function jpost(p, b) { return (await fetch(API + p, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(b) })).json(); }
async function jdelete(p) { return fetch(API + p, { method: "DELETE" }); }
function fmtBytes(n) { if (n == null) return "-"; const u = ["B","KB","MB","GB","TB"]; let i=0,v=n; while(v>=1024&&i<u.length-1){v/=1024;i++;} return `${v.toFixed(i?1:0)} ${u[i]}`; }
function fmtSpeed(n) { return `${fmtBytes(n)}/s`; }
function basename(url) { try { return decodeURIComponent(url.split("?")[0].split("/").filter(Boolean).pop() || url); } catch { return url; } }
function esc(s) { return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c])); }

// ---------- navigation ----------
function showView(view) {
  currentView = view;
  document.querySelectorAll(".nav").forEach((n) => n.classList.toggle("is-active", n.dataset.view === view));
  const isTask = ["baixando", "aguardando", "parados"].includes(view);
  const map = { new: "view-new", term: "view-term", disasm: "view-disasm", status: "view-status", settings: "view-settings" };
  Object.entries(map).forEach(([v, id]) => document.getElementById(id).classList.toggle("hidden", v !== view));
  document.getElementById("view-tasks").classList.toggle("hidden", !isTask);
  if (view === "status") renderStatus();
  if (view === "settings") renderSettings();
  if (view === "term") termActivate();
  if (view === "new") document.getElementById("new-conn").value = settings.defaultConn;
  if (isTask) renderTasks();
}
document.querySelectorAll(".nav").forEach((n) => n.addEventListener("click", () => showView(n.dataset.view)));

// ---------- health + stats ----------
async function pollHealth() {
  const el = document.getElementById("health");
  try { const r = await jget("/api/health"); el.innerHTML = `<span class="dot"></span>${r.name} ${r.status}`; }
  catch { el.innerHTML = `<span class="dot" style="background:var(--critical)"></span>offline`; }
}
async function pollStats() {
  try {
    const s = await jget("/api/stats");
    document.getElementById("gspeed-val").textContent = fmtSpeed(s.total_speed_bps);
    document.title = settings.showSpeedInTitle && s.total_speed_bps > 0
      ? `${String.fromCharCode(8595)} ${fmtSpeed(s.total_speed_bps)} - ShivaDecompiler`
      : "ShivaDecompiler";
    if (currentView === "status") renderStatus(s);
  } catch {}
}

// ---------- tasks ----------
const STATE_OF_VIEW = { baixando: ["active"], aguardando: ["queued"], parados: ["complete", "failed"] };
function updateCounts() {
  const by = (s) => tasks.filter((t) => s.includes(t.state)).length;
  document.getElementById("c-baixando").textContent = by(STATE_OF_VIEW.baixando);
  document.getElementById("c-aguardando").textContent = by(STATE_OF_VIEW.aguardando);
  document.getElementById("c-parados").textContent = by(STATE_OF_VIEW.parados);
}
function renderTasks() {
  const list = document.getElementById("tasklist");
  const states = STATE_OF_VIEW[currentView] || [];
  const rows = tasks.filter((task) => states.includes(task.state)).filter((task) => !filter || task.url.toLowerCase().includes(filter));
  document.getElementById("tasks-empty").classList.toggle("hidden", rows.length > 0);
  list.innerHTML = rows.map((task) => {
    const pct = task.bytes_total ? Math.min(100, Math.round((task.bytes_done / task.bytes_total) * 100)) : (task.state === "complete" ? 100 : 0);
    return `<div class="task" data-id="${task.id}">
      <input type="checkbox" class="sel" />
      <div class="task-main">
        <div class="task-name">${task.via_tor ? '<span class="tor">TOR</span>' : ""}${task.connections > 1 ? `<span class="conn">x${task.connections}</span>` : ""}${esc(basename(task.url))}</div>
        <div class="bar"><span style="width:${pct}%"></span></div>
        <div class="task-meta">
          <span>${pct}%</span>
          <span>${fmtBytes(task.bytes_done)}${task.bytes_total ? " / " + fmtBytes(task.bytes_total) : ""}</span>
          ${task.state === "active" ? `<span>${fmtSpeed(task.speed_bps)}</span>` : ""}
          ${task.sha256 ? `<span title="${task.sha256}">sha256 ${task.sha256.slice(0, 12)}...</span>` : ""}
          ${task.error ? `<span style="color:var(--critical)">${esc(task.error)}</span>` : ""}
        </div>
      </div>
      <div class="task-actions">
        <span class="badge ${task.state}">${task.state}</span>
        <button class="btn" data-act="details">Detalhes</button>
        ${task.state === "failed" ? `<button class="btn" data-act="retry">Repetir</button>` : ""}
        <button class="btn btn-danger" data-act="remove">${tr("btn_remove")}</button>
      </div>
    </div>`;
  }).join("");
  list.querySelectorAll(".task").forEach((el) => {
    const task = tasks.find((x) => x.id === el.dataset.id);
    el.querySelectorAll("[data-act]").forEach((b) => b.addEventListener("click", () => rowAction(b.dataset.act, task)));
  });
}
function confirmIf(flag, msg) { return !flag || window.confirm(msg); }

async function rowAction(act, task) {
  if (act === "remove") { if (!confirmIf(settings.confirmRemove, "Remover esta tarefa?")) return; await jdelete(`/api/downloads/${task.id}`); await refresh(); return; }
  if (act === "retry") { await jpost("/api/downloads", { url: task.url, connections: task.connections }); await jdelete(`/api/downloads/${task.id}`); showView("baixando"); await refresh(); return; }
  if (act === "details") openDetail(task);
}
function openDetail(task) {
  const pct = task.bytes_total ? Math.min(100, Math.round((task.bytes_done / task.bytes_total) * 100)) : 0;
  const rows = [["URL", task.url], ["Estado", task.state], ["Via Tor", task.via_tor ? "sim" : "nao"], ["Conexoes", task.connections],
    ["Progresso", `${pct}%`], ["Bytes", `${fmtBytes(task.bytes_done)}${task.bytes_total ? " / " + fmtBytes(task.bytes_total) : ""}`],
    ["Velocidade", fmtSpeed(task.speed_bps)], ["sha256", task.sha256 || "-"], ["Arquivo", task.out_path || "-"], ["Erro", task.error || "-"]];
  const done = task.state === "complete";
  openDrawer("Detalhe da tarefa", `${rows.map(([k, v]) => `<div class="detail-row"><span class="k">${esc(k)}</span><span class="v">${esc(String(v))}</span></div>`).join("")}
    <div class="detail-actions">
      ${done ? `<button class="btn" id="d-detonate">Detonar</button><button class="btn" id="d-analyze">Analisar</button>` : ""}
      ${task.sha256 ? `<button class="btn" id="d-copy">Copiar sha256</button>` : ""}
      <button class="btn btn-danger" id="d-remove">Remover</button>
    </div>`);
  const $ = (id) => document.getElementById(id);
  if (done) {
    $("d-detonate").onclick = async () => { openDrawer("Sandbox - detonacao", `<pre id="det-out">detonando...</pre>`); const r = await jpost("/api/sandbox/detonate", { artifact_sha256: task.sha256, image: "shiva/analysis:latest", timeout_secs: 120, allow_network: false }); document.getElementById("det-out").textContent = JSON.stringify(r, null, 2); };
    $("d-analyze").onclick = () => { closeDrawer(); document.getElementById("da-sha").value = task.sha256; showView("disasm"); };
  }
  if (task.sha256) $("d-copy").onclick = () => navigator.clipboard && navigator.clipboard.writeText(task.sha256);
  $("d-remove").onclick = async () => { if (!confirmIf(settings.confirmRemove, "Remover esta tarefa?")) return; await jdelete(`/api/downloads/${task.id}`); closeDrawer(); await refresh(); };
}

// ---------- toolbar ----------
document.getElementById("t-new").addEventListener("click", () => showView("new"));
document.getElementById("t-refresh").addEventListener("click", refresh);
document.getElementById("t-remove").addEventListener("click", async () => {
  const ids = [...document.querySelectorAll(".task")].filter((el) => el.querySelector(".sel").checked).map((el) => el.dataset.id);
  if (!ids.length) return;
  if (!confirmIf(settings.confirmRemove, `Remover ${ids.length} tarefa(s)?`)) return;
  for (const id of ids) await jdelete(`/api/downloads/${id}`); await refresh();
});
document.getElementById("t-clear").addEventListener("click", async () => {
  const done = tasks.filter((t) => t.state === "complete" || t.state === "failed");
  if (!done.length) return;
  if (!confirmIf(settings.confirmClear, `Limpar ${done.length} concluidos/com erro?`)) return;
  for (const x of done) await jdelete(`/api/downloads/${x.id}`); await refresh();
});
document.getElementById("t-retry").addEventListener("click", async () => { for (const x of tasks.filter((t) => t.state === "failed")) { await jpost("/api/downloads", { url: x.url, connections: x.connections }); await jdelete(`/api/downloads/${x.id}`); } showView("baixando"); await refresh(); });

// ---------- new download ----------
document.getElementById("new-go").addEventListener("click", async () => {
  const urls = document.getElementById("new-urls").value.split("\n").map((s) => s.trim()).filter(Boolean);
  const connections = Math.max(1, Math.min(16, parseInt(document.getElementById("new-conn").value) || 1));
  const out = document.getElementById("new-out").value.trim();
  for (let i = 0; i < urls.length; i++) { const b = { url: urls[i], connections }; if (out && urls.length === 1) b.out_name = out; await jpost("/api/downloads", b); }
  document.getElementById("new-urls").value = ""; document.getElementById("new-out").value = "";
  if (settings.gotoDownloadingOnAdd) showView("baixando");
  await refresh();
});
document.getElementById("new-cancel").addEventListener("click", () => showView("baixando"));

// ---------- drawer ----------
function openDrawer(title, body) { document.getElementById("drawer-title").textContent = title; document.getElementById("drawer-body").innerHTML = body; document.getElementById("drawer").classList.remove("hidden"); }
function closeDrawer() { document.getElementById("drawer").classList.add("hidden"); }
document.getElementById("drawer-close").addEventListener("click", closeDrawer);

// ---------- terminal ----------
let cwd = "/", termBooted = false;
const termOut = document.getElementById("term-out"), termCmd = document.getElementById("term-cmd");
function termPrint(html) { const l = document.createElement("div"); l.innerHTML = html; termOut.appendChild(l); termOut.scrollTop = termOut.scrollHeight; }
function setPrompt() { document.getElementById("term-prompt").textContent = `shiva:${cwd}$`; }
function normalizePath(base, arg) { const parts = ((arg.startsWith("/") ? "" : base) + "/" + arg).split("/").filter(Boolean); const o = []; for (const p of parts) { if (p === ".") continue; if (p === "..") { o.pop(); continue; } o.push(p); } return "/" + o.join("/"); }
async function termRun(raw) {
  const cmd = raw.trim();
  termPrint(`<span class="dir">shiva:${esc(cwd)}$</span> <span class="cmd">${esc(cmd)}</span>`);
  if (!cmd) return;
  const [name, ...rest] = cmd.split(/\s+/); const arg = rest.join(" ");
  if (name === "help") { termPrint("comandos: ls, cd &lt;dir&gt;, pwd, cat &lt;arquivo&gt;, clear, help"); return; }
  if (name === "clear") { termOut.innerHTML = ""; return; }
  if (name === "pwd") { termPrint(esc(cwd)); return; }
  if (name === "ls") { const r = await jget(`/api/fs?path=${encodeURIComponent(arg ? normalizePath(cwd, arg) : cwd)}`); if (r.error) return termPrint(`<span class="err">ls: ${esc(r.error)}</span>`); if (!r.entries.length) return termPrint("(vazio)"); for (const e of r.entries) termPrint(`${e.is_dir ? `<span class="dir">${esc(e.name)}/</span>` : esc(e.name)}${e.is_dir ? "" : "  " + fmtBytes(e.size)}`); return; }
  if (name === "cd") { const r = await jget(`/api/fs?path=${encodeURIComponent(arg ? normalizePath(cwd, arg) : "/")}`); if (r.error) return termPrint(`<span class="err">cd: ${esc(r.error)}</span>`); cwd = r.cwd; setPrompt(); return; }
  if (name === "cat") { if (!arg) return termPrint(`<span class="err">cat: falta o arquivo</span>`); const r = await jget(`/api/fs/cat?path=${encodeURIComponent(normalizePath(cwd, arg))}`); if (r.error) return termPrint(`<span class="err">cat: ${esc(r.error)}</span>`); termPrint(esc(r.text)); return; }
  termPrint(`<span class="err">${esc(name)}: comando desconhecido (use 'help')</span>`);
}
termCmd.addEventListener("keydown", async (e) => { if (e.key === "Enter") { const v = termCmd.value; termCmd.value = ""; await termRun(v); } });
function termActivate() { if (!termBooted) { termBooted = true; termPrint("ShivaDecompiler - terminal do cofre (enjaulado). 'help' para comandos."); setPrompt(); } setTimeout(() => termCmd.focus(), 0); }

// ---------- disasm ----------
document.getElementById("da-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const sha = document.getElementById("da-sha").value.trim();
  const backends = [...document.querySelectorAll("#view-disasm .engines input:checked")].map((i) => i.value);
  const box = document.getElementById("da-results");
  box.innerHTML = `<div class="da-card"><pre>analisando...</pre></div>`;
  const rep = await jpost("/api/disasm", { artifact_sha256: sha, backends });
  if (rep.error) { box.innerHTML = `<div class="da-card"><pre style="color:var(--critical)">${esc(rep.error)}</pre></div>`; return; }
  box.innerHTML = rep.results.map((r) => `<div class="da-card"><h3>${r.backend} (${r.duration_ms}ms)</h3><pre>${r.error ? "aviso: " + esc(r.error) : esc(r.output || "(sem saida)")}</pre></div>`).join("");
});

// ---------- status / settings ----------
async function renderStatus(s) {
  if (!s) { try { s = await jget("/api/stats"); } catch { s = {}; } }
  document.getElementById("status-cards").innerHTML = `
    <div class="card"><div class="k">Baixando / Aguardando</div><div class="v">${s.active || 0} / ${s.waiting || 0}</div></div>
    <div class="card"><div class="k">Concluidos / Falhas</div><div class="v">${s.complete || 0} / ${s.failed || 0}</div></div>
    <div class="card"><div class="k">Velocidade total</div><div class="v">${fmtSpeed(s.total_speed_bps || 0)}</div></div>
    <div class="card"><div class="k">Total baixado</div><div class="v">${fmtBytes(s.total_bytes || 0)}</div></div>
    <div class="card"><div class="k">Tor (SOCKS)</div><div class="v">socks5h://127.0.0.1:9050</div></div>
    <div class="card"><div class="k">Sandbox</div><div class="v">Docker remoto (SHIVA_DOCKER_HOST), isolada, sem rede por padrao.</div></div>`;
}
function renderSettings() {
  document.getElementById("set-lang").value = settings.lang;
  document.getElementById("set-refresh").value = settings.refreshMs;
  document.getElementById("set-conn").value = settings.defaultConn;
  document.getElementById("set-title").checked = settings.showSpeedInTitle;
  document.getElementById("set-confirm").checked = settings.confirmRemove;
  document.getElementById("set-goto").checked = settings.gotoDownloadingOnAdd;
  document.getElementById("set-confirm-clear").checked = settings.confirmClear;
  document.getElementById("settings-cards").innerHTML = `
    <div class="card"><div class="k">API</div><div class="v">http://127.0.0.1:8777</div></div>
    <div class="card"><div class="k">Cofre de amostras</div><div class="v">SHIVA_DOWNLOAD_DIR (./downloads)</div></div>
    <div class="card"><div class="k">Tor</div><div class="v">SHIVA_TOR_PROXY (socks5h://127.0.0.1:9050)</div></div>
    <div class="card"><div class="k">Sandbox Docker</div><div class="v">SHIVA_DOCKER_HOST</div></div>`;
}
document.getElementById("set-save").addEventListener("click", () => {
  settings.lang = document.getElementById("set-lang").value === "en" ? "en" : "pt";
  settings.refreshMs = Math.max(500, Math.min(10000, parseInt(document.getElementById("set-refresh").value) || 1500));
  settings.defaultConn = Math.max(1, Math.min(16, parseInt(document.getElementById("set-conn").value) || 4));
  settings.showSpeedInTitle = document.getElementById("set-title").checked;
  settings.confirmRemove = document.getElementById("set-confirm").checked;
  settings.gotoDownloadingOnAdd = document.getElementById("set-goto").checked;
  settings.confirmClear = document.getElementById("set-confirm-clear").checked;
  saveSettings();
  document.getElementById("lang").value = settings.lang;
  applyI18n(); startRefresh(); renderSettings();
});
document.getElementById("set-reset").addEventListener("click", () => {
  Object.assign(settings, DEFAULTS); saveSettings();
  document.getElementById("lang").value = settings.lang;
  applyI18n(); startRefresh(); renderSettings();
});
document.getElementById("lang").addEventListener("change", (e) => { settings.lang = e.target.value; saveSettings(); applyI18n(); if (["baixando","aguardando","parados"].includes(currentView)) renderTasks(); });
document.getElementById("search").addEventListener("input", (e) => { filter = e.target.value.toLowerCase(); if (["baixando","aguardando","parados"].includes(currentView)) renderTasks(); });

// ---------- loop ----------
async function refresh() {
  try { tasks = await jget("/api/downloads"); } catch { return; }
  updateCounts();
  if (["baixando", "aguardando", "parados"].includes(currentView)) renderTasks();
}
function startRefresh() { if (refreshTimer) clearInterval(refreshTimer); refreshTimer = setInterval(() => { refresh(); pollStats(); }, settings.refreshMs); }

// ---------- boot ----------
loadSettings();
document.getElementById("lang").value = settings.lang;
applyI18n();
pollHealth(); refresh(); pollStats(); showView("baixando"); startRefresh();
setInterval(pollHealth, 5000);
