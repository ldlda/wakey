const qs = new URLSearchParams(location.search);
const $ = (id) => document.getElementById(id);
const elName = $("name");
const elCheck = $("check");
const elWake = $("wake");
const elLog = $("log");
const elHtml = $("html");
const pill = $("status-pill");
const link = $("permalink");

function setPill(kind, text) {
  pill.className = `pill ${kind}`;
  pill.textContent = text;
}
function setLink(name) {
  const url = new URL(location.href);
  if (name) url.searchParams.set("name", name);
  else url.searchParams.delete("name");
  history.replaceState(null, "", url);
  link.href = url.toString();
}
function getName() {
  return (elName.value || "").trim();
}
function saveName(name) {
  try {
    localStorage.setItem("wakey:name", name);
  } catch {}
}
function loadName() {
  return qs.get("name") || localStorage.getItem("wakey:name") || "";
}
/** @param {String} s NUD state */
function rankState(s) {
  const key = String(s || "")
    .trim()
    .toUpperCase();
  return (
    {
      PERMANENT: 5,
      REACHABLE: 5,
      STALE: 4,
      DELAY: 3,
      PROBE: 3,
      INCOMPLETE: 3,
      NOARP: 2,
      NONE: 1,
      FAILED: 0,
    }[key] ?? 0
  );
}

function buildStatusUrl(name) {
  const u = new URL("/api/status", location.origin);
  if (name) u.searchParams.set("name", name);
  // forward multi-value filters from current page URL
  for (const k of ["ip", "dev", "nud", "mac"]) {
    const vals = qs.getAll(k);
    for (const v of vals) u.searchParams.append(k, v);
  }
  return u;
}

/** @param {{ name?: String, table: Array<{ ip, dev, mac, state }>, filters: {ip, dev, nud, mac}} | { name?: String, error }} data from /api/status */
function renderStatus(data) {
  const tbl = document.createElement("table");
  tbl.innerHTML = `<tr><th>IP</th><th>MAC</th><th>State</th><th>IF</th></tr>`;
  for (const row of data.table || []) {
    const tr = document.createElement("tr");
    const mac = row.mac ?? "";
    const dev = row.dev ?? "";
    tr.innerHTML = `<td>${row.ip}</td><td>${mac}</td><td>${row.state}</td><td>${dev}</td>`;
    tbl.appendChild(tr);
  }
  elHtml.innerHTML = "";
  // Optional: show applied filters
  if (data.filters) {
    const parts = [];
    if (Array.isArray(data.filters.ip) && data.filters.ip.length)
      parts.push(`ip=[${data.filters.ip.join(", ")}]`);
    if (Array.isArray(data.filters.dev) && data.filters.dev.length)
      parts.push(`dev=[${data.filters.dev.join(", ")}]`);
    if (Array.isArray(data.filters.nud) && data.filters.nud.length)
      parts.push(`nud=[${data.filters.nud.join(", ")}]`);
    if (Array.isArray(data.filters.mac) && data.filters.mac.length)
      parts.push(`mac=[${data.filters.mac.join(", ")}]`);
    if (parts.length) {
      const info = document.createElement("div");
      info.className = "filters";
      info.textContent = `Filters: ${parts.join("; ")}`;
      elHtml.appendChild(info);
    }
  }
  elHtml.appendChild(tbl);

  if ((data.table || []).length > 0) {
    const best = data.table.reduce((a, b) =>
      rankState(b.state) > rankState(a.state) ? b : a
    );
    const r = rankState(best.state);
    if (r >= 5) setPill("ok", "online");
    else if (r >= 2) setPill("warn", "maybe");
    else setPill("bad", "offline");
  } else {
    setPill("warn", "unknown");
  }
}

async function fetchStatus(name) {
  setPill("warn", "checking…");
  const u = buildStatusUrl(name);
  elLog.textContent = "GET " + u.pathname + u.search;
  elHtml.innerHTML = "";
  try {
    const r = await fetch(u);
    if (!r.ok) {
      let msg = String(r.status);
      try {
        const err = await r.clone().json();
        msg = err.error || JSON.stringify(err);
      } catch {
        msg = await r.text();
      }
      elLog.textContent = `status error: ${msg}`;
      //   elHtml.textContent = msg;
      setPill("bad", "error");
      return;
    }
    const data = await r.json();
    renderStatus(data);
  } catch (e) {
    elLog.textContent = "status error: " + e;
    setPill("bad", "error");
  }
}

async function sendWake(name) {
  setPill("warn", "waking…");
  elLog.textContent = "POST /wake?name=" + name;
  try {
    const r = await fetch(`/wake?name=${encodeURIComponent(name)}`, {
      method: "POST",
      // headers: { "X-Target-Name": name },
    });
    const t = await r.text();
    elLog.textContent = t || "ok";
    // Optional: recheck after medium delay
    setTimeout(() => fetchStatus(name), 1500);
  } catch (e) {
    elLog.textContent = "wake error: " + e;
    setPill("bad", "error");
  }
}

elCheck.addEventListener("click", () => {
  const name = getName();
  if (!name) return;
  saveName(name);
  setLink(name);
  fetchStatus(name);
});
elWake.addEventListener("click", () => {
  const name = getName();
  if (!name) return;
  saveName(name);
  setLink(name);
  sendWake(name);
});
elName.addEventListener("keydown", (e) => {
  if (e.key === "Enter") elCheck.click();
});

// init
const initial = loadName();
if (initial) {
  elName.value = initial;
  setLink(initial); // when the permalink doing YOUR job
  // auto-check on load in this A/B page
  fetchStatus(initial);
} else {
  setPill("warn", "unknown");
}
