import type {
  PresenceFilter,
  SortDir,
  DeviceRow,
  WakeEvent,
} from "@/pages/devices/types";

export const WAKE_HISTORY_KEY = "wakey_recent_wakes_v1";

export const PRESENCE_FILTERS: PresenceFilter[] = [
  "all",
  "online",
  "likely_online",
  "unknown",
];

export const sorters = {
  name: (a: DeviceRow, b: DeviceRow) => a.name.localeCompare(b.name),
  ip: (a: DeviceRow, b: DeviceRow) =>
    (a.ips[0] || "").localeCompare(b.ips[0] || ""),
  mac: (a: DeviceRow, b: DeviceRow) =>
    (a.macs[0] || "").localeCompare(b.macs[0] || ""),
  presence: (a: DeviceRow, b: DeviceRow) =>
    a.presence.localeCompare(b.presence),
};

export type SortKey = keyof typeof sorters;

export function parseInventoryRows(payload: unknown): DeviceRow[] {
  if (!payload || typeof payload !== "object") return [];
  const obj = payload as Record<string, unknown>;
  if (obj.kind !== "inventory") return [];
  if (!obj.devices || !Array.isArray(obj.devices)) return [];

  return obj.devices
    .map((raw, idx) => {
      if (!raw || typeof raw !== "object") return null;
      const device = raw as Record<string, unknown>;
      const names = Array.isArray(device.names)
        ? (device.names.filter((v) => typeof v === "string") as string[])
        : [];
      const ips = Array.isArray(device.ips)
        ? (device.ips.filter((v) => typeof v === "string") as string[])
        : [];
      const macs = Array.isArray(device.macs)
        ? (device.macs.filter((v) => typeof v === "string") as string[])
        : [];
      const interfaces = Array.isArray(device.interfaces)
        ? (device.interfaces.filter((v) => typeof v === "string") as string[])
        : [];
      const presence =
        typeof device.presence === "string" ? device.presence : "unknown";
      const isUnnamed = names.length === 0;

      const id = macs[0] || ips[0] || names[0] || `row-${idx}`;
      return {
        id,
        name: names[0] || "(unnamed)",
        isUnnamed,
        ips,
        macs,
        interfaces,
        presence,
      };
    })
    .filter((v): v is DeviceRow => Boolean(v));
}

export function parseWakeSummary(response: unknown): {
  outcome: string;
  requestId: string;
  detail: string;
} {
  if (!response || typeof response !== "object") {
    return { outcome: "error", requestId: "", detail: "invalid wake response" };
  }
  const envelope = response as Record<string, unknown>;
  const requestId =
    typeof envelope.request_id === "string" ? envelope.request_id : "";
  const status =
    typeof envelope.status === "string" ? envelope.status : "error";

  if (status !== "ok") {
    const error = envelope.error as Record<string, unknown> | undefined;
    const detail =
      typeof error?.message === "string" ? error.message : "wake failed";
    return { outcome: "error", requestId, detail };
  }

  const result = envelope.result as Record<string, unknown> | undefined;
  if (result?.kind !== "wake") {
    return { outcome: "ok", requestId, detail: "wake dispatched" };
  }

  const entries = Array.isArray(result.result) ? result.result : [];
  const targetOutcomes = entries
    .map((row) => {
      if (!row || typeof row !== "object") return "";
      const rowObj = row as Record<string, unknown>;
      const statusRaw = rowObj.status;
      const status =
        typeof statusRaw === "string"
          ? statusRaw
          : typeof (statusRaw as Record<string, unknown> | undefined)?.kind ===
              "string"
            ? String((statusRaw as Record<string, unknown>).kind)
            : "unknown";
      const ip = typeof rowObj.ip === "string" ? rowObj.ip : "?";
      const mac = typeof rowObj.mac === "string" ? rowObj.mac : "?";
      return `${status}(${ip}/${mac})`;
    })
    .filter(Boolean);

  const detail = targetOutcomes.length
    ? targetOutcomes.join(", ")
    : "wake dispatched";
  const failed = targetOutcomes.some(
    (s) =>
      s.startsWith("incomplete") ||
      s.startsWith("nonexistent_address") ||
      s.startsWith("wrong_size"),
  );
  return { outcome: failed ? "error" : "ok", requestId, detail };
}

export function chooseWakeTarget(device: DeviceRow): string {
  return device.isUnnamed ? device.macs[0] || device.ips[0] || "" : device.name;
}

export function summarize(values: string[]): string {
  if (!values.length) return "-";
  if (values.length === 1) return values[0];
  return `${values[0]} (+${values.length - 1})`;
}

export function loadHistory(): WakeEvent[] {
  try {
    const raw = window.localStorage.getItem(WAKE_HISTORY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((row): row is WakeEvent => {
      if (!row || typeof row !== "object") return false;
      const event = row as Record<string, unknown>;
      return (
        typeof event.ts === "number" &&
        typeof event.target === "string" &&
        typeof event.agentId === "string" &&
        typeof event.outcome === "string" &&
        typeof event.requestId === "string" &&
        typeof event.detail === "string"
      );
    });
  } catch {
    return [];
  }
}

export function saveHistory(history: WakeEvent[]) {
  window.localStorage.setItem(
    WAKE_HISTORY_KEY,
    JSON.stringify(history.slice(0, 20)),
  );
}

export function nextSort(
  current: { key: SortKey; dir: SortDir },
  key: SortKey,
): { key: SortKey; dir: SortDir } {
  return {
    key,
    dir: current.key === key && current.dir === "asc" ? "desc" : "asc",
  };
}
