// ShivaDecompiler frontend — AriaNg-style download manager over the Axum API.
const API = "";
let currentView = "baixando";
let tasks = [];
let filter = "";
let refreshTimer = null;

// ---------- settings (persisted) ----------
const settings = {
  refreshMs: 1500,
  defaultConn: 4,
  lang: "pt",
};
function loadSettings() {
  try {
    const s = JSON.parse(localStorage.getItem("shiva.settings") || "{}");
    Object.assign(settings, s);
  } catch {}
}
function saveSettings() {
  try { localStorage.setItem("shiva.settings", JSON.stringify(settings)); } catch {}
}

// ---------- i18n ----------
const I18N = {
  pt: {
    nav_tasks: "Tarefas", nav_new: "Novo download", nav_downloading: "Baixando",
    nav_waiting: "Aguardando", nav_stopped: "Parados", nav_tools: "Ferramentas",
    nav_terminal: "Terminal", nav_system: "Sistema", nav_status: "Status do engine",
    nav_settings: "Configuracoes", h_new: "Novo download",
    new_hint: "Uma URL por linha. Enderecos .onion vao pelo Tor automaticamente.",
    new_conn: "Conexoes por arquivo", new_out: "Nome de saida (opcional)",
    btn_download: "Baixar", btn_new: "Novo", btn_start: "Iniciar", btn_pause: "Pausar",
    btn_retry: "Repetir com erro", btn_remove: "Remover", btn_clear: "Limpar concluidos",
    btn_refresh: "Atualizar", empty: "Nenhuma tarefa aqui.", h_term: "Terminal do cofre",
    btn_analyze: "Analisar", h_status: "Status do engine", h_settings: "Configuracoes",
    set_refresh: "Intervalo de atualizacao (ms)", set_conn: "Conexoes padrao",
    btn_save: "Salvar", btn_close: "Fechar",
  },
  en: {
    nav_tasks: "Tasks", nav_new: "New download", nav_downloading: "Downloading",
    nav_waiting: "Waiting", nav_stopped: "Stopped", nav_tools: "Tools",
    nav_terminal: "Terminal", nav_system: "System", nav_status: "Engine status",
    nav_settings: "Settings", h_new: "New download",
    new_hint: "One URL per line. .onion addresses are routed through Tor automatically.",
    new_conn: "Connections per file", new_out: "Output name (optional)",
    btn_download: "Download", btn_new: "New", btn_start: "Start", btn_pause: "Pause",
    btn_retry: "Retry errored", btn_remove: "Remove", btn_clear: "Clear completed",
    btn_refresh: "Refresh", empty: "No tasks here.", h_term: "Vault terminal",
    btn_analyze: "Analyze", h_status: "Engine status", h_settings: "Settings",
    set_refresh: "Refresh interval (ms)", set_conn: "Default connections",
    btn_save: "Save", btn_close: "Close",
  },
};
function applyI18n() {
  const d = I18N[settings.lang] || I18N.pt;
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const v = d[el.dataset.i18n];
    if (v != null) el.textContent = v;
  });
  document.documentElement.lang = settings.lang === "en" ? "en" : "pt-BR";
}

