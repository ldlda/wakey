import { elHtml, elLog, setPill, qs } from "./dom.js";
import { rankState } from "./utils.js";

function buildStatusUrl(name) {
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

export function renderStatus(data) {
  const tbl = document.createElement("table");
  tbl.className = "table";
  tbl.innerHTML = `<tr><th>IP</th><th>MAC</th><th>State</th><th>IF</th></tr>`;

  for (const row of data.table || []) {
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td>${
        row.ip
          ? `<a href="#" class="pick" data-value="${row.ip}" title="filter by ip">${row.ip}</a>`
          : ""
      }</td>
      <td>${
        row.mac
          ? `<a href="#" class="pick" data-value="${row.mac}" title="filter by mac">${row.mac}</a>`
          : ""
      }</td>
      <td>${
        row.state
          ? `<a href="#" class="pick" data-value="${row.state}" title="filter by state">${row.state}</a>`
          : ""
      }</td>
      <td>${
        row.dev
          ? `<a href="#" class="pick" data-value="${row.dev}" title="filter by interface">${row.dev}</a>`
          : ""
      }</td>`;
    tbl.appendChild(tr);
  }

  elHtml.innerHTML = "";
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

export async function fetchStatus(name) {
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
