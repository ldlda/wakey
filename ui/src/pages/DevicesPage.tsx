
import { useEffect, useMemo, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
// Utility for sorting
const sorters = {
  name: (a: DeviceRow, b: DeviceRow) => a.name.localeCompare(b.name),
  ip: (a: DeviceRow, b: DeviceRow) => (a.ips[0] || "").localeCompare(b.ips[0] || ""),
  mac: (a: DeviceRow, b: DeviceRow) => (a.macs[0] || "").localeCompare(b.macs[0] || ""),
  presence: (a: DeviceRow, b: DeviceRow) => a.presence.localeCompare(b.presence),
};

type SortKey = keyof typeof sorters;
type SortDir = "asc" | "desc";
type PresenceFilter = "all" | "online" | "likely_online" | "unknown";

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
const PRESENCE_FILTERS: PresenceFilter[] = ["all", "online", "likely_online", "unknown"];

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
  const targetOutcomes = entries
    .map((row) => {
      if (!row || typeof row !== "object") return "";
      const rowObj = row as Record<string, unknown>;
      const statusRaw = rowObj.status;
      const status =
        typeof statusRaw === "string"
          ? statusRaw
          : (typeof (statusRaw as Record<string, unknown> | undefined)?.kind === "string"
              ? String((statusRaw as Record<string, unknown>).kind)
              : "unknown");
      const ip = typeof rowObj.ip === "string" ? rowObj.ip : "?";
      const mac = typeof rowObj.mac === "string" ? rowObj.mac : "?";
      return `${status}(${ip}/${mac})`;
    })
    .filter(Boolean);

  const detail = targetOutcomes.length ? targetOutcomes.join(", ") : "wake dispatched";
  const failed = targetOutcomes.some((s) => s.startsWith("incomplete") || s.startsWith("nonexistent_address") || s.startsWith("wrong_size"));
  return { outcome: failed ? "error" : "ok", requestId, detail };
}

function chooseWakeTarget(device: DeviceRow): string {
  return device.name !== "(unnamed)"
    ? device.name
    : (device.macs[0] || device.ips[0] || "");
}

