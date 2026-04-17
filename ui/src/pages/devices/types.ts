export type DeviceRow = {
  id: string;
  name: string;
  isUnnamed: boolean;
  ips: string[];
  macs: string[];
  interfaces: string[];
  presence: string;
};

export type WakeEvent = {
  ts: number;
  target: string;
  agentId: string;
  outcome: string;
  requestId: string;
  detail: string;
};

export type SortDir = "asc" | "desc";
export type PresenceFilter = "all" | "online" | "likely_online" | "unknown";
