import type { Agent, Alert, AlertTransition } from "@/api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

type Props = {
  agents: Agent[];
  alerts: Alert[];
  transitions: AlertTransition[];
  loading: boolean;
  onRefresh: () => void;
};

export function DashboardPage({
  agents,
  alerts,
  transitions,
  loading,
  onRefresh,
}: Props) {
  const connected = agents.filter((a) => a.connected).length;
  return (
    <section className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <Card>
        <CardHeader className="pb-2">
          <CardDescription>Agents</CardDescription>
          <CardTitle className="text-2xl">{agents.length}</CardTitle>
        </CardHeader>
      </Card>
      <Card>
        <CardHeader className="pb-2">
          <CardDescription>Connected</CardDescription>
          <CardTitle className="text-2xl">{connected}</CardTitle>
        </CardHeader>
      </Card>
      <Card>
        <CardHeader className="pb-2">
          <CardDescription>Active Alerts</CardDescription>
          <CardTitle className="text-2xl">{alerts.length}</CardTitle>
        </CardHeader>
      </Card>
      <Card>
        <CardHeader className="pb-2">
          <CardDescription>Transitions</CardDescription>
          <CardTitle className="text-2xl">{transitions.length}</CardTitle>
        </CardHeader>
      </Card>
      <Card className="sm:col-span-2 lg:col-span-4">
        <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
          <CardTitle>Overview</CardTitle>
          <Button
            onClick={onRefresh}
            disabled={loading}
            variant="outline"
            size="sm"
          >
            {loading ? "Refreshing..." : "Refresh"}
          </Button>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            Use tabs to run commands, inspect audits, and monitor alerts in real
            time.
          </p>
        </CardContent>
      </Card>
    </section>
  );
}
