import { useEffect, useMemo, useState } from "react";
import {
  Check,
  Copy,
  MoreHorizontal,
  RefreshCw,
  Search,
  Zap,
} from "lucide-react";

import {
  attachDeviceIdentifier,
  createKnownDevice,
  fetchFleetDevices,
  fetchKnownDevices,
  mergeKnownDevice,
  refreshFleetDevices,
  wakeFleetDevice,
  type Agent,
  type FleetDevice,
  type FleetWakeRoute,
  type KnownDevice,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onAfterWake?: () => Promise<void>;
};

type SortKey = "name" | "presence" | "last_seen" | "agents" | "ip" | "mac";
type SortDir = "asc" | "desc";
type KnownFilter = "all" | "known" | "unknown";

const presenceFilters = [
  "all",
  "online",
  "likely_online",
  "unknown",
  "offline",
] as const;

const knownFilters: KnownFilter[] = ["all", "known", "unknown"];

export function DevicesPage({ agents, onAfterWake }: Props) {
  const [devices, setDevices] = useState<FleetDevice[]>([]);
  const [knownDevices, setKnownDevices] = useState<KnownDevice[]>([]);
  const [query, setQuery] = useState("");
  const [presence, setPresence] = useState("all");
  const [known, setKnown] = useState<KnownFilter>("all");
  const [agentId, setAgentId] = useState("all");
  const [sort, setSort] = useState<{ key: SortKey; dir: SortDir }>({
    key: "last_seen",
    dir: "desc",
  });
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [wakeBusyKey, setWakeBusyKey] = useState("");
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [copied, setCopied] = useState("");
  const [details, setDetails] = useState<FleetDevice | null>(null);

  async function loadFleet() {
    setLoading(true);
    setError("");
    try {
      const [nextDevices, nextKnownDevices] = await Promise.all([
        fetchFleetDevices({
          query,
          presence,
          known,
          agentId: agentId === "all" ? "" : agentId,
          limit: 500,
        }),
        fetchKnownDevices(),
      ]);
      setDevices(nextDevices);
      setKnownDevices(nextKnownDevices);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  async function refreshAllConnected() {
    setRefreshing(true);
    setStatus("");
    setError("");
    try {
      const result = await refreshFleetDevices();
      const failed = result.agents.filter((agent) => agent.status !== "ok");
      setStatus(
        failed.length
          ? `Stored ${result.total_accepted} observations; ${failed.length} agent refresh failed`
          : `Stored ${result.total_accepted} observations from ${result.agents.length} connected agents`,
      );
      await loadFleet();
    } catch (err) {
      setError(String(err));
    } finally {
      setRefreshing(false);
    }
  }

  async function wakeDevice(device: FleetDevice, routeId?: string | null) {
    if (!device.recommended_route && !routeId) return;
    setWakeBusyKey(device.device_key);
    setStatus("");
    setError("");
    try {
      const result = await wakeFleetDevice({
        deviceKey: device.device_key,
        routeId,
      });
      setStatus(
        `Wake sent via ${agentLabel(result.route.agent_id, result.route.nickname)} (${result.route.mac ?? "no mac"})`,
      );
      if (onAfterWake) await onAfterWake();
      await loadFleet();
    } catch (err) {
      setError(String(err));
    } finally {
      setWakeBusyKey("");
    }
  }

  async function copyValue(label: string, value: string) {
    const text = value.trim();
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(label);
    } catch {
      const area = document.createElement("textarea");
      area.value = text;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      document.execCommand("copy");
      document.body.removeChild(area);
      setCopied(label);
    }
  }

  useEffect(() => {
    void loadFleet();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [presence, known, agentId]);

  useEffect(() => {
    const timer = window.setTimeout(() => void loadFleet(), 220);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query]);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(""), 1400);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const sortedDevices = useMemo(() => {
    const rows = [...devices];
    rows.sort((a, b) => {
      const dir = sort.dir === "asc" ? 1 : -1;
      if (sort.key === "presence") {
        return dir * (presenceRank(a.presence) - presenceRank(b.presence));
      }
      if (sort.key === "last_seen") {
        return dir * ((a.last_seen_unix ?? 0) - (b.last_seen_unix ?? 0));
      }
      if (sort.key === "agents")
        return dir * (a.agents.length - b.agents.length);
      if (sort.key === "ip") {
        return dir * (a.ips[0] ?? "").localeCompare(b.ips[0] ?? "");
      }
      if (sort.key === "mac") {
        return dir * (a.macs[0] ?? "").localeCompare(b.macs[0] ?? "");
      }
      return dir * a.display_name.localeCompare(b.display_name);
    });
    return rows;
  }, [devices, sort]);

  const connectedCount = agents.filter((agent) => agent.connected).length;

  return (
    <section className="grid gap-3">
      <Card>
        <CardHeader className="gap-3">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <CardTitle>Devices</CardTitle>
              <p className="mt-1 text-sm text-muted-foreground">
                {devices.length} fleet rows from stored observations and known
                devices
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => void loadFleet()}
                disabled={loading}
              >
                <RefreshCw className="size-4" aria-hidden />
                {loading ? "Refreshing" : "Refresh view"}
              </Button>
              <Button
                size="sm"
                onClick={() => void refreshAllConnected()}
                disabled={refreshing || connectedCount === 0}
              >
                <RefreshCw className="size-4" aria-hidden />
                {refreshing
                  ? "Collecting"
                  : `Collect fleet (${connectedCount})`}
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-2 lg:grid-cols-[minmax(16rem,1fr)_11rem_10rem_14rem_10rem]">
            <label className="grid gap-1 text-sm text-muted-foreground">
              <span>Search</span>
              <div className="relative">
                <Search className="absolute left-2 top-2.5 size-4 text-muted-foreground" />
                <Input
                  className="pl-8"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="name, ip, mac, agent"
                />
              </div>
            </label>
            <FilterSelect
              label="Presence"
              value={presence}
              values={presenceFilters}
              onChange={setPresence}
            />
            <FilterSelect
              label="Identity"
              value={known}
              values={knownFilters}
              onChange={(value) => setKnown(value as KnownFilter)}
            />
            <label className="grid gap-1 text-sm text-muted-foreground">
              <span>Agent</span>
              <Select
                value={agentId}
                onValueChange={(value) => setAgentId(value ?? "all")}
              >
                <SelectTrigger>
                  <span>
                    {agentId === "all"
                      ? "all agents"
                      : agentLabel(
                          agentId,
                          agents.find((agent) => agent.agent_id === agentId)
                            ?.nickname,
                        )}
                  </span>
                </SelectTrigger>
                <SelectContent alignItemWithTrigger={false}>
                  <SelectItem value="all">all agents</SelectItem>
                  {agents.map((agent) => (
                    <SelectItem key={agent.agent_id} value={agent.agent_id}>
                      {agentLabel(agent.agent_id, agent.nickname)}
                      {agent.connected ? " · connected" : " · offline"}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            <FilterSelect
              label="Sort"
              value={`${sort.key}:${sort.dir}`}
              values={[
                "last_seen:desc",
                "name:asc",
                "presence:desc",
                "agents:desc",
                "ip:asc",
                "mac:asc",
              ]}
              onChange={(value) => {
                const [key, dir] = value.split(":") as [SortKey, SortDir];
                setSort({ key, dir });
              }}
            />
          </div>

          {status && (
            <div className="rounded-md border bg-muted/50 px-3 py-2 text-sm">
              {status}
            </div>
          )}
          {error && (
            <pre className="max-h-80 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
              {error}
            </pre>
          )}
          {copied && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Check className="size-4" aria-hidden />
              Copied {copied}
            </div>
          )}

          <div className="grid gap-2">
            <div className="hidden grid-cols-[minmax(10rem,1.3fr)_8rem_minmax(9rem,1fr)_minmax(10rem,1fr)_minmax(10rem,1fr)_8rem_8rem] gap-2 rounded-md border bg-muted/60 px-3 py-2 text-sm font-medium xl:grid">
              <span>Device</span>
              <span>Presence</span>
              <span>IPs</span>
              <span>MACs</span>
              <span>Agents</span>
              <span>Last seen</span>
              <span className="text-right">Actions</span>
            </div>
            {sortedDevices.map((device) => (
              <FleetDeviceRow
                key={device.device_key}
                device={device}
                busy={wakeBusyKey === device.device_key}
                onWake={() => void wakeDevice(device)}
                onDetails={() => setDetails(device)}
                onCopy={(label, value) => void copyValue(label, value)}
              />
            ))}
            {!sortedDevices.length && (
              <div className="rounded-md border bg-card p-4 text-sm text-muted-foreground">
                No devices match the current filters.
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      <FleetDeviceDetailsDialog
        device={details}
        knownDevices={knownDevices}
        open={Boolean(details)}
        busy={wakeBusyKey === details?.device_key}
        onOpenChange={(open) => {
          if (!open) setDetails(null);
        }}
        onWake={(routeId) => {
          if (details) void wakeDevice(details, routeId);
        }}
        onCopy={(label, value) => void copyValue(label, value)}
        onChanged={() => void loadFleet()}
      />
    </section>
  );
}

function FleetDeviceRow({
  device,
  busy,
  onWake,
  onDetails,
  onCopy,
}: {
  device: FleetDevice;
  busy: boolean;
  onWake: () => void;
  onDetails: () => void;
  onCopy: (label: string, value: string) => void;
}) {
  return (
    <div className="grid gap-3 rounded-md border bg-card px-3 py-3 text-sm xl:grid-cols-[minmax(10rem,1.3fr)_8rem_minmax(9rem,1fr)_minmax(10rem,1fr)_minmax(10rem,1fr)_8rem_8rem] xl:items-center xl:gap-2">
      <button className="min-w-0 text-left" type="button" onClick={onDetails}>
        <div className="truncate font-medium">{device.display_name}</div>
        <div className="mt-1 flex flex-wrap gap-1">
          {device.known_device ? (
            <Badge variant="secondary">known</Badge>
          ) : (
            <Badge variant="outline">unknown</Badge>
          )}
          {device.pinned && <Badge variant="outline">pinned</Badge>}
          {device.sources.map((source) => (
            <Badge key={source} variant="outline">
              {source}
            </Badge>
          ))}
        </div>
      </button>
      <span>
        <PresenceBadge presence={device.presence} />
      </span>
      <span className="min-w-0 text-muted-foreground xl:truncate">
        <MobileLabel label="IPs" />
        {summarize(device.ips)}
      </span>
      <span className="min-w-0 text-muted-foreground xl:truncate">
        <MobileLabel label="MACs" />
        {summarize(device.macs)}
      </span>
      <span className="min-w-0 text-muted-foreground xl:truncate">
        <MobileLabel label="Agents" />
        {summarize(
          device.agents.map((agent) =>
            agentLabel(agent.agent_id, agent.nickname),
          ),
        )}
      </span>
      <span className="text-muted-foreground">
        <MobileLabel label="Last seen" />
        {formatSeen(device.last_seen_unix)}
      </span>
      <div className="flex justify-end gap-2">
        <Button
          size="sm"
          onClick={onWake}
          disabled={busy || !device.recommended_route}
        >
          <Zap className="size-4" aria-hidden />
          {busy ? "Waking" : "Wake"}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger className="inline-flex size-9 items-center justify-center rounded-md border bg-background">
            <MoreHorizontal className="size-4" aria-hidden />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            <DropdownMenuItem onClick={onDetails}>Details</DropdownMenuItem>
            <DropdownMenuItem
              onClick={() => onCopy("IP", device.ips[0] ?? "")}
              data-disabled={!device.ips.length}
            >
              Copy first IP
            </DropdownMenuItem>
            <DropdownMenuItem
              onClick={() => onCopy("MAC", device.macs[0] ?? "")}
              data-disabled={!device.macs.length}
            >
              Copy first MAC
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}

function FleetDeviceDetailsDialog({
  device,
  knownDevices,
  open,
  busy,
  onOpenChange,
  onWake,
  onCopy,
  onChanged,
}: {
  device: FleetDevice | null;
  knownDevices: KnownDevice[];
  open: boolean;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onWake: (routeId?: string | null) => void;
  onCopy: (label: string, value: string) => void;
  onChanged: () => void;
}) {
  const [displayName, setDisplayName] = useState("");
  const [targetDeviceId, setTargetDeviceId] = useState("");
  const [routeId, setRouteId] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    setDisplayName(device?.display_name ?? "");
    setTargetDeviceId("");
    setRouteId(device?.recommended_route?.route_id ?? "");
    setError("");
  }, [device]);

  if (!device) return null;

  const currentDevice = device;
  const identifiers = identifiersFor(currentDevice);
  const otherKnownDevices = knownDevices.filter(
    (known) => known.device_id !== currentDevice.known_device?.device_id,
  );

  async function createRemembered() {
    setError("");
    try {
      await createKnownDevice({
        display_name: displayName.trim() || currentDevice.display_name,
        pinned: true,
        identifiers,
      });
      onChanged();
    } catch (err) {
      setError(String(err));
    }
  }

  async function attachToExisting() {
    if (!targetDeviceId) return;
    setError("");
    try {
      for (const identifier of identifiers) {
        await attachDeviceIdentifier(targetDeviceId, identifier);
      }
      onChanged();
    } catch (err) {
      setError(String(err));
    }
  }

  async function mergeIntoExisting() {
    if (!targetDeviceId || !currentDevice.known_device) return;
    setError("");
    try {
      await mergeKnownDevice(
        targetDeviceId,
        currentDevice.known_device.device_id,
      );
      onChanged();
      onOpenChange(false);
    } catch (err) {
      setError(String(err));
    }
  }

  const selectedRoute =
    device.route_candidates.find((route) => route.route_id === routeId) ??
    device.recommended_route;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[92vh] overflow-auto sm:max-w-3xl">
        <DialogHeader>
          <div className="flex min-w-0 items-start justify-between gap-3 pr-8">
            <div className="min-w-0">
              <DialogTitle className="truncate">
                {device.display_name}
              </DialogTitle>
              <DialogDescription>
                {device.known_device
                  ? "Remembered fleet device"
                  : "Observed fleet device"}
              </DialogDescription>
            </div>
            <PresenceBadge presence={device.presence} />
          </div>
        </DialogHeader>

        {error && (
          <pre className="max-h-40 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
            {error}
          </pre>
        )}

        <div className="grid gap-3 md:grid-cols-2">
          <DetailBlock label="IPs" values={device.ips} onCopy={onCopy} />
          <DetailBlock label="MACs" values={device.macs} onCopy={onCopy} />
          <DetailBlock
            label="Hostnames"
            values={device.hostnames}
            onCopy={onCopy}
          />
          <DetailBlock
            label="Agents"
            values={device.agents.map((agent) =>
              agentLabel(agent.agent_id, agent.nickname),
            )}
            onCopy={onCopy}
          />
        </div>

        <Separator />

        <div className="grid gap-2">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-sm font-medium">Wake route</h3>
            <Button
              size="sm"
              onClick={() => onWake(selectedRoute?.route_id ?? null)}
              disabled={busy || !selectedRoute?.wakeable}
            >
              <Zap className="size-4" aria-hidden />
              {busy ? "Waking" : "Wake"}
            </Button>
          </div>
          <Select
            value={routeId}
            onValueChange={(value) => setRouteId(value ?? "")}
          >
            <SelectTrigger>
              <span>
                {selectedRoute
                  ? routeLabel(selectedRoute)
                  : "No connected MAC-backed route"}
              </span>
            </SelectTrigger>
            <SelectContent alignItemWithTrigger={false}>
              {device.route_candidates.map((route) => (
                <SelectItem key={route.route_id} value={route.route_id}>
                  {routeLabel(route)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <Separator />

        <div className="grid gap-3">
          <h3 className="text-sm font-medium">Remember / merge</h3>
          {!device.known_device && (
            <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
              <Input
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder="Device name"
              />
              <Button
                onClick={() => void createRemembered()}
                disabled={!identifiers.length}
              >
                Remember device
              </Button>
            </div>
          )}
          <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto_auto]">
            <Select
              value={targetDeviceId}
              onValueChange={(value) => setTargetDeviceId(value ?? "")}
            >
              <SelectTrigger>
                <span>
                  {targetDeviceId
                    ? knownDevices.find(
                        (known) => known.device_id === targetDeviceId,
                      )?.display_name
                    : "Select known device"}
                </span>
              </SelectTrigger>
              <SelectContent
                className="max-w-[min(92vw,30rem)]"
                alignItemWithTrigger={false}
              >
                {otherKnownDevices.map((known) => (
                  <SelectItem key={known.device_id} value={known.device_id}>
                    {known.display_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              onClick={() => void attachToExisting()}
              disabled={!targetDeviceId || !identifiers.length}
            >
              Attach IDs
            </Button>
            <Button
              variant="outline"
              onClick={() => void mergeIntoExisting()}
              disabled={!targetDeviceId || !device.known_device}
            >
              Merge
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            Proposed identifiers:{" "}
            {identifiers.map((id) => `${id.kind}:${id.value}`).join(", ") ||
              "-"}
          </p>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function FilterSelect({
  label,
  value,
  values,
  onChange,
}: {
  label: string;
  value: string;
  values: readonly string[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid gap-1 text-sm text-muted-foreground">
      <span>{label}</span>
      <Select value={value} onValueChange={(next) => next && onChange(next)}>
        <SelectTrigger>
          <span>{value.replace("_", " ")}</span>
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          {values.map((item) => (
            <SelectItem key={item} value={item}>
              {item.replace("_", " ")}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  );
}

function DetailBlock({
  label,
  values,
  onCopy,
}: {
  label: string;
  values: string[];
  onCopy: (label: string, value: string) => void;
}) {
  return (
    <div className="rounded-md border bg-muted/30 p-3">
      <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
        {label}
      </div>
      <div className="grid gap-1">
        {values.length ? (
          values.map((value) => (
            <button
              key={value}
              type="button"
              className="flex min-w-0 items-center justify-between gap-2 rounded px-1 py-0.5 text-left hover:bg-accent"
              onClick={() => onCopy(label, value)}
            >
              <span className="min-w-0 truncate">{value}</span>
              <Copy
                className="size-3.5 shrink-0 text-muted-foreground"
                aria-hidden
              />
            </button>
          ))
        ) : (
          <span className="text-sm text-muted-foreground">-</span>
        )}
      </div>
    </div>
  );
}

function PresenceBadge({ presence }: { presence: string }) {
  return (
    <Badge variant={presence === "offline" ? "outline" : "secondary"}>
      {presence.replace("_", " ")}
    </Badge>
  );
}

function MobileLabel({ label }: { label: string }) {
  return (
    <span className="mb-1 block text-xs font-medium text-muted-foreground xl:hidden">
      {label}
    </span>
  );
}

function summarize(values: string[]): string {
  if (!values.length) return "-";
  if (values.length <= 2) return values.join(", ");
  return `${values[0]}, ${values[1]} (+${values.length - 2})`;
}

function formatSeen(value: number | null): string {
  if (!value) return "-";
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - value);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return new Date(value * 1000).toLocaleString();
}

function presenceRank(presence: string): number {
  if (presence === "online") return 4;
  if (presence === "likely_online") return 3;
  if (presence === "unknown") return 2;
  if (presence === "offline") return 1;
  return 0;
}

function identifiersFor(
  device: FleetDevice,
): { kind: string; value: string }[] {
  return [
    ...device.macs.map((value) => ({ kind: "mac", value })),
    ...device.ips.map((value) => ({ kind: "ip", value })),
  ];
}

function agentLabel(agentId: string, nickname?: string | null): string {
  const trimmed = nickname?.trim();
  return trimmed ? trimmed : agentId;
}

function routeLabel(route: FleetWakeRoute): string {
  const status = route.connected ? "connected" : "offline";
  const target = route.mac
    ? `${route.mac}${route.ip ? ` / ${route.ip}` : ""}`
    : (route.ip ?? "-");
  return `${agentLabel(route.agent_id, route.nickname)} · ${target} · ${route.source} · ${status}`;
}
