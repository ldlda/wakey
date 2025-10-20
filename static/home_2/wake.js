import { elHtml, elLog, setPill } from "./dom.js";
import { fetchStatus, renderStatus } from "./status.js";

/**
 * @param {String} name just plain name
 */
export async function sendWake(name) {
  const data = await fetchStatus(name, false);
  if (!data) return; // can not proceed; theres nothing.
  const wake_targets = data.table.map(({ ip, mac }) => {
    return { ip, mac };
  });
  setPill("warn", "waking…");
  elLog.textContent = "POST /api/wake";
  try {
    const r = await fetch(`/api/wake`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(wake_targets),
    });
    if (!r.ok) {
      let msg = String(r.status);
      try {
        const err = await r.clone().json();
        msg = err.error || JSON.stringify(err);
      } catch {
        msg = await r.text();
      }
      elLog.textContent = `wake error: ${msg}`;
      setPill("bad", "error");
      return;
    }

    const j = await r.json();

    if (!j.success) {
      elLog.textContent = `wake error: ${j.error}`;
      setPill("bad", "error");
      return;
    }

    const result = j.result;
    data.table = merge_wake_data(data.table, result);
    data.has_wake = true;

    renderStatus(data);

    setTimeout(() => fetchStatus(name, undefined, result), 2000); // long ass timeout
  } catch (e) {
    elLog.textContent = "wake error: " + e;
    setPill("bad", "error");
  }
}

/**
 *
 * @param {{
 *          ip: String
 *          dev: String
 *          mac: String
 *          state: String
 *          }[]} table
 * @param {{
 *          ip: String,
 *          mac: String,
 *          status: "incomplete" | "succeed" | "nonexistent_address" | "wrong_size" 
 *          }[]} wake
 * @returns {{
 *          ip: String
 *          dev: String
 *          mac: String
 *          state: String
 *          }[] | {
 *          ip: String
 *          dev: String
 *          mac: String
 *          state: String
 *          wake_status: boolean
 *          }[]}
 */
export function merge_wake_data(table, wake) {
  if (!wake) return table;
  if (wake.length != table.length)
    throw TypeError(
      "wake status table and status table not of the same length"
    );
  const return_array = [];
  let linear_failed = false;
  for (const [index, entry] of table.entries()) {
    if (wake[index].ip != entry.ip || wake[index].mac != entry.mac) {
      linear_failed = true;
      break;
    } // use alternative method

    return_array.push({ wake_status: wake[index].status, ...entry });
  }

  // never happening AHH
  if (linear_failed) {
    return_array = [];
    const wake_map = new Map();
    wake.forEach(({ ip, mac, status }) => {
      wake_map.set(JSON.stringify({ ip, mac }), status);
    });
    return_array = table.map((entry) => {
      const { ip, mac } = entry;
      return {
        wake_status: wake_map.get(JSON.stringify({ ip, mac })),
        ...entry,
      };
    });
  }
  //
  return return_array;
}

/**
 * 
 * @param {"incomplete" | "succeed" | "nonexistent_address" | "wrong_size" | any} wake_msg 
 * @returns {string}
 */
export function translate_wake_message(wake_msg) {
  switch (wake_msg) {
    case "incomplete":
      return "Incomplete address (both ip and MAC required)"
    case "succeed":
      return "Wake request sent successfully"
    case "nonexistent_address":
      return "Errored pinging this address (nonexistent address)"
    case "wrong_size":
      return "Wake request malformed"
    default:
      return "Unknown"
  }
}