function summarize(values: string[]): string {
  if (!values.length) return "-";
  if (values.length === 1) return values[0];
  return `${values[0]} (+${values.length - 1})`;
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
  const location = useLocation();
  const navigate = useNavigate();

  // Parse sort state from URL
  function getSortFromUrl(): { key: SortKey; dir: SortDir } {
    const params = new URLSearchParams(location.search);
    const key = params.get("sort") as SortKey;
    const dir = params.get("dir") as SortDir;
    if (key && dir && key in sorters && (dir === "asc" || dir === "desc")) {
      return { key, dir };
    }
    return { key: "name", dir: "asc" };
  }

  function getPresenceFromUrl(): PresenceFilter {
    const params = new URLSearchParams(location.search);
    const raw = params.get("presence");
    return PRESENCE_FILTERS.includes(raw as PresenceFilter) ? (raw as PresenceFilter) : "all";
  }

  const [sort, setSort] = useState<{ key: SortKey; dir: SortDir }>(getSortFromUrl());
  const [presenceFilter, setPresenceFilter] = useState<PresenceFilter>(getPresenceFromUrl());

  // Keep view state in sync with URL
  useEffect(() => {
    const params = new URLSearchParams(location.search);
    params.set("sort", sort.key);
    params.set("dir", sort.dir);
    if (presenceFilter === "all") {
      params.delete("presence");
    } else {
      params.set("presence", presenceFilter);
    }
    const nextSearch = params.toString();
    if (nextSearch !== location.search.slice(1)) {
      navigate({ search: nextSearch }, { replace: true });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sort.key, sort.dir, presenceFilter, location.search]);

  // Update state if URL changes (e.g., back/forward nav)
  useEffect(() => {
    setSort(getSortFromUrl());
    setPresenceFilter(getPresenceFromUrl());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [location.search]);

  const [rows, setRows] = useState<DeviceRow[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [wakeBusyId, setWakeBusyId] = useState("");
  const [bulkWakeBusy, setBulkWakeBusy] = useState(false);
  const [quickWake, setQuickWake] = useState("");
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [copyStatus, setCopyStatus] = useState("");
  const [recentWakes, setRecentWakes] = useState<WakeEvent[]>([]);
  function appendWakeEvent(event: WakeEvent) {
    setRecentWakes((prev) => {
      const next = [event, ...prev].slice(0, 20);
      saveHistory(next);
      return next;
    });
  }


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
    await wakeTarget(target, device.id);
  }

  async function wakeTarget(target: string, busyId: string, opts: { refresh: boolean; notify: boolean } = { refresh: true, notify: true }) {
    if (!target || !selectedAgentId) return;
    setWakeBusyId(busyId);
    try {
      const response = await runCommand(selectedAgentId, "wake", target);
      const summary = parseWakeSummary(response);
      appendWakeEvent({
        ts: Date.now(),
        target,
        agentId: selectedAgentId,
        outcome: summary.outcome,
        requestId: summary.requestId,
        detail: summary.detail,
      });
      if (opts.refresh) await loadInventory();
      if (opts.notify && onAfterWake) await onAfterWake();
    } catch (err) {
      appendWakeEvent({
        ts: Date.now(),
        target,
        agentId: selectedAgentId,
        outcome: "error",
        requestId: "",
        detail: String(err),
      });
    } finally {
      setWakeBusyId("");
    }
  }

  async function wakeSelectedDevices() {
    if (!selectedAgentId || bulkWakeBusy) return;
    const selectedRows = filtered.filter((row) => selectedIds.includes(row.id));
    if (!selectedRows.length) return;

    setBulkWakeBusy(true);
    try {
      for (const row of selectedRows) {
        const target = chooseWakeTarget(row);
        if (!target) continue;
        await wakeTarget(target, `bulk:${row.id}`, { refresh: false, notify: false });
      }
      await loadInventory();
      if (onAfterWake) await onAfterWake();
    } finally {
      setBulkWakeBusy(false);
      setWakeBusyId("");
    }
  }

  async function copyValue(label: string, value: string) {
    const trimmed = value.trim();
    if (!trimmed) return;
    try {
      await navigator.clipboard.writeText(trimmed);
      setCopyStatus(`Copied ${label}`);
    } catch {
      setCopyStatus(`Copy failed for ${label}`);
    }
  }

  useEffect(() => {
    void loadInventory();
  }, [selectedAgentId]);

  useEffect(() => {
    setSelectedIds((prev) => prev.filter((id) => rows.some((row) => row.id === id)));
  }, [rows]);

  useEffect(() => {
    if (!copyStatus) return;
    const timer = window.setTimeout(() => setCopyStatus(""), 1800);
    return () => window.clearTimeout(timer);
  }, [copyStatus]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    let result = rows;
    if (presenceFilter !== "all") {
      result = result.filter((row) => row.presence === presenceFilter);
    }
    if (q) {
      result = result.filter((row) =>
        row.name.toLowerCase().includes(q)
        || row.presence.toLowerCase().includes(q)
        || row.ips.some((v) => v.toLowerCase().includes(q))
        || row.macs.some((v) => v.toLowerCase().includes(q))
        || row.interfaces.some((v) => v.toLowerCase().includes(q)),
      );
    }
    // Sort
    const sorter = sorters[sort.key];
    result = [...result].sort(sorter);
    if (sort.dir === "desc") result.reverse();
    return result;
  }, [rows, query, sort, presenceFilter]);

  const selectedVisibleCount = filtered.filter((row) => selectedIds.includes(row.id)).length;
  const allVisibleSelected = filtered.length > 0 && selectedVisibleCount === filtered.length;

  function toggleRowSelection(id: string, checked: boolean) {
    setSelectedIds((prev) => {
      if (checked) return prev.includes(id) ? prev : [...prev, id];
      return prev.filter((v) => v !== id);
    });
  }

  function toggleAllVisible(checked: boolean) {
    if (!checked) {
      setSelectedIds((prev) => prev.filter((id) => !filtered.some((row) => row.id === id)));
      return;
    }
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const row of filtered) next.add(row.id);
      return Array.from(next);
    });
  }

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

        <div className="quick-wake">
          <label>
            Quick Wake (name, IP, or MAC)
            <input
              value={quickWake}
              onChange={(e) => setQuickWake(e.target.value)}
              placeholder="bedroom-pc or aa:bb:cc:dd:ee:ff"
            />
          </label>
          <button
            onClick={() => void wakeTarget(quickWake.trim(), "quick")}
            disabled={!selectedAgentId || !quickWake.trim() || wakeBusyId === "quick"}
          >
            {wakeBusyId === "quick" ? "Waking..." : "Wake target"}
          </button>
        </div>

        <div className="presence-filters" role="group" aria-label="Filter by presence">
          {PRESENCE_FILTERS.map((value) => (
            <button
              key={value}
              className={`filter-chip ${presenceFilter === value ? "active" : ""}`}
              onClick={() => setPresenceFilter(value)}
              type="button"
            >
              {value === "all" ? "all" : value.replace("_", " ")}
            </button>
          ))}
        </div>

        <div className="bulk-actions">
          <label className="select-visible">
            <input
              type="checkbox"
              checked={allVisibleSelected}
              onChange={(e) => toggleAllVisible(e.target.checked)}
              disabled={!filtered.length}
            />
            Select visible
          </label>
          <button
            onClick={() => void wakeSelectedDevices()}
            disabled={!selectedVisibleCount || !selectedAgentId || bulkWakeBusy}
            type="button"
          >
            {bulkWakeBusy ? "Waking selected..." : `Wake selected (${selectedVisibleCount})`}
          </button>
          <button
            onClick={() => setSelectedIds([])}
            disabled={!selectedIds.length}
            type="button"
          >
            Clear selection
          </button>
          {copyStatus && <span className="muted">{copyStatus}</span>}
        </div>

        <p className="muted">Showing {filtered.length} of {rows.length}</p>
        {error && <pre className="error">{error}</pre>}
        <div className="list device-list">
          <div className="row plain device-row device-header">
            <span className="device-cell device-select">
              <input
                type="checkbox"
                checked={allVisibleSelected}
                onChange={(e) => toggleAllVisible(e.target.checked)}
                disabled={!filtered.length}
                aria-label="Select all visible devices"
              />
            </span>
            <span
              className="sortable-col device-cell"
              onClick={() => setSort((s) => ({ key: "name", dir: s.key === "name" && s.dir === "asc" ? "desc" : "asc" }))}
            >
              Name {sort.key === "name" ? (sort.dir === "asc" ? "▲" : "▼") : ""}
            </span>
            <span
              className="sortable-col device-cell"
              onClick={() => setSort((s) => ({ key: "ip", dir: s.key === "ip" && s.dir === "asc" ? "desc" : "asc" }))}
            >
              IP {sort.key === "ip" ? (sort.dir === "asc" ? "▲" : "▼") : ""}
            </span>
            <span
              className="sortable-col device-cell"
              onClick={() => setSort((s) => ({ key: "mac", dir: s.key === "mac" && s.dir === "asc" ? "desc" : "asc" }))}
            >
              MAC {sort.key === "mac" ? (sort.dir === "asc" ? "▲" : "▼") : ""}
            </span>
            <span
              className="sortable-col device-cell"
              onClick={() => setSort((s) => ({ key: "presence", dir: s.key === "presence" && s.dir === "asc" ? "desc" : "asc" }))}
            >
              Presence {sort.key === "presence" ? (sort.dir === "asc" ? "▲" : "▼") : ""}
            </span>
            <span className="device-cell">Interfaces</span>
            <span className="device-cell device-action">Actions</span>
          </div>
          {filtered.map((row) => (
            <div className="row plain device-row" key={row.id}>
              <span className="device-cell device-select" data-label="Pick">
                <input
                  type="checkbox"
                  checked={selectedIds.includes(row.id)}
                  onChange={(e) => toggleRowSelection(row.id, e.target.checked)}
                  aria-label={`Select ${row.name}`}
                />
              </span>
              <span className="device-cell" data-label="Name" title={row.name}>{row.name}</span>
              <span className="device-cell muted" data-label="IP" title={row.ips.join(", ") || "-"}>{summarize(row.ips)}</span>
              <span className="device-cell muted" data-label="MAC" title={row.macs.join(", ") || "-"}>{summarize(row.macs)}</span>
              <span className="device-cell" data-label="Presence"><span className="pill">{row.presence}</span></span>
              <span className="device-cell" data-label="Interfaces" title={row.interfaces.join(", ") || "-"}>{summarize(row.interfaces)}</span>
              <span className="device-cell device-action" data-label="">
                <button onClick={() => void wakeDevice(row)} disabled={wakeBusyId === row.id || !selectedAgentId || bulkWakeBusy}>
                  {wakeBusyId === row.id ? "Waking..." : "Wake"}
                </button>
                <button type="button" className="mini-btn" onClick={() => void copyValue("name", chooseWakeTarget(row))}>Copy name</button>
                <button type="button" className="mini-btn" onClick={() => void copyValue("ip", row.ips[0] || "")}>Copy IP</button>
                <button type="button" className="mini-btn" onClick={() => void copyValue("mac", row.macs[0] || "")}>Copy MAC</button>
              </span>
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
                {event.requestId && (
                  <Link
                    className="muted linkish"
                    to={`/audit?q=${encodeURIComponent(event.requestId)}`}
                  >
                    View related audit ({event.requestId})
                  </Link>
                )}
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