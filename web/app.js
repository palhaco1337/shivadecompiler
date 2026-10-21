// ShivaDecompiler frontend — talks to the Axum JSON API.
const API = "";

async function jpost(path, body) {
  const r = await fetch(API + path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return r.json();
}

// --- tab switching ---
document.querySelectorAll(".tab").forEach((t) => {
  t.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((x) => x.classList.remove("is-active"));
    t.classList.add("is-active");
    const view = t.dataset.view;
    document.getElementById("view-downloads").classList.toggle("hidden", view !== "downloads");
    document.getElementById("view-sandbox").classList.toggle("hidden", view !== "sandbox");
  });
});

// --- health ---
async function pollHealth() {
  const el = document.getElementById("health");
  try {
    const r = await (await fetch(API + "/api/health")).json();
    el.textContent = `● ${r.name} ${r.status}`;
  } catch {
    el.textContent = "● offline";
  }
}

// --- downloads ---
const dlForm = document.getElementById("dl-form");
dlForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  const url = document.getElementById("dl-url").value.trim();
  if (!url) return;
  await jpost("/api/downloads", { url });
  document.getElementById("dl-url").value = "";
  refreshDownloads();
});

function shortHash(h) {
  return h ? h.slice(0, 12) + "…" : "—";
}

async function refreshDownloads() {
  const tbody = document.querySelector("#dl-table tbody");
  let tasks = [];
  try {
    tasks = await (await fetch(API + "/api/downloads")).json();
  } catch {
    return;
  }
  tbody.innerHTML = tasks
    .map((t) => {
      const pct = t.bytes_total ? Math.round((t.bytes_done / t.bytes_total) * 100) + "%" : t.bytes_done + " B";
      return `<tr>
        <td title="${t.url}">${t.url.slice(0, 40)}</td>
        <td>${t.via_tor ? "🧅" : "—"}</td>
        <td><span class="badge ${t.state}">${t.state}</span></td>
        <td>${pct}</td>
        <td>${shortHash(t.sha256)}</td>
      </tr>`;
    })
    .join("");
}

// --- sandbox ---
document.getElementById("det-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const sha = document.getElementById("det-sha").value.trim();
  const out = document.getElementById("det-out");
  out.textContent = "detonando…";
  const report = await jpost("/api/sandbox/detonate", {
    artifact_sha256: sha,
    image: "shiva/analysis:latest",
    timeout_secs: 120,
    allow_network: false,
  });
  out.textContent = JSON.stringify(report, null, 2);
});

// --- disassembly packages ---
document.getElementById("da-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const sha = document.getElementById("da-sha").value.trim();
  const backends = [...document.querySelectorAll("#da-form .engines input:checked")].map((i) => i.value);
  const report = await jpost("/api/disasm", { artifact_sha256: sha, backends });
  const box = document.getElementById("da-results");
  box.innerHTML = report.results
    .map(
      (r) => `<div class="da-card">
        <h3>${r.backend}</h3>
        <pre>${r.error ? "⚠ " + r.error : r.output || "(sem saída)"}</pre>
      </div>`
    )
    .join("");
});

// boot
pollHealth();
refreshDownloads();
setInterval(refreshDownloads, 2000);
setInterval(pollHealth, 5000);
