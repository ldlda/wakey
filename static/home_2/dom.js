export const qs = new URLSearchParams(location.search);

export const $ = (id) => document.getElementById(id);
export const elName = $("name");
export const elCheck = $("check");
export const elWake = $("wake");
export const elLog = $("log");
export const elHtml = $("html");
export const elLeases = $("leases_html");
export const pill = $("status-pill");
export const link = $("permalink");
export const elPreview = $("preview");

export function setPill(kind, text) {
  pill.className = `pill ${kind}`;
  pill.textContent = text;
}

export function setLink(name) {
  const url = new URL(location.href);
  if (name) url.searchParams.set("name", name);
  else url.searchParams.delete("name");
  history.replaceState(null, "", url);
  link.href = url.toString();
}
