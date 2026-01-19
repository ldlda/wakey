import { elLeases } from "./dom.js";
import { rankState } from "./utils.js";

/**
 *
 * @param {{
 *          expires_epoch: Number
 *          rank?: Number
 *          ip: String
 *          mac: String
 *          nud_state?: String
 *          name?: String
 *      }[]} leases
 * @returns
 */
export function renderLeases(leases) {
  if (!elLeases) return;
  if (!Array.isArray(leases) || leases.length === 0) {
    elLeases.textContent = "no leases";
    return;
  }
  const tbl = document.createElement("table");
  tbl.className = "table";

  (
    tbl.tHead || tbl.createTHead()
  ).innerHTML = `<tr><th></th><th>IP</th><th>MAC</th><th>Name</th><th>Expires</th></tr>`;

  const tbd = tbl.tBodies.item(0) || tbl.createTBody();
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
    } else if (l?.nud_state) {
      if (rankState(l.nud_state) >= 5) dotClass = "dot ok";
      else if (rankState(l.nud_state) >= 2) dotClass = "dot warn";
      else dotClass = "dot bad";
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
      ${Object.entries({ ip, mac, name })
        .map(
          ([name, value]) =>
            `<td>${
              value &&
              `<a href="#" class="pick" data-value="${value}" title="filter by ${name}">${value}</a>`
            }</td>`
        )
        .join("\n")}
      <td><span class="tiny">${whenText}</span></td>`;
    tbd.appendChild(tr);
  }
  elLeases.innerHTML = "";
  elLeases.appendChild(tbl);
}

export async function fetchLeases() {
  try {
    const r = await fetch("/api/dhcp_leases?include_state=1");
    if (!r.ok) return;
    const leases = await r.json();
    renderLeases(leases);
  } catch {}
}
