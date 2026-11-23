import { useEffect, useState } from "react";
import { Activity, LoaderCircle, Zap } from "lucide-react";
import { pingDevice, type Agent, type FleetDevice } from "@/api";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { routeLabel } from "./utils";

const PING_FEEDBACK_DELAY_MS = 150;

export function FleetDeviceActions({
  device,
  agents,
  busy,
  waking,
  onWake,
}: {
  device: FleetDevice;
  agents: Agent[];
  busy: boolean;
  waking: boolean;
  onWake: (routeId: string) => void;
}) {
  const [routeId, setRouteId] = useState("");
  const [pendingRouteId, setPendingRouteId] = useState<string>();
  const [showWaiting, setShowWaiting] = useState(false);
  const [result, setResult] = useState<{
    routeId: string;
    text: string;
    failed: boolean;
  }>();
  const route =
    device.route_candidates.find(
      (candidate) => candidate.route_id === routeId,
    ) ??
    device.recommended_route ??
    device.route_candidates.find(
      (candidate) => candidate.connected && candidate.ip,
    ) ??
    device.route_candidates[0];
  const canPing =
    route?.connected &&
    route.ip &&
    agents.some(
      (agent) =>
        agent.agent_id === route.agent_id &&
        agent.capabilities.includes("ping"),
    );

  const pending = pendingRouteId !== undefined;
  useEffect(() => {
    if (!pending) return;
    // Fast replies can go straight to the result without a spinner flash.
    const timer = window.setTimeout(
      () => setShowWaiting(true),
      PING_FEEDBACK_DELAY_MS,
    );
    return () => window.clearTimeout(timer);
  }, [pending]);

  async function ping() {
    if (busy || !canPing || !route?.ip || pendingRouteId !== undefined) return;
    setPendingRouteId(route.route_id);
    try {
      const reply = await pingDevice(route.agent_id, route.ip, route.interface);
      const rtt =
        reply.rtt_ms == null ? "" : ` · ${reply.rtt_ms.toFixed(2)} ms`;
      setResult({
        routeId: route.route_id,
        text: reply.replied
          ? `Reply from ${route.ip}${rtt}`
          : `No reply from ${route.ip}`,
        failed: false,
      });
    } catch (error) {
      setResult({ routeId: route.route_id, text: String(error), failed: true });
    } finally {
      setShowWaiting(false);
      setPendingRouteId(undefined);
    }
  }

  const currentResult =
    result?.routeId === route?.route_id ? result : undefined;
  const waitingForRoute = showWaiting && pendingRouteId === route?.route_id;
  const PingIcon = showWaiting ? LoaderCircle : Activity;

  return (
    <section className="grid gap-2" aria-label="Device actions">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium">Agent route</h3>
        <div className="grid grid-cols-[repeat(2,5rem)] gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => void ping()}
            disabled={busy || !canPing}
            aria-disabled={pending || undefined}
          >
            <PingIcon
              className={showWaiting ? "motion-safe:animate-spin" : undefined}
              aria-hidden
            />
            Ping
          </Button>
          <Button
            size="sm"
            onClick={() => {
              if (route?.wakeable && !busy && !waking) onWake(route.route_id);
            }}
            disabled={busy || !route?.wakeable}
            aria-disabled={waking || undefined}
            aria-busy={waking || undefined}
          >
            <Zap aria-hidden />
            Wake
          </Button>
        </div>
      </div>
      <Select
        value={route?.route_id ?? ""}
        onValueChange={(value) => setRouteId(value ?? "")}
      >
        <SelectTrigger
          aria-label="Agent route"
          disabled={!route}
          className="w-full min-w-0"
        >
          <span className="truncate">
            {route ? routeLabel(route) : "No routes available"}
          </span>
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          {device.route_candidates.map((candidate) => (
            <SelectItem
              key={candidate.route_id}
              value={candidate.route_id}
              className="break-all"
            >
              {routeLabel(candidate)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p
        role="status"
        className={`min-h-4 wrap-break-word text-xs leading-4 ${!waitingForRoute && currentResult?.failed ? "text-destructive" : "text-muted-foreground"}`}
      >
        {waitingForRoute ? "Waiting for reply…" : currentResult?.text}
      </p>
    </section>
  );
}
