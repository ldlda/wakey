export type Agent = {
  agent_id: string;
  connected: boolean;
  nickname?: string | null;
};

export type Alert = {
  alert_id: string;
  kind: string;
  severity: string;
  status: string;
  agent_id: string | null;
  message: string;
  value: number;
  threshold: number;
  last_seen_unix: number;
  metadata: Record<string, unknown>;
};

export type AlertTransition = {
  transition_id: string;
  ts_unix: number;
  alert_id: string;
  kind: string;
  agent_id: string | null;
  from_status: string | null;
  to_status: string;
  message: string;
  metadata: Record<string, unknown>;
};

export type AuditEvent = {
  event_id: string;
  ts_unix: number;
  actor_type: string;
  actor_id: string | null;
  agent_id: string | null;
  request_id: string | null;
  event_type: string;
  outcome: string;
  latency_ms: number | null;
  message: string;
  metadata: Record<string, unknown>;
};

export type CommandKind = "devs" | "leases" | "inventory" | "wake";

export type EnrollTokenStatus = {
  enroll_token: string;
  expires_at_unix: number;
  expired: boolean;
};

export type IssueEnrollTokenResponse = {
  enroll_token: string;
  expires_at_unix: number;
};

export type RevokeEnrollTokenResponse = {
  token: string;
  revoked: boolean;
};

export type RevokeAgentResponse = {
  agent_id: string;
  revoked: boolean;
};

export type SetAgentNicknameResponse = {
  agent_id: string;
  nickname: string | null;
  updated: boolean;
};

export type DeviceIdentifier = {
  identifier_key: string;
  device_id: string;
  kind: string;
  value: string;
  created_at_unix: number;
};

export type KnownDevice = {
  device_id: string;
  display_name: string;
  pinned: boolean;
  created_at_unix: number;
  updated_at_unix: number;
  notes: string | null;
  identifiers: DeviceIdentifier[];
};

export type KnownDeviceSummary = {
  device_id: string;
  display_name: string;
  pinned: boolean;
};

export type AgentDeviceObservation = {
  observation_key: string;
  agent_id: string;
  kind: string;
  mac: string | null;
  ip: string | null;
  hostname: string | null;
  first_seen_unix: number;
  last_seen_unix: number;
  last_action: string;
  known_device: KnownDeviceSummary | null;
};

export type AgentDeviceObservationEvent = {
  event_id: string;
  observation_key: string;
  agent_id: string;
  kind: string;
  action: string;
  mac: string | null;
  ip: string | null;
  hostname: string | null;
  ts_unix: number;
  known_device: KnownDeviceSummary | null;
};

export type FleetDeviceAgent = {
  agent_id: string;
  nickname?: string | null;
  connected: boolean;
  last_seen_unix: number;
};

export type FleetWakeRoute = {
  route_id: string;
  agent_id: string;
  nickname?: string | null;
  connected: boolean;
  mac: string | null;
  ip: string | null;
  hostname: string | null;
  source: string;
  last_seen_unix: number;
  wakeable: boolean;
};

export type FleetDevice = {
  device_key: string;
  display_name: string;
  known_device: KnownDeviceSummary | null;
  pinned: boolean;
  ips: string[];
  macs: string[];
  hostnames: string[];
  agents: FleetDeviceAgent[];
  sources: string[];
  first_seen_unix: number | null;
  last_seen_unix: number | null;
  presence: string;
  route_candidates: FleetWakeRoute[];
  recommended_route: FleetWakeRoute | null;
};

export type RefreshFleetDevicesResponse = {
  total_accepted: number;
  agents: {
    agent_id: string;
    status: string;
    accepted: number;
    error: string | null;
  }[];
};

export type WakeFleetDeviceResponse = {
  route: FleetWakeRoute;
  command: {
    request_id: string;
    status: string;
    result?: unknown;
    error?: { code: string; message: string; retryable?: boolean | null };
  };
};

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(init?.headers || {}),
    },
  });

  if (!res.ok) {
    const raw = await res.text();
    let detail = raw;
    try {
      detail = JSON.stringify(JSON.parse(raw), null, 2);
    } catch {}
    throw new Error(`${res.status} ${res.statusText}\n${detail}`);
  }

  return res.json() as Promise<T>;
}

