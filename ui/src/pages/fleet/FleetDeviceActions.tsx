import { useState } from "react";
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
  const [pending, setPending] = useState(false);
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

  async function ping() {
    if (!canPing || !route?.ip || pending) return;
    setPending(true);
    setResult(undefined);
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
      setPending(false);
    }
  }

  const currentResult =
    result?.routeId === route?.route_id ? result : undefined;
  const PingIcon = pending ? LoaderCircle : Activity;

  return (
    <section className="grid gap-2" aria-label="Device actions">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium">Agent route</h3>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => void ping()}
            disabled={busy || pending || !canPing}
          >
            <PingIcon
              className={pending ? "motion-safe:animate-spin" : undefined}
              aria-hidden
            />
            Ping
          </Button>
          <Button
            size="sm"
            onClick={() => route && onWake(route.route_id)}
            disabled={busy || waking || !route?.wakeable}
          >
            <Zap aria-hidden />
            {waking ? "Waking" : "Wake"}
          </Button>
        </div>
      </div>
      <Select
        value={route?.route_id ?? ""}
        onValueChange={(value) => setRouteId(value ?? "")}
      >
        <SelectTrigger
          aria-label="Agent route"
          disabled={!route || pending}
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
        className={`h-10 overflow-y-auto wrap-break-word text-sm ${currentResult?.failed ? "text-destructive" : "text-muted-foreground"}`}
      >
        {pending ? "Waiting for reply…" : currentResult?.text}
      </p>
    </section>
  );
}
