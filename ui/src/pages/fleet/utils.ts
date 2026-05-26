import type { FleetDevice, FleetWakeRoute } from "@/api";

export type SortKey =
  | "name"
  | "presence"
  | "last_seen"
  | "agents"
  | "ip"
  | "mac";
export type SortDir = "asc" | "desc";
export type KnownFilter = "all" | "known" | "unknown";

export const presenceFilters = [
  "all",
  "online",
  "likely_online",
  "offline",
  "unknown",
] as const;

export const knownFilters: KnownFilter[] = ["all", "known", "unknown"];

export function summarize(values: string[]): string {
  if (!values.length) return "-";
  if (values.length <= 2) return values.join(", ");
  return `${values[0]}, ${values[1]} (+${values.length - 2})`;
}

export function formatSeen(value: number | null): string {
  if (!value) return "-";
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - value);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return new Date(value * 1000).toLocaleString();
}

export function presenceRank(presence: string): number {
  if (presence === "online") return 4;
  if (presence === "likely_online") return 3;
  if (presence === "offline") return 2;
  if (presence === "unknown") return 1;
  return 0;
}

export function identifiersFor(
  device: FleetDevice,
): { kind: string; value: string }[] {
  return [
    ...device.macs.map((value) => ({ kind: "mac", value })),
    ...device.ips.map((value) => ({ kind: "ip", value })),
  ];
}

export function agentLabel(agentId: string, nickname?: string | null): string {
  const trimmed = nickname?.trim();
  return trimmed ? trimmed : agentId;
}

export function routeLabel(route: FleetWakeRoute): string {
  const status = route.connected ? "connected" : "agent offline";
  const target = route.mac
    ? `${route.mac}${route.ip ? ` / ${route.ip}` : ""}`
    : (route.ip ?? "-");
  return `${agentLabel(route.agent_id, route.nickname)} · ${target} · ${route.source} · ${status}`;
}
