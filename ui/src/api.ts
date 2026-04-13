export type Agent = { agent_id: string; connected: boolean };

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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(init?.headers || {}),
    },
  });

  if (!res.ok) {
    let detail = "";
    try {
      detail = JSON.stringify(await res.json(), null, 2);
    } catch {
      detail = await res.text();
    }
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

export function fetchEnrollTokens(
  includeExpired = false,
): Promise<EnrollTokenStatus[]> {
  return request<EnrollTokenStatus[]>(
    `/api/v1/control/enroll-tokens?include_expired=${includeExpired ? "true" : "false"}`,
  );
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
    return { command: { kind: "leases", include_state: true } };
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
