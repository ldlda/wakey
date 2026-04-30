import { useEffect, useMemo, useState } from "react";
import {
  Check,
  Inbox,
  MoreHorizontal,
  RefreshCw,
  Search,
  Zap,
} from "lucide-react";
import { toast } from "sonner";

import {
  fetchFleetDevices,
  fetchKnownDevices,
  refreshFleetDevices,
  wakeFleetDevice,
  type Agent,
  type FleetDevice,
  type KnownDevice,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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
import { Skeleton } from "@/components/ui/skeleton";
import {
  FilterSelect,
  MobileLabel,
  PresenceBadge,
} from "@/pages/fleet/FleetComponents";
import { FleetDeviceDetailsDialog } from "@/pages/fleet/FleetDeviceDetailsDialog";
import {
  agentLabel,
  formatSeen,
  knownFilters,
  presenceFilters,
  presenceRank,
  summarize,
  type KnownFilter,
  type SortDir,
  type SortKey,
} from "@/pages/fleet/utils";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onAfterWake?: () => Promise<void>;
};

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
      const msg = failed.length
        ? `Stored ${result.total_accepted} observations; ${failed.length} agent refresh failed`
        : `Stored ${result.total_accepted} observations from ${result.agents.length} connected agents`;
      setStatus(msg);
      toast.success(msg);
      await loadFleet();
    } catch (err) {
      setError(String(err));
      toast.error("Fleet refresh failed", { description: String(err) });
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
      const msg = `Wake sent via ${agentLabel(result.route.agent_id, result.route.nickname)} (${result.route.mac ?? "no mac"})`;
      toast.success(msg);
      setStatus(msg);
      if (onAfterWake) await onAfterWake();
      await loadFleet();
    } catch (err) {
      setError(String(err));
      toast.error("Wake failed", { description: String(err) });
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
      toast.success(`Copied ${label}`);
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
      toast.success(`Copied ${label}`);
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

          {/* Skeleton loading */}
          {loading && !devices.length ? (
            <div className="grid gap-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-14 w-full rounded-md" />
              ))}
            </div>
          ) : (
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
                <div className="flex flex-col items-center justify-center rounded-md border bg-card py-12 text-center">
                  <Inbox className="size-10 text-muted-foreground/30" />
                  <p className="mt-3 text-sm text-muted-foreground">
                    No devices match the current filters
                  </p>
                </div>
              )}
            </div>
          )}
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
