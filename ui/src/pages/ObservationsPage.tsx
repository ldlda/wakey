import { useEffect, useMemo, useState } from "react";

import {
  attachObservationIdentifier,
  createKnownDevice,
  fetchKnownDevices,
  fetchObservations,
  type AgentDeviceObservation,
  type KnownDevice,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";

type Filter = "all" | "unknown" | "known";

function observationLabel(observation: AgentDeviceObservation): string {
  return (
    observation.hostname?.trim() ||
    observation.mac?.trim() ||
    observation.ip?.trim() ||
    observation.observation_key
  );
}

function formatSeen(tsUnix: number): string {
  if (!Number.isFinite(tsUnix) || tsUnix <= 0) return "-";
  return new Date(tsUnix * 1000).toLocaleString();
}

function identifierSummary(device: KnownDevice): string {
  if (!device.identifiers.length) return "no identifiers";
  return device.identifiers
    .map((identifier) => `${identifier.kind}:${identifier.value}`)
    .join(", ");
}

export function ObservationsPage() {
  const [observations, setObservations] = useState<AgentDeviceObservation[]>(
    [],
  );
  const [devices, setDevices] = useState<KnownDevice[]>([]);
  const [filter, setFilter] = useState<Filter>("unknown");
  const [query, setQuery] = useState("");
  const [selectedDevices, setSelectedDevices] = useState<
    Record<string, string>
  >({});
  const [newNames, setNewNames] = useState<Record<string, string>>({});
  const [busyKey, setBusyKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");

  async function load() {
    setLoading(true);
    setError("");
    try {
      const [nextObservations, nextDevices] = await Promise.all([
        fetchObservations({ limit: 500 }),
        fetchKnownDevices(),
      ]);
      setObservations(nextObservations);
      setDevices(nextDevices);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return observations.filter((observation) => {
      if (filter === "known" && !observation.known_device) return false;
      if (filter === "unknown" && observation.known_device) return false;
      if (!q) return true;

      const haystack = [
        observation.hostname,
        observation.mac,
        observation.ip,
        observation.agent_id,
        observation.kind,
        observation.last_action,
        observation.known_device?.display_name,
      ]
        .filter((value): value is string => Boolean(value))
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    });
  }, [filter, observations, query]);

  async function attachExisting(observation: AgentDeviceObservation) {
    const deviceId = selectedDevices[observation.observation_key];
    if (!deviceId) return;
    setBusyKey(observation.observation_key);
    setStatus("");
    setError("");
    try {
      const device = await attachObservationIdentifier(
        deviceId,
        observation.observation_key,
      );
      setStatus(
        `Attached ${observationLabel(observation)} to ${device.display_name}`,
      );
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusyKey("");
    }
  }

  async function createAndAttach(observation: AgentDeviceObservation) {
    const displayName =
      newNames[observation.observation_key]?.trim() ||
      observationLabel(observation);
    if (!displayName.trim()) return;
    setBusyKey(observation.observation_key);
    setStatus("");
    setError("");
    try {
      const device = await createKnownDevice({
        display_name: displayName,
        pinned: true,
        identifiers: [],
      });
      const updated = await attachObservationIdentifier(
        device.device_id,
        observation.observation_key,
      );
      setStatus(
        `Created ${updated.display_name} and attached ${observationLabel(observation)}`,
      );
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusyKey("");
    }
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
        <CardTitle>Observed Devices</CardTitle>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void load()}
          disabled={loading}
        >
          {loading ? "Refreshing..." : "Refresh"}
        </Button>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid gap-2 md:grid-cols-[12rem_minmax(0,1fr)]">
          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>Status</span>
            <Select
              value={filter}
              onValueChange={(value: Filter | null) => {
                if (value) setFilter(value);
              }}
            >
              <SelectTrigger className="w-full">
                <span>{filter}</span>
              </SelectTrigger>
              <SelectContent alignItemWithTrigger={false}>
                <SelectItem value="unknown">unknown</SelectItem>
                <SelectItem value="known">known</SelectItem>
                <SelectItem value="all">all</SelectItem>
              </SelectContent>
            </Select>
          </label>
          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>Search</span>
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="hostname, mac, ip, agent, known device"
            />
          </label>
        </div>

        <p className="text-sm text-muted-foreground">
          Showing {filtered.length} of {observations.length}
        </p>
        {status && (
          <pre className="max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 text-xs">
            {status}
          </pre>
        )}
        {error && (
          <pre className="max-h-80 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
            {error}
          </pre>
        )}

        <div className="grid gap-2 overflow-x-auto">
          <div className="grid min-w-[68rem] grid-cols-[minmax(10rem,1.2fr)_minmax(8rem,0.8fr)_minmax(8rem,0.8fr)_minmax(9rem,0.8fr)_minmax(16rem,1.3fr)] gap-2 rounded-md border bg-muted/60 px-3 py-2 text-sm">
            <span>Device</span>
            <span>Network</span>
            <span>Agent</span>
            <span>Seen</span>
            <span>Known Device</span>
          </div>

          {filtered.map((observation) => {
            const busy = busyKey === observation.observation_key;
            const attachable = Boolean(observation.mac || observation.ip);
            return (
              <div
                key={observation.observation_key}
                className="grid min-w-[68rem] grid-cols-[minmax(10rem,1.2fr)_minmax(8rem,0.8fr)_minmax(8rem,0.8fr)_minmax(9rem,0.8fr)_minmax(16rem,1.3fr)] gap-2 rounded-md border bg-card px-3 py-2 text-sm"
              >
                <div className="min-w-0">
                  <div className="truncate font-medium">
                    {observationLabel(observation)}
                  </div>
                  <div className="mt-1 flex flex-wrap gap-1 text-xs text-muted-foreground">
                    <Badge variant="outline">{observation.kind}</Badge>
                    <Badge variant="outline">{observation.last_action}</Badge>
                  </div>
                </div>
                <div className="min-w-0 text-muted-foreground">
                  <div className="truncate" title={observation.ip ?? "-"}>
                    {observation.ip ?? "-"}
                  </div>
                  <div className="truncate" title={observation.mac ?? "-"}>
                    {observation.mac ?? "-"}
                  </div>
                </div>
                <div className="min-w-0 truncate text-muted-foreground">
                  {observation.agent_id}
                </div>
                <div className="min-w-0 text-muted-foreground">
                  <div className="truncate">
                    {formatSeen(observation.last_seen_unix)}
                  </div>
                </div>
                <div className="min-w-0">
                  {observation.known_device ? (
                    <div className="grid gap-1">
                      <div className="flex min-w-0 items-center gap-2">
                        <Badge variant="secondary">known</Badge>
                        <span className="truncate font-medium">
                          {observation.known_device.display_name}
                        </span>
                      </div>
                      <span className="text-xs text-muted-foreground">
                        {observation.known_device.pinned
                          ? "pinned"
                          : "not pinned"}
                      </span>
                    </div>
                  ) : (
                    <div className="grid min-w-0 gap-2">
                      <div className="flex min-w-0 gap-2">
                        <Select
                          value={
                            selectedDevices[observation.observation_key] ?? ""
                          }
                          onValueChange={(value: string | null) => {
                            setSelectedDevices((prev) => ({
                              ...prev,
                              [observation.observation_key]: value ?? "",
                            }));
                          }}
                        >
                          <SelectTrigger className="min-w-0 flex-1">
                            <span className="truncate">
                              {selectedDevices[observation.observation_key]
                                ? devices.find(
                                    (device) =>
                                      device.device_id ===
                                      selectedDevices[
                                        observation.observation_key
                                      ],
                                  )?.display_name
                                : "Attach to existing"}
                            </span>
                          </SelectTrigger>
                          <SelectContent
                            className="max-w-[min(92vw,28rem)]"
                            alignItemWithTrigger={false}
                          >
                            {devices.map((device) => (
                              <SelectItem
                                key={device.device_id}
                                value={device.device_id}
                              >
                                <span className="grid min-w-0 gap-0.5">
                                  <span className="truncate">
                                    {device.display_name}
                                  </span>
                                  <span className="truncate text-xs text-muted-foreground">
                                    {identifierSummary(device)}
                                  </span>
                                </span>
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={
                            busy ||
                            !attachable ||
                            !selectedDevices[observation.observation_key]
                          }
                          onClick={() => void attachExisting(observation)}
                        >
                          Attach
                        </Button>
                      </div>
                      <div className="flex min-w-0 gap-2">
                        <Input
                          value={newNames[observation.observation_key] ?? ""}
                          onChange={(event) =>
                            setNewNames((prev) => ({
                              ...prev,
                              [observation.observation_key]: event.target.value,
                            }))
                          }
                          placeholder={`Create as ${observationLabel(observation)}`}
                          disabled={busy || !attachable}
                        />
                        <Button
                          size="sm"
                          disabled={busy || !attachable}
                          onClick={() => void createAndAttach(observation)}
                        >
                          Create
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            );
          })}

          {!filtered.length && (
            <div className="px-1 py-2 text-sm text-muted-foreground">
              No observations found
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
