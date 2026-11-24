import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Activity, Bell, Bot, Key, ArrowRight } from "lucide-react";

import {
  type Agent,
  type Alert,
  type AlertTransition,
  type EnrollTokenStatus,
  fetchEnrollTokens,
} from "@/api";
import { RecentActivityFeed } from "./dashboard/RecentActivityFeed";
import { ProcessStatusPanel } from "./dashboard/ProcessStatusPanel";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";

type Props = {
  agents: Agent[];
  alerts: Alert[];
  transitions: AlertTransition[];
  loading: boolean;
  alertsHealth: { available: boolean; reason?: string } | null;
  alertsStreamConnected: boolean;
};

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

export function DashboardPage({
  agents,
  alerts,
  transitions,
  loading,
  alertsHealth,
  alertsStreamConnected,
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
          {(!alertsStreamConnected || alertsHealth?.available === false) && (
            <div
              aria-live="polite"
              className="mb-2 text-xs text-muted-foreground"
            >
              {!alertsStreamConnected && <div>Alert stream disconnected</div>}
              {alertsHealth?.available === false && (
                <div>
                  Alert results may be stale or unavailable
                  {alertsHealth.reason ? `: ${alertsHealth.reason}` : ""}
                </div>
              )}
            </div>
          )}
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

      <RecentActivityFeed agents={agents} />
      <ProcessStatusPanel agents={agents} />
    </section>
  );
}
