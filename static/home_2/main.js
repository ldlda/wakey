import {
  elName,
  elCheck,
  elWake,
  elHtml,
  elLeases,
  elPreview,
  setLink,
  setPill,
  qs,
} from "./dom.js";
import {
  getName,
  saveName,
  loadName,
  extractHostLikeBackend,
} from "./utils.js";
import { fetchStatus } from "./status.js";
import { fetchLeases } from "./leases.js";
import { sendWake } from "./wake.js";

// delegated click handler
function handlePickClick(e) {
  const a = e.target && e.target.closest("a.pick");
  if (!a) return;
  e.preventDefault();
  const v = a.getAttribute("data-value");
  pickTarget(v);
}
if (elHtml) elHtml.addEventListener("click", handlePickClick);
if (elLeases) elLeases.addEventListener("click", handlePickClick);

function updatePreview() {
  const raw = getName(elName);
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
  fetchLeases();
}

// events
elCheck.addEventListener("click", () => {
  const name = getName(elName);
  if (!name) return;
  saveName(name);
  setLink(name);
  fetchStatus(name);
  fetchLeases();
});
elWake.addEventListener("click", () => {
  const name = getName(elName);
  if (!name) return;
  saveName(name);
  setLink(name);
  sendWake(name);
  fetchLeases();
});
elName.addEventListener("keydown", (e) => {
  if (e.key === "Enter") elCheck.click();
});
elName.addEventListener("input", updatePreview);

// init
const initial = loadName(qs);
if (initial) {
  elName.value = initial;
  updatePreview();
  setLink(initial);
  fetchStatus(initial);
  fetchLeases();
} else {
  setPill("warn", "unknown");
  fetchLeases();
}
