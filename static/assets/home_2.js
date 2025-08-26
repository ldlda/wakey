const qs = new URLSearchParams(location.search);
const $ = (id) => document.getElementById(id);
const elName = $("name");
const elCheck = $("check");
const elWake = $("wake");
const elLog = $("log");
const elHtml = $("html");
const elLeases = $("leases_html");
const pill = $("status-pill");
const link = $("permalink");
const elPreview = $("preview");

// One-time delegated click handler for all pickable links
function handlePickClick(e) {
  const a = e.target && /** @type {HTMLElement} */ (e.target).closest("a.pick");
  if (!a) return;
  e.preventDefault();
  const v = a.getAttribute("data-value");
  pickTarget(v);
}
if (elHtml) elHtml.addEventListener("click", handlePickClick);
if (elLeases) elLeases.addEventListener("click", handlePickClick);

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
function extractHostLikeBackend(input) {
  let s = String(input || "").trim();
  if (!s) return "";
  // strip scheme or network-path reference
  const schemeIdx = s.indexOf("://");
  if (schemeIdx >= 0) s = s.slice(schemeIdx + 3);
  else if (s.startsWith("//")) s = s.slice(2);
  // strip userinfo
  const at = s.lastIndexOf("@");
  if (at >= 0) s = s.slice(at + 1);
  // bracketed IPv6
  if (s.startsWith("[")) {
    const end = s.indexOf("]");
    if (end > 1) s = s.slice(1, end);
  } else {
    const slash = s.indexOf("/");
    if (slash >= 0) s = s.slice(0, slash);
    const colon = s.lastIndexOf(":");
    if (colon > 0 && (s.match(/:/g) || []).length === 1) {
      const port = s.slice(colon + 1);
      if (/^\d+$/.test(port)) s = s.slice(0, colon);
    }
  }
  return s.trim();
}
function updatePreview() {
  if (!elPreview) return;
  const raw = getName();
  const host = extractHostLikeBackend(raw);
  elPreview.textContent = host && host !== raw ? `→ ${host}` : "";
}
function pickTarget(value) {
  const v = String(value || "").trim();
  if (!v) return;
  elName.value = v;
  updatePreview();
  saveName(v);
  setLink(v);
  fetchStatus(v);
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
  // If no extra filters, use smart redirect for name/ip/mac/dev/nud detection
  const hasExtraFilters = ["ip", "dev", "nud", "mac"].some(
    (k) => qs.getAll(k).length
  );
  if (name && !hasExtraFilters) {
    return new URL(`/api/smart/${encodeURIComponent(name)}`, location.origin);
  }
  const u = new URL("/api/status", location.origin);
  if (name) u.searchParams.set("name", name);
  for (const k of ["ip", "dev", "nud", "mac"]) {
    const vals = qs.getAll(k);
    for (const v of vals) u.searchParams.append(k, v);
  }
  return u;
}

/** @param {{ name?: String, table: Array<{ ip, dev, mac, state }>, filters: {ip, dev, nud, mac}} | { name?: String, error }} data from /api/status */
function renderStatus(data) {
  const tbl = document.createElement("table");
  tbl.className = "table";
  tbl.innerHTML = `<tr><th>IP</th><th>MAC</th><th>State</th><th>IF</th></tr>`;
  for (const row of data.table || []) {
    const tr = document.createElement("tr");
    const ip = row.ip || "";
    const mac = row.mac || "";
    const dev = row.dev || "";
    const state = row.state || "";
    tr.innerHTML = `
      <td>${
        ip
          ? `<a href="#" class="pick" data-value="${ip}" title="filter by ip">${ip}</a>`
          : ""
      }</td>
      <td>${
        mac
          ? `<a href="#" class="pick" data-value="${mac}" title="filter by mac">${mac}</a>`
          : ""
      }</td>
      <td>${
        state
          ? `<a href="#" class="pick" data-value="${state}" title="filter by state">${state}</a>`
          : ""
      }</td>
      <td>${
        dev
          ? `<a href="#" class="pick" data-value="${dev}" title="filter by interface">${dev}</a>`
          : ""
      }</td>`;
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

  // click-to-filter handled by a single, persistent delegated listener (set once at load)

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

function renderLeases(leases) {
  if (!elLeases) return;
  if (!Array.isArray(leases) || leases.length === 0) {
    elLeases.textContent = "no leases";
    return;
  }
  const tbl = document.createElement("table");
  tbl.className = "table";
  tbl.innerHTML = `<tr><th></th><th>IP</th><th>MAC</th><th>Name</th><th>Expires</th></tr>`;
  const nowSec = Math.floor(Date.now() / 1000);
  for (const l of leases) {
    const tr = document.createElement("tr");
    const exp = Number(l.expires_epoch || 0);
    const expired = exp > 0 && exp <= nowSec;
    const when = exp > 0 ? new Date(exp * 1000) : null;
    const whenText = when ? when.toLocaleString() : "";
    let dotClass = "dot ok";
    if (expired) {
      dotClass = "dot bad";
    } else if (typeof l?.rank === "number") {
      if (l.rank >= 5) dotClass = "dot ok";
      else if (l.rank >= 2) dotClass = "dot warn";
      else dotClass = "dot bad";
    }
    const title = expired
      ? "expired"
      : l?.nud_state
      ? String(l.nud_state).toLowerCase()
      : "unknown";
    const ip = l.ip || "";
    const mac = l.mac || "";
    const name = l.name || "";
    tr.innerHTML = `
      <td><span class="${dotClass}" title="${title}"></span></td>
      <td>${
        ip
          ? `<a href="#" class="pick" data-value="${ip}" title="filter by ip">${ip}</a>`
          : ""
      }</td>
      <td>${
        mac
          ? `<a href="#" class="pick" data-value="${mac}" title="filter by mac">${mac}</a>`
          : ""
      }</td>
      <td>${
        name
          ? `<a href="#" class="pick" data-value="${name}" title="filter by name">${name}</a>`
          : ""
      }</td>
      <td><span class="tiny">${whenText}</span></td>`;
    tbl.appendChild(tr);
  }
  elLeases.innerHTML = "";
  elLeases.appendChild(tbl);
  // click-to-filter handled by a single, persistent delegated listener (set once at load)
}

async function fetchLeases() {
  try {
    const r = await fetch("/api/dhcp_leases?include_state=1");
    if (!r.ok) return;
    const leases = await r.json();
    renderLeases(leases);
  } catch {}
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
elName.addEventListener("input", updatePreview);

// init
const initial = loadName();
if (initial) {
  elName.value = initial;
  updatePreview();
  setLink(initial); // when the permalink doing YOUR job
  // auto-check on load in this A/B page
  fetchStatus(initial);
  fetchLeases();
} else {
  setPill("warn", "unknown");
  fetchLeases();
}
