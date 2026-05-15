import { useMemo, useState } from "react";

import type { Alert, AlertTransition } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

type Props = {
  alerts: Alert[];
  transitions: AlertTransition[];
  onRefresh: () => Promise<void>;
};

export function AlertsPage({ alerts, transitions, onRefresh }: Props) {
  const [severity, setSeverity] = useState("all");
  const [status, setStatus] = useState("all");
  const [kind, setKind] = useState("all");
  const [transitionQ, setTransitionQ] = useState("");

  const severities = useMemo(
    () => ["all", ...Array.from(new Set(alerts.map((a) => a.severity))).sort()],
    [alerts],
  );
  const statuses = useMemo(
    () => ["all", ...Array.from(new Set(alerts.map((a) => a.status))).sort()],
    [alerts],
  );
  const kinds = useMemo(
    () => ["all", ...Array.from(new Set(alerts.map((a) => a.kind))).sort()],
    [alerts],
  );

  const filteredAlerts = useMemo(
    () =>
      alerts.filter(
        (a) =>
          (severity === "all" || a.severity === severity) &&
          (status === "all" || a.status === status) &&
          (kind === "all" || a.kind === kind),
      ),
    [alerts, severity, status, kind],
  );

  const filteredTransitions = useMemo(() => {
    const q = transitionQ.trim().toLowerCase();
    if (!q) return transitions;
    return transitions.filter(
      (t) =>
        t.kind.toLowerCase().includes(q) ||
        t.to_status.toLowerCase().includes(q) ||
        t.message.toLowerCase().includes(q) ||
        (t.agent_id || "").toLowerCase().includes(q),
    );
  }, [transitions, transitionQ]);

  return (
    <section className="grid gap-3 lg:grid-cols-2">
      <Card>
        <CardHeader className="flex items-center justify-between gap-2">
          <CardTitle>Active Alerts</CardTitle>
          <Button size="sm" variant="outline" onClick={() => void onRefresh()}>
            Refresh
          </Button>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-2 sm:grid-cols-3">
            <label
              htmlFor="alert-severity"
              className="grid gap-1 text-sm text-muted-foreground"
            >
              Severity
              <Select
                value={severity}
                onValueChange={(value) => {
                  if (value) setSeverity(value);
                }}
              >
                <SelectTrigger id="alert-severity" className="w-full">
                  <SelectValue placeholder="Severity" />
                </SelectTrigger>
                <SelectContent>
                  {severities.map((v) => (
                    <SelectItem key={v} value={v}>
                      {v}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>

            <label
              htmlFor="alert-status"
              className="grid gap-1 text-sm text-muted-foreground"
            >
              Status
              <Select
                value={status}
                onValueChange={(value) => {
                  if (value) setStatus(value);
                }}
              >
                <SelectTrigger id="alert-status" className="w-full">
                  <SelectValue placeholder="Status" />
                </SelectTrigger>
                <SelectContent>
                  {statuses.map((v) => (
                    <SelectItem key={v} value={v}>
                      {v}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>

            <label
              htmlFor="alert-kind"
              className="grid gap-1 text-sm text-muted-foreground"
            >
              Kind
              <Select
                value={kind}
                onValueChange={(value) => {
                  if (value) setKind(value);
                }}
              >
                <SelectTrigger id="alert-kind" className="w-full">
                  <SelectValue placeholder="Kind" />
                </SelectTrigger>
                <SelectContent>
                  {kinds.map((v) => (
                    <SelectItem key={v} value={v}>
                      {v}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
          </div>

          <p className="text-sm text-muted-foreground">
            Showing {filteredAlerts.length} of {alerts.length}
          </p>

          <div className="grid gap-2">
            {filteredAlerts.map((alert) => (
              <div
                className="flex items-center justify-between gap-2 rounded-lg border border-border bg-background px-3 py-2 text-sm"
                key={alert.alert_id}
              >
                <span>
                  {alert.kind}
                  {alert.agent_id ? ` (${alert.agent_id})` : ""}
                </span>
                <span className="text-muted-foreground">{alert.severity}</span>
              </div>
            ))}
            {!filteredAlerts.length && (
              <div className="px-1 text-sm text-muted-foreground">
                No active alerts
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex items-center justify-between gap-2">
          <CardTitle>Transition History</CardTitle>
          <span className="text-sm text-muted-foreground">
            {filteredTransitions.length} shown
          </span>
        </CardHeader>
        <CardContent className="space-y-2">
          <label
            htmlFor="alert-transition-search"
            className="grid gap-1 text-sm text-muted-foreground"
          >
            Search
            <Input
              id="alert-transition-search"
              value={transitionQ}
              onChange={(e) => setTransitionQ(e.target.value)}
              placeholder="kind, status, message, agent"
            />
          </label>

          <Textarea
            className="min-h-57.5 font-mono text-xs"
            readOnly
            value={JSON.stringify(filteredTransitions, null, 2)}
          />
        </CardContent>
      </Card>
    </section>
  );
}
