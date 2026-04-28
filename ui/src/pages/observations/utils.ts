import type { AgentDeviceObservation, KnownDevice } from "@/api";

export type ObservationFilter = "all" | "unknown" | "known";

export function observationLabel(observation: AgentDeviceObservation): string {
  return (
    observation.hostname?.trim() ||
    observation.mac?.trim() ||
    observation.ip?.trim() ||
    observation.observation_key
  );
}

export function formatSeen(tsUnix: number): string {
  if (!Number.isFinite(tsUnix) || tsUnix <= 0) return "-";
  return new Date(tsUnix * 1000).toLocaleString();
}

export function identifierSummary(device: KnownDevice): string {
  if (!device.identifiers.length) return "no identifiers";
  return device.identifiers
    .map((identifier) => `${identifier.kind}:${identifier.value}`)
    .join(", ");
}

export function observationStateLabel(action: string): string {
  if (action === "remove") return "stale";
  if (action === "add" || action === "update" || action === "old") {
    return "present";
  }
  return action;
}
