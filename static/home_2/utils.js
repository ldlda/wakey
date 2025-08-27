export function getName(elName) {
  return (elName.value || "").trim();
}

export function saveName(name) {
  try {
    localStorage.setItem("wakey:name", name);
  } catch {}
}

export function loadName(qs) {
  return qs.get("name") || localStorage.getItem("wakey:name") || "";
}

export function extractHostLikeBackend(input) {
  let s = String(input || "").trim();
  if (!s) return "";
  const schemeIdx = s.indexOf("://");
  if (schemeIdx >= 0) s = s.slice(schemeIdx + 3);
  else if (s.startsWith("//")) s = s.slice(2);
  const at = s.lastIndexOf("@");
  if (at >= 0) s = s.slice(at + 1);
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

export function rankState(s) {
  const key = String(s || "").trim().toUpperCase();
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
