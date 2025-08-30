import { elHtml, elLog, setPill, qs, pill } from "./dom.js";
import { rankState } from "./utils.js";
import { merge_wake_data, translate_wake_message } from "./wake.js";

const status_map = {
  ip: "ip",
  mac: "mac",
  state: "state",
  dev: "interface",
};

const filter_array = ["ip", "dev", "nud", "mac"];

function buildStatusUrl(name) {
  const hasExtraFilters = filter_array.some((k) => qs.getAll(k).length);
  if (name && !hasExtraFilters) {
    return new URL(`/api/smart/${encodeURIComponent(name)}`, location.origin);
  }
  const u = new URL("/api/status", location.origin);
  if (name) u.searchParams.set("name", name);
  for (const k of Object.keys(status_map)) {
    const vals = qs.getAll(k);
    for (const v of vals) u.searchParams.append(k, v);
  }
  return u;
}

export function renderStatus(data) {
  const tbl = document.createElement("table");
  tbl.className = "table";
  tbl.innerHTML = `<thead><tr>${
    data.has_wake ? "<th>Wake status</th>" : ""
  }<th>IP</th><th>MAC</th><th>State</th><th>IF</th></tr></thead>`;

  // sum hax
  const tbd = tbl.tBodies.item(0) || tbl.createTBody();
  for (const row of data.table) {
    if (data.has_wake && !row.wake_status)
      throw TypeError("specified has wake but no wake stats");
    const tr = document.createElement("tr");
    tr.innerHTML = `${
      row.wake_status
        ? `<td><span class="dot ${
            row.wake_status == "succeed" ? "ok" : "bad"
          }" title="${translate_wake_message(row.wake_status)}"></span></td>`
        : ""
    }${Object.entries(status_map)
      .map(([field, description]) => {
        const value = row[field];
        return `<td>${
          value
            ? `<a href="#" class="pick" data-value="${value}" title="filter by ${description}">${value}</a>`
            : ""
        }</td>`;
      })
      .join("")}`;
    tbd.appendChild(tr);
  }

  elHtml.innerHTML = "";
  if (data.filters) {
    const parts = [];
    filter_array.forEach((field) => {
      if (Array.isArray(data.filters[field]) && data.filters[field].length)
        parts.push(`${field}=[${data.filters[field].join(", ")}]`);
    });

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
    if (data.filters.nud?.length > 0) pill.textContent += " (filtered)";
  } else {
    setPill("warn", "unknown");
  }
}

export async function fetchStatus(name, render = true, data_wake) {
  setPill("warn", "checking…");
  const u = buildStatusUrl(name);
  elLog.textContent = "GET " + u.pathname + u.search;
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

    if (data_wake) {
      data.table = merge_wake_data(data.table, data_wake);
      data.has_wake = true;
    }
    if (render) renderStatus(data);
    return data;
  } catch (e) {
    elLog.textContent = "status error: " + e;
    setPill("bad", "error");
  }
}
