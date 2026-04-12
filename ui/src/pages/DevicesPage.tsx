import { useEffect, useMemo, useState } from "react";

import { runCommand, type Agent } from "@/api";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onAfterWake?: () => Promise<void>;
};

type DeviceRow = {
  id: string;
  name: string;
  ips: string[];
  macs: string[];
  interfaces: string[];
  presence: string;
};

type WakeEvent = {
  ts: number;
  target: string;
  agentId: string;
  outcome: string;
  requestId: string;
  detail: string;
};

const WAKE_HISTORY_KEY = "wakey_recent_wakes_v1";

function parseInventoryRows(payload: unknown): DeviceRow[] {
  if (!payload || typeof payload !== "object") return [];
  const obj = payload as Record<string, unknown>;
  if (obj.kind !== "inventory") return [];
  if (!obj.devices || !Array.isArray(obj.devices)) return [];

  return obj.devices
    .map((raw, idx) => {
      if (!raw || typeof raw !== "object") return null;
      const device = raw as Record<string, unknown>;
      const names = Array.isArray(device.names) ? device.names.filter((v) => typeof v === "string") as string[] : [];
      const ips = Array.isArray(device.ips) ? device.ips.filter((v) => typeof v === "string") as string[] : [];
      const macs = Array.isArray(device.macs) ? device.macs.filter((v) => typeof v === "string") as string[] : [];
      const interfaces = Array.isArray(device.interfaces)
        ? device.interfaces.filter((v) => typeof v === "string") as string[]
        : [];
      const presence = typeof device.presence === "string" ? device.presence : "unknown";

      const id = macs[0] || ips[0] || names[0] || `row-${idx}`;
      return {
        id,
        name: names[0] || "(unnamed)",
        ips,
        macs,
        interfaces,
        presence,
      };
    })
    .filter((v): v is DeviceRow => Boolean(v));
}

function parseWakeSummary(response: unknown): { outcome: string; requestId: string; detail: string } {
  if (!response || typeof response !== "object") {
    return { outcome: "error", requestId: "", detail: "invalid wake response" };
  }
  const envelope = response as Record<string, unknown>;
  const requestId = typeof envelope.request_id === "string" ? envelope.request_id : "";
  const status = typeof envelope.status === "string" ? envelope.status : "error";

  if (status !== "ok") {
    const error = envelope.error as Record<string, unknown> | undefined;
    const detail = typeof error?.message === "string" ? error.message : "wake failed";
    return { outcome: "error", requestId, detail };
  }

  const result = envelope.result as Record<string, unknown> | undefined;
  if (result?.kind !== "wake") {
    return { outcome: "ok", requestId, detail: "wake dispatched" };
  }

  const entries = Array.isArray(result.result) ? result.result : [];
  const statuses = entries
    .map((row) => {
      if (!row || typeof row !== "object") return "";
      const statusRow = (row as Record<string, unknown>).status as Record<string, unknown> | undefined;
      return typeof statusRow?.kind === "string" ? statusRow.kind : "";
    })
    .filter(Boolean);

  const detail = statuses.length ? statuses.join(", ") : "wake dispatched";
  const failed = statuses.some((s) => s === "error" || s === "incomplete");
  return { outcome: failed ? "error" : "ok", requestId, detail };
}

function chooseWakeTarget(device: DeviceRow): string {
  return device.name !== "(unnamed)"
    ? device.name
    : (device.macs[0] || device.ips[0] || "");
}

