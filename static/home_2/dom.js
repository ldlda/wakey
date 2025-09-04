import { filter_array } from "./status.js";

export const qs = new URLSearchParams(location.search);

// $ is normally queryselector are we fr
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

export function setLink(name, clear) {
  const url = new URL(location.href);
  if (clear) url.searchParams.forEach((_, k) => url.searchParams.delete(k)); // FUCK
  /* const hasExtraFilters = filter_array.some(
    (k) => url.searchParams.getAll(k).length
  );
  if (hasExtraFilters) {
    // jus returns whatever; they are specifying further
  } else */ {
    if (name) url.searchParams.set("name", name);
    else url.searchParams.delete("name");
  }
  history.replaceState(null, "", url);
  link.href = url.toString();
}
