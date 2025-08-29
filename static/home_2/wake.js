import { elLog, setPill } from "./dom.js";
import { fetchStatus } from "./status.js";

export async function sendWake(name) {
  setPill("warn", "waking…");
  elLog.textContent = "POST /wake?name=" + name;
  try {
    const r = await fetch(`/wake?name=${encodeURIComponent(name)}`, {
      method: "POST",
    });
    const t = await r.text();
    elLog.textContent = t || "ok";
    setTimeout(() => fetchStatus(name), 800);
  } catch (e) {
    elLog.textContent = "wake error: " + e;
    setPill("bad", "error");
  }
}