export function fetchAgents(): Promise<Agent[]> {
  return request<Agent[]>("/api/v1/control/agents");
}

export function fetchAlerts(): Promise<Alert[]> {
  return request<Alert[]>("/api/v1/control/alerts");
}

export function fetchAlertHistory(limit = 50): Promise<AlertTransition[]> {
  return request<AlertTransition[]>(
    `/api/v1/control/alerts/history?limit=${limit}`,
  );
}

export function fetchAudit(limit = 50): Promise<AuditEvent[]> {
  return request<AuditEvent[]>(`/api/v1/control/audit/events?limit=${limit}`);
}

export function fetchEnrollTokens(): Promise<EnrollTokenStatus[]> {
  return request<EnrollTokenStatus[]>(`/api/v1/control/enroll-tokens`);
}

export function issueEnrollToken(
  ttlSeconds: number,
): Promise<IssueEnrollTokenResponse> {
  return request<IssueEnrollTokenResponse>(
    `/api/v1/control/enroll-token?ttl_seconds=${Math.max(1, Math.floor(ttlSeconds))}`,
    { method: "POST" },
  );
}

export function revokeEnrollToken(
  token: string,
): Promise<RevokeEnrollTokenResponse> {
  return request<RevokeEnrollTokenResponse>(
    `/api/v1/control/enroll-tokens/${encodeURIComponent(token)}`,
    { method: "DELETE" },
  );
}

export function revokeAgent(agentId: string): Promise<RevokeAgentResponse> {
  return request<RevokeAgentResponse>(
    `/api/v1/control/agents/${encodeURIComponent(agentId)}`,
    { method: "DELETE" },
  );
}

export function setAgentNickname(
  agentId: string,
  nickname: string | null,
): Promise<SetAgentNicknameResponse> {
  const normalized = nickname?.trim() ?? "";
  return request<SetAgentNicknameResponse>(
    `/api/v1/control/agents/${encodeURIComponent(agentId)}/nickname`,
    {
      method: "PATCH",
      body: JSON.stringify({ nickname: normalized ? normalized : null }),
    },
  );
}

export function fetchKnownDevices(): Promise<KnownDevice[]> {
  return request<KnownDevice[]>("/api/v1/control/devices");
}

export function fetchFleetDevices(opts?: {
  query?: string;
  presence?: string;
  known?: string;
  agentId?: string;
  visibility?: "operator" | "all";
  limit?: number;
}): Promise<FleetDevice[]> {
  const params = new URLSearchParams();
  if (opts?.query) params.set("query", opts.query);
  if (opts?.presence && opts.presence !== "all") {
    params.set("presence", opts.presence);
  }
  if (opts?.known && opts.known !== "all") params.set("known", opts.known);
  if (opts?.agentId) params.set("agent_id", opts.agentId);
  if (opts?.visibility) params.set("visibility", opts.visibility);
  params.set("limit", String(opts?.limit ?? 500));
  return request<FleetDevice[]>(
    `/api/v1/control/fleet/devices?${params.toString()}`,
  );
}

export function refreshFleetDevices(input?: {
  agentIds?: string[];
  timeoutMs?: number;
}): Promise<RefreshFleetDevicesResponse> {
  return request<RefreshFleetDevicesResponse>(
    "/api/v1/control/fleet/devices/refresh",
    {
      method: "POST",
      body: JSON.stringify({
        agent_ids: input?.agentIds ?? [],
        timeout_ms: input?.timeoutMs ?? null,
      }),
    },
  );
}

export function wakeFleetDevice(input: {
  deviceKey: string;
  routeId?: string | null;
  timeoutMs?: number;
}): Promise<WakeFleetDeviceResponse> {
  return request<WakeFleetDeviceResponse>("/api/v1/control/fleet/wake", {
    method: "POST",
    body: JSON.stringify({
      device_key: input.deviceKey,
      route_id: input.routeId ?? null,
      timeout_ms: input.timeoutMs ?? null,
    }),
  });
}

