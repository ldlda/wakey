import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Activity, Bell, Bot, Key, RefreshCw, ArrowRight } from "lucide-react";

import {
  type Agent,
  type Alert,
  type AlertTransition,
  type AuditEvent,
  type EnrollTokenStatus,
  fetchEnrollTokens,
} from "@/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";

type Props = {
  agents: Agent[];
  alerts: Alert[];
  transitions: AlertTransition[];
  audit: AuditEvent[];
  loading: boolean;
  onRefresh: () => void;
};

function relativeTime(unixMs: number): string {
  const diff = Date.now() - unixMs;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

function severityColor(severity: string): string {
  switch (severity.toLowerCase()) {
    case "critical":
      return "bg-red-500/15 text-red-600 dark:text-red-400 border-red-500/30";
    case "warning":
      return "bg-amber-500/15 text-amber-600 dark:text-amber-400 border-amber-500/30";
    case "info":
      return "bg-blue-500/15 text-blue-600 dark:text-blue-400 border-blue-500/30";
    default:
      return "";
  }
}

function eventIcon(eventType: string): string {
  if (eventType.includes("wake")) return "⚡";
  if (eventType.includes("enroll")) return "🔑";
  if (eventType.includes("command")) return "▶";
  if (eventType.includes("connect")) return "🔗";
  if (eventType.includes("disconnect")) return "⛓️‍💥";
  return "•";
}

export function DashboardPage({
  agents,
  alerts,
  transitions,
  audit,
  loading,
  onRefresh,
}: Props) {
  const connected = agents.filter((a) => a.connected).length;
  const total = agents.length;

  const severityBuckets = alerts.reduce<Record<string, number>>((acc, a) => {
    acc[a.severity] = (acc[a.severity] || 0) + 1;
    return acc;
  }, {});

  const [tokens, setTokens] = useState<EnrollTokenStatus[]>([]);
  useEffect(() => {
    void fetchEnrollTokens()
      .then(setTokens)
      .catch(() => undefined);
  }, []);
  const activeTokens = tokens.filter((t) => !t.expired).length;

  const recentAudit = audit.slice(0, 8);

  if (loading && !agents.length) {
    return (
      <section className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <Card key={i}>
            <CardHeader className="pb-2">
              <Skeleton className="h-4 w-20" />
              <Skeleton className="mt-2 h-8 w-12" />
            </CardHeader>
          </Card>
        ))}
        <Card className="sm:col-span-2 lg:col-span-4">
          <CardHeader>
            <Skeleton className="h-5 w-32" />
          </CardHeader>
          <CardContent className="space-y-3">
            {Array.from({ length: 4 }).map((_, i) => (
              <Skeleton key={i} className="h-12 w-full" />
            ))}
          </CardContent>
        </Card>
      </section>
    );
  }

  return (
    <section className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {/* Fleet Status */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-y-0 pb-2">
          <CardDescription className="flex items-center gap-1.5">
            <Bot className="size-3.5" />
            Fleet Status
          </CardDescription>
          <Link
            to="/agents"
            className="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowRight className="size-3.5" />
          </Link>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold tabular-nums">
            {connected}
            <span className="text-base font-normal text-muted-foreground">
              /{total}
            </span>
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            {connected === total
              ? "All agents connected"
              : `${total - connected} offline`}
          </p>
          {total > 0 && (
            <div className="mt-2 flex h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-emerald-500 transition-all"
                style={{ width: `${(connected / total) * 100}%` }}
              />
            </div>
          )}
        </CardContent>
      </Card>

      {/* Active Alerts */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-y-0 pb-2">
          <CardDescription className="flex items-center gap-1.5">
            <Bell className="size-3.5" />
            Active Alerts
          </CardDescription>
          <Link
            to="/alerts"
            className="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowRight className="size-3.5" />
          </Link>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold tabular-nums">{alerts.length}</div>
          {Object.keys(severityBuckets).length > 0 ? (
            <div className="mt-2 flex flex-wrap gap-1.5">
              {Object.entries(severityBuckets)
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([sev, count]) => (
                  <Badge
                    key={sev}
                    variant="outline"
                    className={`text-[0.65rem] ${severityColor(sev)}`}
                  >
                    {count} {sev}
                  </Badge>
                ))}
            </div>
          ) : (
            <p className="mt-1 text-xs text-muted-foreground">All clear</p>
          )}
        </CardContent>
      </Card>

      {/* Transitions */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-y-0 pb-2">
          <CardDescription className="flex items-center gap-1.5">
            <Activity className="size-3.5" />
            Transitions
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold tabular-nums">
            {transitions.length}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            Recent state changes
          </p>
        </CardContent>
      </Card>

      {/* Token Health */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-y-0 pb-2">
          <CardDescription className="flex items-center gap-1.5">
            <Key className="size-3.5" />
            Enroll Tokens
          </CardDescription>
          <Link
            to="/tokens"
            className="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            <ArrowRight className="size-3.5" />
          </Link>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold tabular-nums">{activeTokens}</div>
          <p className="mt-1 text-xs text-muted-foreground">
            {activeTokens === 1 ? "active token" : "active tokens"}
          </p>
        </CardContent>
      </Card>

      {/* Recent Activity Feed */}
      <Card className="sm:col-span-2 lg:col-span-4">
        <CardHeader className="flex flex-row items-start justify-between gap-3 gap-y-0">
          <div>
            <CardTitle>Recent Activity</CardTitle>
            <CardDescription className="mt-1">
              Latest audit events across the fleet
            </CardDescription>
          </div>
          <Button
            onClick={onRefresh}
            disabled={loading}
            variant="outline"
            size="sm"
            className="gap-1.5"
          >
            <RefreshCw
              className={`size-3.5 ${loading ? "animate-spin" : ""}`}
            />
            Refresh
          </Button>
        </CardHeader>
        <CardContent>
          {recentAudit.length > 0 ? (
            <div className="grid gap-0">
              {recentAudit.map((event, i) => (
                <div
                  key={event.event_id}
                  className={`flex items-start gap-3 py-3 ${
                    i < recentAudit.length - 1
                      ? "border-b border-border/50"
                      : ""
                  }`}
                >
                  <span
                    className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-sm"
                    aria-hidden
                  >
                    {eventIcon(event.event_type)}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium">
                        {event.event_type.replace(/_/g, " ")}
                      </span>
                      <Badge
                        variant={
                          event.outcome === "success"
                            ? "secondary"
                            : "destructive"
                        }
                        className="text-[0.6rem] px-1.5 py-0"
                      >
                        {event.outcome}
                      </Badge>
                    </div>
                    <p className="mt-0.5 truncate text-xs text-muted-foreground">
                      {event.message}
                    </p>
                  </div>
                  <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                    {relativeTime(event.ts_unix * 1000)}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center py-8 text-center">
              <Activity className="size-8 text-muted-foreground/40" />
              <p className="mt-2 text-sm text-muted-foreground">
                No recent activity
              </p>
            </div>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
