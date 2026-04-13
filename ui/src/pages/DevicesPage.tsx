import { useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { runCommand, type Agent } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { DeviceInventoryTable } from "@/pages/devices/DeviceInventoryTable";
import { WakeHistoryCard } from "@/pages/devices/WakeHistoryCard";
import type {
  DeviceRow,
  PresenceFilter,
  SortDir,
  WakeEvent,
} from "@/pages/devices/types";
import {
  chooseWakeTarget,
  loadHistory,
  parseInventoryRows,
  parseWakeSummary,
  PRESENCE_FILTERS,
  saveHistory,
  sorters,
  type SortKey,
} from "@/pages/devices/utils";
import "@/pages/devices/devices.css";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onAfterWake?: () => Promise<void>;
};

function middleEllipsis(value: string, head = 16, tail = 10): string {
  if (value.length <= head + tail + 3) return value;
  return `${value.slice(0, head)}...${value.slice(-tail)}`;
}

export function DevicesPage({
  agents,
  selectedAgentId,
  onSelectAgent,
  onAfterWake,
}: Props) {
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
    return PRESENCE_FILTERS.includes(raw as PresenceFilter)
      ? (raw as PresenceFilter)
      : "all";
  }

  const [sort, setSort] = useState<{ key: SortKey; dir: SortDir }>(
    getSortFromUrl(),
  );
  const [presenceFilter, setPresenceFilter] =
    useState<PresenceFilter>(getPresenceFromUrl());

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

  async function wakeTarget(
    target: string,
    busyId: string,
    opts: { refresh: boolean; notify: boolean } = {
      refresh: true,
      notify: true,
    },
  ) {
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
        await wakeTarget(target, `bulk:${row.id}`, {
          refresh: false,
          notify: false,
        });
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

    async function copyWithFallback(text: string) {
      const textarea = document.createElement("textarea");
      textarea.value = text;
      textarea.setAttribute("readonly", "");
      textarea.style.position = "fixed";
      textarea.style.top = "0";
      textarea.style.left = "0";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.focus();
      textarea.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(textarea);
      if (!ok) throw new Error("fallback-copy-failed");
    }

    try {
      if (window.isSecureContext && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(trimmed);
      } else {
        await copyWithFallback(trimmed);
      }
      setCopyStatus(`Copied ${label}`);
    } catch {
      try {
        await copyWithFallback(trimmed);
        setCopyStatus(`Copied ${label}`);
      } catch {
        setCopyStatus(`Copy failed for ${label}`);
      }
    }
  }

  useEffect(() => {
    void loadInventory();
  }, [selectedAgentId]);

  useEffect(() => {
    setSelectedIds((prev) =>
      prev.filter((id) => rows.some((row) => row.id === id)),
    );
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
      result = result.filter(
        (row) =>
          row.name.toLowerCase().includes(q) ||
          row.presence.toLowerCase().includes(q) ||
          row.ips.some((v) => v.toLowerCase().includes(q)) ||
          row.macs.some((v) => v.toLowerCase().includes(q)) ||
          row.interfaces.some((v) => v.toLowerCase().includes(q)),
      );
    }
    // Sort
    const sorter = sorters[sort.key];
    result = [...result].sort(sorter);
    if (sort.dir === "desc") result.reverse();
    return result;
  }, [rows, query, sort, presenceFilter]);

  const selectedVisibleCount = filtered.filter((row) =>
    selectedIds.includes(row.id),
  ).length;
  const allVisibleSelected =
    filtered.length > 0 && selectedVisibleCount === filtered.length;

  function toggleRowSelection(id: string, checked: boolean) {
    setSelectedIds((prev) => {
      if (checked) return prev.includes(id) ? prev : [...prev, id];
      return prev.filter((v) => v !== id);
    });
  }

  function toggleAllVisible(checked: boolean) {
    if (!checked) {
      setSelectedIds((prev) =>
        prev.filter((id) => !filtered.some((row) => row.id === id)),
      );
      return;
    }
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const row of filtered) next.add(row.id);
      return Array.from(next);
    });
  }

  return (
    <section className="grid gap-3 xl:grid-cols-[minmax(0,2fr)_minmax(20rem,1fr)]">
      <Card className="min-w-0">
        <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
          <CardTitle>Devices</CardTitle>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadInventory()}
            disabled={loading || !selectedAgentId}
          >
            {loading ? "Refreshing..." : "Refresh"}
          </Button>
        </CardHeader>

        <CardContent>
          <div className="mt-2 grid gap-2 md:grid-cols-3">
            <label className="grid min-w-0 gap-1 text-sm text-muted-foreground">
              <span>Agent</span>
              <Select
                value={selectedAgentId}
                onValueChange={(value) => {
                  if (value) onSelectAgent(value);
                }}
              >
                <SelectTrigger className="w-full min-w-0">
                  <span className="min-w-0 flex-1 truncate text-start">
                    {selectedAgentId
                      ? middleEllipsis(selectedAgentId)
                      : "Select agent"}
                  </span>
                </SelectTrigger>
                <SelectContent
                  className="max-w-[min(92vw,30rem)]"
                  alignItemWithTrigger={false}
                >
                  {agents.map((agent) => (
                    <SelectItem
                      key={agent.agent_id}
                      value={agent.agent_id}
                      className="pe-10"
                    >
                      {middleEllipsis(agent.agent_id, 14, 8)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>

            <label className="grid min-w-0 gap-1 text-sm text-muted-foreground md:col-span-2">
              <span>Search</span>
              <Input
                className="w-full min-w-0"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="name, ip, mac, interface"
              />
            </label>
          </div>

          <div className="quick-wake">
            <label>
              Quick Wake (name, IP, or MAC)
              <Input
                value={quickWake}
                onChange={(e) => setQuickWake(e.target.value)}
                placeholder="bedroom-pc or aa:bb:cc:dd:ee:ff"
              />
            </label>
            <Button
              onClick={() => void wakeTarget(quickWake.trim(), "quick")}
              disabled={
                !selectedAgentId || !quickWake.trim() || wakeBusyId === "quick"
              }
            >
              {wakeBusyId === "quick" ? "Waking..." : "Wake target"}
            </Button>
          </div>

          <div
            className="presence-filters"
            role="group"
            aria-label="Filter by presence"
          >
            {PRESENCE_FILTERS.map((value) => (
              <Button
                key={value}
                variant={presenceFilter === value ? "secondary" : "outline"}
                size="xs"
                onClick={() => setPresenceFilter(value)}
                type="button"
              >
                {value === "all" ? "all" : value.replace("_", " ")}
              </Button>
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
            <Button
              onClick={() => void wakeSelectedDevices()}
              disabled={
                !selectedVisibleCount || !selectedAgentId || bulkWakeBusy
              }
              type="button"
              size="sm"
            >
              {bulkWakeBusy
                ? "Waking selected..."
                : `Wake selected (${selectedVisibleCount})`}
            </Button>
            <Button
              onClick={() => setSelectedIds([])}
              disabled={!selectedIds.length}
              type="button"
              variant="outline"
              size="sm"
            >
              Clear selection
            </Button>
            {copyStatus && (
              <span className="text-sm text-muted-foreground">
                {copyStatus}
              </span>
            )}
          </div>

          <p className="text-sm text-muted-foreground">
            Showing {filtered.length} of {rows.length}
          </p>
          {error && (
            <pre className="max-h-80 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
              {error}
            </pre>
          )}

          <DeviceInventoryTable
            rows={filtered}
            selectedIds={selectedIds}
            sort={sort}
            allVisibleSelected={allVisibleSelected}
            wakeBusyId={wakeBusyId}
            selectedAgentId={selectedAgentId}
            bulkWakeBusy={bulkWakeBusy}
            onToggleAllVisible={toggleAllVisible}
            onToggleRow={toggleRowSelection}
            onSortChange={setSort}
            onWakeDevice={(device) => void wakeDevice(device)}
            onCopyValue={(label, value) => void copyValue(label, value)}
          />
        </CardContent>
      </Card>

      <WakeHistoryCard
        events={recentWakes}
        onClear={() => {
          setRecentWakes([]);
          saveHistory([]);
        }}
      />
    </section>
  );
}