export function fetchObservations(opts?: {
  agentId?: string;
  limit?: number;
}): Promise<AgentDeviceObservation[]> {
  const params = new URLSearchParams();
  if (opts?.agentId) params.set("agent_id", opts.agentId);
  params.set("limit", String(opts?.limit ?? 500));
  return request<AgentDeviceObservation[]>(
    `/api/v1/control/observations?${params.toString()}`,
  );
}

export function fetchObservationHistory(opts?: {
  agentId?: string;
  kind?: string;
  mac?: string;
  ip?: string;
  observationKey?: string;
  limit?: number;
}): Promise<AgentDeviceObservationEvent[]> {
  const params = new URLSearchParams();
  if (opts?.agentId) params.set("agent_id", opts.agentId);
  if (opts?.kind) params.set("kind", opts.kind);
  if (opts?.mac) params.set("mac", opts.mac);
  if (opts?.ip) params.set("ip", opts.ip);
  if (opts?.observationKey) params.set("observation_key", opts.observationKey);
  params.set("limit", String(opts?.limit ?? 500));
  return request<AgentDeviceObservationEvent[]>(
    `/api/v1/control/observations/history?${params.toString()}`,
  );
}

export function createKnownDevice(input: {
  display_name: string;
  pinned?: boolean;
  notes?: string | null;
  identifiers?: { kind: string; value: string }[];
}): Promise<KnownDevice> {
  return request<KnownDevice>("/api/v1/control/devices", {
    method: "POST",
    body: JSON.stringify({
      display_name: input.display_name,
      pinned: input.pinned ?? true,
      notes: input.notes ?? null,
      identifiers: input.identifiers ?? [],
    }),
  });
}

export function attachObservationIdentifier(
  deviceId: string,
  observationKey: string,
): Promise<KnownDevice> {
  return request<KnownDevice>(
    `/api/v1/control/devices/${encodeURIComponent(deviceId)}/identifiers/from-observation`,
    {
      method: "POST",
      body: JSON.stringify({ observation_key: observationKey }),
    },
  );
}

export function attachDeviceIdentifier(
  deviceId: string,
  identifier: { kind: string; value: string },
): Promise<KnownDevice> {
  return request<KnownDevice>(
    `/api/v1/control/devices/${encodeURIComponent(deviceId)}/identifiers`,
    {
      method: "POST",
      body: JSON.stringify(identifier),
    },
  );
}

export function mergeKnownDevice(
  targetDeviceId: string,
  sourceDeviceId: string,
): Promise<KnownDevice> {
  return request<KnownDevice>(
    `/api/v1/control/devices/${encodeURIComponent(targetDeviceId)}/merge`,
    {
      method: "POST",
      body: JSON.stringify({ source_device_id: sourceDeviceId }),
    },
  );
}

export function runCommand(
  agentId: string,
  kind: CommandKind,
  query: string,
): Promise<unknown> {
  const payload = buildCommandPayload(kind, query);
  return request(
    `/api/v1/control/agents/${encodeURIComponent(agentId)}/command`,
    {
      method: "POST",
      body: JSON.stringify(payload),
    },
  );
}

function buildCommandPayload(
  kind: CommandKind,
  query: string,
): { command: Record<string, unknown> } {
  if (kind === "devs") {
    return { command: { kind: "devs", dev: null, up_only: false } };
  }
  if (kind === "leases") {
    return { command: { kind: "leases", include_state: false } };
  }
  if (kind === "inventory") {
    return {
      command: {
        kind: "inventory",
        query: query || null,
        name: null,
        ips: [],
        devs: [],
        nuds: [],
        macs: [],
      },
    };
  }
  return {
    command: {
      kind: "wake",
      query: query || null,
      mac: null,
      ip: null,
    },
  };
}