function loadHistory(): WakeEvent[] {
  try {
    const raw = window.localStorage.getItem(WAKE_HISTORY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((row): row is WakeEvent => {
      if (!row || typeof row !== "object") return false;
      const event = row as Record<string, unknown>;
      return (
        typeof event.ts === "number"
        && typeof event.target === "string"
        && typeof event.agentId === "string"
        && typeof event.outcome === "string"
        && typeof event.requestId === "string"
        && typeof event.detail === "string"
      );
    });
  } catch {
    return [];
  }
}

function saveHistory(history: WakeEvent[]) {
  window.localStorage.setItem(WAKE_HISTORY_KEY, JSON.stringify(history.slice(0, 20)));
}

export function DevicesPage({ agents, selectedAgentId, onSelectAgent, onAfterWake }: Props) {
  const [rows, setRows] = useState<DeviceRow[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [wakeBusyId, setWakeBusyId] = useState("");
  const [recentWakes, setRecentWakes] = useState<WakeEvent[]>([]);

  useEffect(() => {
    setRecentWakes(loadHistory());
  }, []);

  useEffect(() => {
    if (!selectedAgentId && agents[0]) {
      onSelectAgent(agents[0].agent_id);
    }
  }, [selectedAgentId, agents, onSelectAgent]);

  async function loadInventory() {
    if (!selectedAgentId) return;
    setLoading(true);
    setError("");
    try {
      const response = await runCommand(selectedAgentId, "inventory", "");
      if (!response || typeof response !== "object") {
        setRows([]);
      } else {
        const envelope = response as Record<string, unknown>;
        setRows(parseInventoryRows(envelope.result));
      }
    } catch (err) {
      setError(String(err));
      setRows([]);
    } finally {
      setLoading(false);
    }
  }

  async function wakeDevice(device: DeviceRow) {
    const target = chooseWakeTarget(device);
    if (!target || !selectedAgentId) return;
    setWakeBusyId(device.id);
    try {
      const response = await runCommand(selectedAgentId, "wake", target);
      const summary = parseWakeSummary(response);
      const next: WakeEvent[] = [
        {
          ts: Date.now(),
          target,
          agentId: selectedAgentId,
          outcome: summary.outcome,
          requestId: summary.requestId,
          detail: summary.detail,
        },
        ...recentWakes,
      ].slice(0, 20);
      setRecentWakes(next);
      saveHistory(next);
      await loadInventory();
      if (onAfterWake) await onAfterWake();
    } catch (err) {
      const next: WakeEvent[] = [
        {
          ts: Date.now(),
          target,
          agentId: selectedAgentId,
          outcome: "error",
          requestId: "",
          detail: String(err),
        },
        ...recentWakes,
      ].slice(0, 20);
      setRecentWakes(next);
      saveHistory(next);
    } finally {
      setWakeBusyId("");
    }
  }

  useEffect(() => {
    void loadInventory();
  }, [selectedAgentId]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter((row) =>
      row.name.toLowerCase().includes(q)
      || row.presence.toLowerCase().includes(q)
      || row.ips.some((v) => v.toLowerCase().includes(q))
      || row.macs.some((v) => v.toLowerCase().includes(q))
      || row.interfaces.some((v) => v.toLowerCase().includes(q)),
    );
  }, [rows, query]);

  return (
    <section className="two-col">
      <div className="card">
        <div className="row-head">
          <h2>Devices</h2>
          <button onClick={() => void loadInventory()} disabled={loading || !selectedAgentId}>
            {loading ? "Refreshing..." : "Refresh"}
          </button>
        </div>

        <div className="grid-3 compact">
          <label>
            Agent
            <select
              value={selectedAgentId}
              onChange={(e) => onSelectAgent(e.target.value)}
              required
            >
              <option value="" disabled>Select agent</option>
              {agents.map((agent) => (
                <option key={agent.agent_id} value={agent.agent_id}>
                  {agent.agent_id} ({agent.connected ? "connected" : "offline"})
                </option>
              ))}
            </select>
          </label>
          <label className="span-2">
            Search
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="name, ip, mac, interface"
            />
          </label>
        </div>

        <p className="muted">Showing {filtered.length} of {rows.length}</p>
        {error && <pre className="error">{error}</pre>}
        <div className="list device-list">
          {filtered.map((row) => (
            <div className="row plain device-row" key={row.id}>
              <div className="device-main">
                <strong>{row.name}</strong>
                <div className="muted">{row.ips.join(", ") || "-"}</div>
                <div className="muted">{row.macs.join(", ") || "-"}</div>
              </div>
              <div className="device-meta">
                <span>{row.interfaces.join(", ") || "-"}</span>
                <span className="pill">{row.presence}</span>
                <button onClick={() => void wakeDevice(row)} disabled={wakeBusyId === row.id || !selectedAgentId}>
                  {wakeBusyId === row.id ? "Waking..." : "Wake"}
                </button>
              </div>
            </div>
          ))}
          {!filtered.length && <div className="empty">No devices found</div>}
        </div>
      </div>

      <div className="card">
        <div className="row-head">
          <h2>Recent Wake Actions</h2>
          <button
            onClick={() => {
              setRecentWakes([]);
              saveHistory([]);
            }}
          >
            Clear
          </button>
        </div>
        <div className="list">
          {recentWakes.map((event, idx) => (
            <div className="row plain" key={`${event.ts}-${event.target}-${idx}`}>
              <div>
                <strong>{event.target}</strong>
                <div className="muted">{new Date(event.ts).toLocaleString()} on {event.agentId}</div>
                <div className="muted">{event.detail}</div>
              </div>
              <span className={`pill ${event.outcome === "ok" ? "ready" : "error"}`}>
                {event.outcome}
              </span>
            </div>
          ))}
          {!recentWakes.length && <div className="empty">No wake actions yet</div>}
        </div>
      </div>
    </section>
  );
}