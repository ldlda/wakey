import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import {
  fetchProcessStatus,
  type Agent,
  type FleetProcessStatus,
  type ProcessStatusSample,
} from "@/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";

function useProcessStatus() {
  const [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{
    revision: number;
    status?: FleetProcessStatus;
    failed: boolean;
  }>();

  useEffect(() => {
    const controller = new AbortController();

    void fetchProcessStatus({ signal: controller.signal, cache: "no-store" })
      .then((status) => {
        if (!controller.signal.aborted)
          setResult({ revision, status, failed: false });
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setResult((previous) => ({
            revision,
            status: previous?.status,
            failed: true,
          }));
        }
      });

    return () => controller.abort();
  }, [revision]);

  return {
    status: result?.status,
    failed: result?.failed ?? false,
    loading: result?.revision !== revision,
    refresh: () => setRevision((current) => current + 1),
  };
}

function uptime(seconds: number | null): string {
  if (seconds === null) return "-";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return `${Math.floor(seconds)}s`;
  const hours = Math.floor(minutes / 60);
  if (hours < 1) return `${minutes}m`;
  const days = Math.floor(hours / 24);
  return days ? `${days}d ${hours % 24}h` : `${hours}h ${minutes % 60}m`;
}

function memory(bytes: number | null): string {
  if (bytes === null) return "-";
  return bytes < 1024 ** 3
    ? `${(bytes / 1024 ** 2).toFixed(1)} MiB`
    : `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}

type ProcessNote = "Disconnected" | "Stale" | "Unavailable" | "Loading";

function ProcessRow({
  name,
  note,
  sample,
}: {
  name: string;
  note?: ProcessNote;
  sample?: ProcessStatusSample | null;
}) {
  const values = sample?.values;
  const sampledAt = sample && new Date(sample.received_at_unix * 1000);
  return (
    <li className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 px-3 py-2 text-sm sm:px-4">
      <div className="flex min-w-0 items-center gap-2">
        <span className="min-w-0 truncate font-medium" title={name}>
          {name}
        </span>
        {note && (
          <Badge variant="outline" className="shrink-0">
            {note}
          </Badge>
        )}
      </div>
      {values && sampledAt && (
        <div
          className="grid grid-cols-[3.5rem_4.5rem] gap-2 text-right text-xs tabular-nums text-muted-foreground"
          title={`Sample received ${sampledAt.toLocaleString()}`}
        >
          <span>
            <span className="sr-only">Uptime: </span>
            {uptime(values.process_uptime_seconds)}
          </span>
          <span>
            <span className="sr-only">RSS: </span>
            {memory(values.resident_memory_bytes)}
          </span>
        </div>
      )}
    </li>
  );
}

export function ProcessStatusPanel({ agents }: { agents: Agent[] }) {
  const { status, failed, loading, refresh } = useProcessStatus();
  const emptyNote = loading ? "Loading" : "Unavailable";
  const control = status?.control_plane;
  return (
    <section
      className="col-span-full min-w-0 rounded-md border"
      aria-labelledby="process-status-title"
    >
      <div className="flex items-center justify-between gap-3 border-b px-3 py-1.5 sm:px-4">
        <h2 id="process-status-title" className="text-sm font-semibold">
          Processes
        </h2>
        <div className="flex items-center gap-2">
          {failed && (
            <span role="status" className="text-xs text-muted-foreground">
              Refresh failed
            </span>
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            onClick={refresh}
            disabled={loading}
            title="Refresh process status"
            aria-label="Refresh process status"
          >
            <RefreshCw className="size-3.5" />
          </Button>
        </div>
      </div>
      <ul className="divide-y">
        <ProcessRow
          name="Control plane"
          sample={control}
          note={control ? (failed ? "Stale" : undefined) : emptyNote}
        />
        {agents.map((agent) => {
          const entry = status?.agents[agent.agent_id];
          const connected = entry?.connected ?? agent.connected;
          let note: ProcessNote | undefined;
          if (!connected) note = "Disconnected";
          else if (!entry?.sample) note = emptyNote;
          else if (entry.stale || failed) note = "Stale";
          return (
            <ProcessRow
              key={agent.agent_id}
              name={agent.nickname || agent.agent_id}
              sample={entry?.sample}
              note={note}
            />
          );
        })}
      </ul>
    </section>
  );
}
