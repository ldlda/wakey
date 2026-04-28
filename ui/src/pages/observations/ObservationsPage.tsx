import { useEffect, useMemo, useState } from "react";

import {
  attachObservationIdentifier,
  createKnownDevice,
  fetchKnownDevices,
  fetchObservations,
  type AgentDeviceObservation,
  type KnownDevice,
} from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { ObservationHistoryDialog } from "@/pages/observations/ObservationHistoryDialog";
import { ObservationTable } from "@/pages/observations/ObservationTable";
import {
  observationLabel,
  type ObservationFilter,
} from "@/pages/observations/utils";

export function ObservationsPage() {
  const [observations, setObservations] = useState<AgentDeviceObservation[]>(
    [],
  );
  const [devices, setDevices] = useState<KnownDevice[]>([]);
  const [filter, setFilter] = useState<ObservationFilter>("all");
  const [query, setQuery] = useState("");
  const [selectedDevices, setSelectedDevices] = useState<
    Record<string, string>
  >({});
  const [newNames, setNewNames] = useState<Record<string, string>>({});
  const [busyKey, setBusyKey] = useState("");
  const [historyObservation, setHistoryObservation] =
    useState<AgentDeviceObservation | null>(null);
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
              onValueChange={(value: ObservationFilter | null) => {
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

        <ObservationTable
          observations={filtered}
          devices={devices}
          selectedDevices={selectedDevices}
          newNames={newNames}
          busyKey={busyKey}
          onSelectDevice={(observationKey, deviceId) =>
            setSelectedDevices((prev) => ({
              ...prev,
              [observationKey]: deviceId,
            }))
          }
          onNewNameChange={(observationKey, name) =>
            setNewNames((prev) => ({ ...prev, [observationKey]: name }))
          }
          onAttachExisting={(observation) => void attachExisting(observation)}
          onCreateAndAttach={(observation) => void createAndAttach(observation)}
          onOpenHistory={setHistoryObservation}
        />

        <ObservationHistoryDialog
          observation={historyObservation}
          open={Boolean(historyObservation)}
          onOpenChange={(open) => {
            if (!open) setHistoryObservation(null);
          }}
        />
      </CardContent>
    </Card>
  );
}
