import { useEffect, useState } from "react";
import {
  Activity,
  ArrowDownLeft,
  ArrowUpRight,
  Key,
  Plug,
  RefreshCw,
  Zap,
} from "lucide-react";

import { fetchAudit, type Agent, type AuditEvent } from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { formatRecentTime } from "@/lib/formatRecentTime";

function eventIcon(eventType: string) {
  if (eventType === "command_dispatch") return ArrowUpRight;
  if (eventType === "command_result") return ArrowDownLeft;
  if (eventType.includes("wake")) return Zap;
  if (eventType.includes("enroll")) return Key;
  if (eventType.includes("connect")) return Plug;
  return Activity;
}

function activityRow(event: AuditEvent, agentNames: Map<string, string>) {
  const agent = event.agent_id
    ? (agentNames.get(event.agent_id) ?? event.agent_id)
    : "agent";
  const command = event.metadata.command;
  let title = event.event_type.replace(/_/g, " ");
  if (typeof command === "string") {
    if (event.event_type === "command_dispatch")
      title = `${command} to ${agent}`;
    if (event.event_type === "command_result") {
      const direction = ["ok", "error"].includes(event.outcome) ? "from" : "on";
      title = `${command} ${direction} ${agent}`;
    }
  }
  return {
    id: event.event_id,
    title,
    icon: eventIcon(event.event_type),
    outcome: event.outcome,
    successful: ["success", "ok", "sent"].includes(event.outcome),
    summary:
      event.latency_ms == null
        ? event.message
        : `${event.message} · ${event.latency_ms} ms`,
    time: formatRecentTime(event.ts_unix),
  };
}

function ActivityRow({ row }: { row: ReturnType<typeof activityRow> }) {
  const Icon = row.icon;
  return (
    <li className="flex items-start gap-3 py-3">
      <span
        className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-md bg-muted"
        aria-hidden
      >
        <Icon className="size-4" />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="wrap-break-word text-sm font-medium">
            {row.title}
          </span>
          <Badge variant={row.successful ? "secondary" : "destructive"}>
            {row.outcome}
          </Badge>
        </div>
        <p className="mt-0.5 wrap-break-word text-xs text-muted-foreground">
          {row.summary}
        </p>
      </div>
      <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
        {row.time}
      </span>
    </li>
  );
}

export function RecentActivityFeed({ agents }: { agents: Agent[] }) {
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [refresh, setRefresh] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    // Mount and explicit refresh share one request lifecycle.
    void fetchAudit(8, { signal: controller.signal })
      .then((events) => {
        if (!controller.signal.aborted) setEvents(events);
      })
      .catch(() => {
        if (!controller.signal.aborted) setError(true);
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [refresh]);

  function reload() {
    setLoading(true);
    setError(false);
    setRefresh((count) => count + 1);
  }

  const agentNames = new Map(
    agents.map((agent) => [
      agent.agent_id,
      agent.nickname?.trim() || agent.agent_id,
    ]),
  );
  const rows = events.map((event) => activityRow(event, agentNames));
  const status = error
    ? "Could not load recent activity"
    : loading
      ? "Loading activity"
      : "No recent activity";

  return (
    <Card className="sm:col-span-2 lg:col-span-4">
      <CardHeader className="flex flex-row items-center justify-between gap-3">
        <CardTitle>Recent Activity</CardTitle>
        <Button onClick={reload} disabled={loading} variant="outline" size="sm">
          <RefreshCw
            className={loading ? "motion-safe:animate-spin" : undefined}
            aria-hidden
          />
          Refresh
        </Button>
      </CardHeader>
      <CardContent>
        {(error || !rows.length) && (
          <p role="status" className="py-3 text-sm text-muted-foreground">
            {status}
          </p>
        )}
        <ul className="divide-y divide-border/50">
          {rows.map((row) => (
            <ActivityRow key={row.id} row={row} />
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