// ---------- http ----------
async function jget(p) { return (await fetch(API + p)).json(); }
async function jpost(p, body) {
  const r = await fetch(API + p, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return r.json();
}
async function jdelete(p) { return fetch(API + p, { method: "DELETE" }); }

// ---------- format ----------
function fmtBytes(n) {
  if (n == null) return "-";
  const u = ["B", "KB", "MB", "GB", "TB"]; let i = 0; let v = n;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(i ? 1 : 0)} ${u[i]}`;
}
function fmtSpeed(n) { return `${fmtBytes(n)}/s`; }
function basename(url) {
  try { return decodeURIComponent(url.split("?")[0].split("/").filter(Boolean).pop() || url); }
  catch { return url; }
}

// ---------- navigation ----------
const VIEWS = ["new", "baixando", "aguardando", "parados", "term", "disasm", "status", "settings"];
function showView(view) {
  currentView = view;
  document.querySelectorAll(".nav").forEach((n) => n.classList.toggle("is-active", n.dataset.view === view));
  const isTask = ["baixando", "aguardando", "parados"].includes(view);
  document.getElementById("view-new").classList.toggle("hidden", view !== "new");
  document.getElementById("view-tasks").classList.toggle("hidden", !isTask);
  document.getElementById("view-term").classList.toggle("hidden", view !== "term");
  document.getElementById("view-disasm").classList.toggle("hidden", view !== "disasm");
  document.getElementById("view-status").classList.toggle("hidden", view !== "status");
  document.getElementById("view-settings").classList.toggle("hidden", view !== "settings");
  if (view === "status") renderStatus();
  if (view === "settings") renderSettings();
  if (view === "term") termActivate();
  if (view === "new") document.getElementById("new-conn").value = settings.defaultConn;
  if (isTask) renderTasks();
}
document.querySelectorAll(".nav").forEach((n) => n.addEventListener("click", () => showView(n.dataset.view)));

// ---------- health + global speed ----------
async function pollHealth() {
  try {
    const r = await jget("/api/health");
    document.getElementById("health").textContent = `${r.name} ${r.status}`;
  } catch { document.getElementById("health").textContent = "offline"; }
}
async function pollStats() {
  try {
    const s = await jget("/api/stats");
    document.getElementById("gspeed").textContent = fmtSpeed(s.total_speed_bps);
    if (currentView === "status") renderStatus(s);
  } catch {}
}

// ---------- task views ----------
const STATE_OF_VIEW = { baixando: ["active"], aguardando: ["queued"], parados: ["complete", "failed"] };
function updateCounts() {
  const by = (sts) => tasks.filter((t) => sts.includes(t.state)).length;
  document.getElementById("c-baixando").textContent = by(STATE_OF_VIEW.baixando);
  document.getElementById("c-aguardando").textContent = by(STATE_OF_VIEW.aguardando);
  document.getElementById("c-parados").textContent = by(STATE_OF_VIEW.parados);
}
function renderTasks() {
  const list = document.getElementById("tasklist");
  const states = STATE_OF_VIEW[currentView] || [];
  const rows = tasks.filter((t) => states.includes(t.state)).filter((t) => !filter || t.url.toLowerCase().includes(filter));
  document.getElementById("tasks-empty").classList.toggle("hidden", rows.length > 0);

  list.innerHTML = rows.map((t) => {
    const pct = t.bytes_total ? Math.min(100, Math.round((t.bytes_done / t.bytes_total) * 100)) : (t.state === "complete" ? 100 : 0);
    return `<div class="task" data-id="${t.id}">
      <input type="checkbox" class="sel" />
      <div class="task-main">
        <div class="task-name">
          ${t.via_tor ? '<span class="tor" title="via Tor">TOR</span>' : ""}
          ${t.connections > 1 ? `<span class="conn" title="conexoes">x${t.connections}</span>` : ""}
          ${basename(t.url)}
        </div>
        <div class="bar"><span style="width:${pct}%"></span></div>
        <div class="task-meta">
          <span>${pct}%</span>
          <span>${fmtBytes(t.bytes_done)}${t.bytes_total ? " / " + fmtBytes(t.bytes_total) : ""}</span>
          ${t.state === "active" ? `<span>${fmtSpeed(t.speed_bps)}</span>` : ""}
          ${t.sha256 ? `<span title="${t.sha256}">sha256 ${t.sha256.slice(0, 12)}...</span>` : ""}
          ${t.error ? `<span style="color:var(--danger)">${t.error}</span>` : ""}
        </div>
      </div>
      <div class="task-actions">
        <span class="badge ${t.state}">${t.state}</span>
        <button class="btn" data-act="details">Detalhes</button>
        ${t.state === "failed" ? `<button class="btn" data-act="retry">Repetir</button>` : ""}
        <button class="btn btn-danger" data-act="remove">${I18N[settings.lang].btn_remove}</button>
      </div>
    </div>`;
  }).join("");

  list.querySelectorAll(".task").forEach((el) => {
    const t = tasks.find((x) => x.id === el.dataset.id);
    el.querySelectorAll("[data-act]").forEach((b) => b.addEventListener("click", () => rowAction(b.dataset.act, t)));
  });
}

async function rowAction(act, t) {
  if (act === "remove") { await jdelete(`/api/downloads/${t.id}`); await refresh(); return; }
  if (act === "retry") {
    await jpost("/api/downloads", { url: t.url, connections: t.connections });
    await jdelete(`/api/downloads/${t.id}`);
    showView("baixando"); await refresh(); return;
  }
  if (act === "details") openDetail(t);
}

function openDetail(t) {
  const pct = t.bytes_total ? Math.min(100, Math.round((t.bytes_done / t.bytes_total) * 100)) : 0;
  const rows = [
    ["URL", t.url], ["Estado", t.state], ["Via Tor", t.via_tor ? "sim" : "nao"],
    ["Conexoes", t.connections], ["Progresso", `${pct}%`],
    ["Bytes", `${fmtBytes(t.bytes_done)}${t.bytes_total ? " / " + fmtBytes(t.bytes_total) : ""}`],
    ["Velocidade", fmtSpeed(t.speed_bps)], ["sha256", t.sha256 || "-"],
    ["Arquivo", t.out_path || "-"], ["Erro", t.error || "-"],
  ];
  const done = t.state === "complete";
  const body = `
    ${rows.map(([k, v]) => `<div class="detail-row"><span class="k">${k}</span><span class="v">${String(v)}</span></div>`).join("")}
    <div class="detail-actions">
      ${done ? `<button class="btn" id="d-detonate">Detonar (sandbox)</button>` : ""}
      ${done ? `<button class="btn" id="d-analyze">Analisar (disasm)</button>` : ""}
      ${t.sha256 ? `<button class="btn" id="d-copy">Copiar sha256</button>` : ""}
      <button class="btn btn-danger" id="d-remove">${I18N[settings.lang].btn_remove}</button>
    </div>`;
  openDrawer("Detalhe da tarefa", body);
  const $ = (id) => document.getElementById(id);
  if (done) {
    $("d-detonate").onclick = async () => {
      openDrawer("Sandbox - detonacao", `<pre id="det-out">detonando...</pre>`);
      const rep = await jpost("/api/sandbox/detonate", { artifact_sha256: t.sha256, image: "shiva/analysis:latest", timeout_secs: 120, allow_network: false });
      document.getElementById("det-out").textContent = JSON.stringify(rep, null, 2);
    };
    $("d-analyze").onclick = () => { closeDrawer(); document.getElementById("da-sha").value = t.sha256; showView("disasm"); };
  }
  if (t.sha256) $("d-copy").onclick = () => navigator.clipboard && navigator.clipboard.writeText(t.sha256);
  $("d-remove").onclick = async () => { await jdelete(`/api/downloads/${t.id}`); closeDrawer(); await refresh(); };
}

// ---------- toolbar ----------
document.getElementById("t-new").addEventListener("click", () => showView("new"));
document.getElementById("t-refresh").addEventListener("click", refresh);
document.getElementById("t-remove").addEventListener("click", async () => {
  const ids = [...document.querySelectorAll(".task")].filter((el) => el.querySelector(".sel").checked).map((el) => el.dataset.id);
  for (const id of ids) await jdelete(`/api/downloads/${id}`);
  await refresh();
});
document.getElementById("t-clear").addEventListener("click", async () => {
  for (const t of tasks.filter((t) => t.state === "complete" || t.state === "failed")) await jdelete(`/api/downloads/${t.id}`);
  await refresh();
});
document.getElementById("t-retry").addEventListener("click", async () => {
  for (const t of tasks.filter((t) => t.state === "failed")) {
    await jpost("/api/downloads", { url: t.url, connections: t.connections });
    await jdelete(`/api/downloads/${t.id}`);
  }
  showView("baixando"); await refresh();
});

// ---------- new download ----------
document.getElementById("new-go").addEventListener("click", async () => {
  const urls = document.getElementById("new-urls").value.split("\n").map((s) => s.trim()).filter(Boolean);
  const connections = Math.max(1, Math.min(16, parseInt(document.getElementById("new-conn").value) || 1));
  const out = document.getElementById("new-out").value.trim();
  for (let i = 0; i < urls.length; i++) {
    const body = { url: urls[i], connections };
    if (out && urls.length === 1) body.out_name = out;
    await jpost("/api/downloads", body);
  }
  document.getElementById("new-urls").value = "";
  document.getElementById("new-out").value = "";
  showView("baixando"); await refresh();
});

// ---------- drawer ----------
function openDrawer(title, bodyHtml) {
  document.getElementById("drawer-title").textContent = title;
  document.getElementById("drawer-body").innerHTML = bodyHtml;
  document.getElementById("drawer").classList.remove("hidden");
}
function closeDrawer() { document.getElementById("drawer").classList.add("hidden"); }
document.getElementById("drawer-close").addEventListener("click", closeDrawer);

// ---------- terminal ----------
let cwd = "/", termBooted = false;
const termOut = document.getElementById("term-out");
const termCmd = document.getElementById("term-cmd");
function termPrint(html) { const l = document.createElement("div"); l.innerHTML = html; termOut.appendChild(l); termOut.scrollTop = termOut.scrollHeight; }
function esc(s) { return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c])); }
function setPrompt() { document.getElementById("term-prompt").textContent = `shiva:${cwd}$`; }
function normalizePath(base, arg) {
  const parts = ((arg.startsWith("/") ? "" : base) + "/" + arg).split("/").filter(Boolean);
  const out = [];
  for (const p of parts) { if (p === ".") continue; if (p === "..") { out.pop(); continue; } out.push(p); }
  return "/" + out.join("/");
}
async function termRun(raw) {
  const cmd = raw.trim();
  termPrint(`<span class="dir">shiva:${esc(cwd)}$</span> <span class="cmd">${esc(cmd)}</span>`);
  if (!cmd) return;
  const [name, ...rest] = cmd.split(/\s+/);
  const arg = rest.join(" ");
  if (name === "help") { termPrint("comandos: ls, cd &lt;dir&gt;, pwd, cat &lt;arquivo&gt;, clear, help"); return; }
  if (name === "clear") { termOut.innerHTML = ""; return; }
  if (name === "pwd") { termPrint(esc(cwd)); return; }
  if (name === "ls") {
    const r = await jget(`/api/fs?path=${encodeURIComponent(arg ? normalizePath(cwd, arg) : cwd)}`);
    if (r.error) { termPrint(`<span class="err">ls: ${esc(r.error)}</span>`); return; }
    if (!r.entries.length) { termPrint("(vazio)"); return; }
    for (const e of r.entries) termPrint(`${e.is_dir ? `<span class="dir">${esc(e.name)}/</span>` : esc(e.name)}${e.is_dir ? "" : "  " + fmtBytes(e.size)}`);
    return;
  }
  if (name === "cd") {
    const r = await jget(`/api/fs?path=${encodeURIComponent(arg ? normalizePath(cwd, arg) : "/")}`);
    if (r.error) { termPrint(`<span class="err">cd: ${esc(r.error)}</span>`); return; }
    cwd = r.cwd; setPrompt(); return;
  }
  if (name === "cat") {
    if (!arg) { termPrint(`<span class="err">cat: falta o arquivo</span>`); return; }
    const r = await jget(`/api/fs/cat?path=${encodeURIComponent(normalizePath(cwd, arg))}`);
    if (r.error) { termPrint(`<span class="err">cat: ${esc(r.error)}</span>`); return; }
    termPrint(esc(r.text)); return;
  }
  termPrint(`<span class="err">${esc(name)}: comando desconhecido (use 'help')</span>`);
}
termCmd.addEventListener("keydown", async (e) => { if (e.key === "Enter") { const v = termCmd.value; termCmd.value = ""; await termRun(v); } });
function termActivate() {
  if (!termBooted) { termBooted = true; termPrint("ShivaDecompiler - terminal do cofre (enjaulado). 'help' para comandos."); setPrompt(); }
  setTimeout(() => termCmd.focus(), 0);
}

// ---------- disasm ----------
document.getElementById("da-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const sha = document.getElementById("da-sha").value.trim();
  const backends = [...document.querySelectorAll("#view-disasm .engines input:checked")].map((i) => i.value);
  const rep = await jpost("/api/disasm", { artifact_sha256: sha, backends });
  document.getElementById("da-results").innerHTML = rep.results.map((r) =>
    `<div class="da-card"><h3>${r.backend}</h3><pre>${r.error ? "aviso: " + r.error : (r.output || "(sem saida)")}</pre></div>`).join("");
});

// ---------- status ----------
async function renderStatus(s) {
  if (!s) { try { s = await jget("/api/stats"); } catch { s = {}; } }
  document.getElementById("status-cards").innerHTML = `
    <div class="card"><div class="k">Baixando / Aguardando</div><div class="v">${s.active || 0} / ${s.waiting || 0}</div></div>
    <div class="card"><div class="k">Concluidos / Falhas</div><div class="v">${s.complete || 0} / ${s.failed || 0}</div></div>
    <div class="card"><div class="k">Velocidade total</div><div class="v">${fmtSpeed(s.total_speed_bps || 0)}</div></div>
    <div class="card"><div class="k">Total baixado</div><div class="v">${fmtBytes(s.total_bytes || 0)}</div></div>
    <div class="card"><div class="k">Tor (SOCKS)</div><div class="v">socks5h://127.0.0.1:9050</div></div>
    <div class="card"><div class="k">Sandbox</div><div class="v">Docker remoto (SHIVA_DOCKER_HOST). Isolada, sem rede por padrao.</div></div>`;
}

// ---------- settings ----------
function renderSettings() {
  document.getElementById("set-refresh").value = settings.refreshMs;
  document.getElementById("set-conn").value = settings.defaultConn;
  document.getElementById("settings-cards").innerHTML = `
    <div class="card"><div class="k">API</div><div class="v">http://127.0.0.1:8777</div></div>
    <div class="card"><div class="k">Pasta de amostras</div><div class="v">./downloads (SHIVA_DOWNLOAD_DIR)</div></div>
    <div class="card"><div class="k">Tor</div><div class="v">socks5h://127.0.0.1:9050 (SHIVA_TOR_PROXY)</div></div>`;
}
document.getElementById("set-save").addEventListener("click", () => {
  settings.refreshMs = Math.max(500, Math.min(10000, parseInt(document.getElementById("set-refresh").value) || 1500));
  settings.defaultConn = Math.max(1, Math.min(16, parseInt(document.getElementById("set-conn").value) || 4));
  saveSettings();
  startRefresh();
});
document.getElementById("lang").addEventListener("change", (e) => { settings.lang = e.target.value; saveSettings(); applyI18n(); renderTasks(); });

// ---------- search ----------
document.getElementById("search").addEventListener("input", (e) => { filter = e.target.value.toLowerCase(); renderTasks(); });

// ---------- loop ----------
async function refresh() {
  try { tasks = await jget("/api/downloads"); } catch { return; }
  updateCounts();
  if (["baixando", "aguardando", "parados"].includes(currentView)) renderTasks();
}
function startRefresh() {
  if (refreshTimer) clearInterval(refreshTimer);
  refreshTimer = setInterval(() => { refresh(); pollStats(); }, settings.refreshMs);
}

// ---------- boot ----------
loadSettings();
document.getElementById("lang").value = settings.lang;
applyI18n();
pollHealth();
refresh();
pollStats();
showView("baixando");
startRefresh();
setInterval(pollHealth, 5000);
