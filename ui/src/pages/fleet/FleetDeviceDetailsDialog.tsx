import { useEffect, useState } from "react";
import { Zap } from "lucide-react";

import {
  attachDeviceIdentifier,
  createKnownDevice,
  mergeKnownDevice,
  type FleetDevice,
  type KnownDevice,
} from "@/api";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { DetailBlock, PresenceBadge } from "@/pages/fleet/FleetComponents";
import { agentLabel, identifiersFor, routeLabel } from "@/pages/fleet/utils";

type Props = {
  device: FleetDevice | null;
  knownDevices: KnownDevice[];
  open: boolean;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onWake: (routeId?: string | null) => void;
  onCopy: (label: string, value: string) => void;
  onChanged: () => void;
};

export function FleetDeviceDetailsDialog({
  device,
  knownDevices,
  open,
  busy,
  onOpenChange,
  onWake,
  onCopy,
  onChanged,
}: Props) {
  const [displayName, setDisplayName] = useState("");
  const [targetDeviceId, setTargetDeviceId] = useState("");
  const [routeId, setRouteId] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    setDisplayName(device?.display_name ?? "");
    setTargetDeviceId("");
    setRouteId(device?.recommended_route?.route_id ?? "");
    setError("");
  }, [device]);

  if (!device) return null;

  const currentDevice = device;
  const identifiers = identifiersFor(currentDevice);
  const otherKnownDevices = knownDevices.filter(
    (known) => known.device_id !== currentDevice.known_device?.device_id,
  );

  async function createRemembered() {
    setError("");
    try {
      await createKnownDevice({
        display_name: displayName.trim() || currentDevice.display_name,
        pinned: true,
        identifiers,
      });
      onChanged();
    } catch (err) {
      setError(String(err));
    }
  }

  async function attachToExisting() {
    if (!targetDeviceId) return;
    setError("");
    try {
      for (const identifier of identifiers) {
        await attachDeviceIdentifier(targetDeviceId, identifier);
      }
      onChanged();
    } catch (err) {
      setError(String(err));
    }
  }

  async function mergeIntoExisting() {
    if (!targetDeviceId || !currentDevice.known_device) return;
    setError("");
    try {
      await mergeKnownDevice(
        targetDeviceId,
        currentDevice.known_device.device_id,
      );
      onChanged();
      onOpenChange(false);
    } catch (err) {
      setError(String(err));
    }
  }

  const selectedRoute =
    device.route_candidates.find((route) => route.route_id === routeId) ??
    device.recommended_route;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[92vh] overflow-auto sm:max-w-3xl">
        <DialogHeader>
          <div className="flex min-w-0 items-start justify-between gap-3 pr-8">
            <div className="min-w-0">
              <DialogTitle className="truncate">
                {device.display_name}
              </DialogTitle>
              <DialogDescription>
                {device.known_device
                  ? "Remembered fleet device"
                  : "Observed fleet device"}
              </DialogDescription>
            </div>
            <PresenceBadge presence={device.presence} />
          </div>
        </DialogHeader>

        {error && (
          <pre className="max-h-40 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
            {error}
          </pre>
        )}

        <div className="grid gap-3 md:grid-cols-2">
          <DetailBlock label="IPs" values={device.ips} onCopy={onCopy} />
          <DetailBlock label="MACs" values={device.macs} onCopy={onCopy} />
          <DetailBlock
            label="Hostnames"
            values={device.hostnames}
            onCopy={onCopy}
          />
          <DetailBlock
            label="Agents"
            values={device.agents.map((agent) =>
              agentLabel(agent.agent_id, agent.nickname),
            )}
            onCopy={onCopy}
          />
        </div>

        <Separator />

        <div className="grid gap-2">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-sm font-medium">Wake route</h3>
            <Button
              size="sm"
              onClick={() => onWake(selectedRoute?.route_id ?? null)}
              disabled={busy || !selectedRoute?.wakeable}
            >
              <Zap className="size-4" aria-hidden />
              {busy ? "Waking" : "Wake"}
            </Button>
          </div>
          <Select
            value={routeId}
            onValueChange={(value) => setRouteId(value ?? "")}
          >
            <SelectTrigger>
              <span>
                {selectedRoute
                  ? routeLabel(selectedRoute)
                  : "No connected MAC-backed route"}
              </span>
            </SelectTrigger>
            <SelectContent alignItemWithTrigger={false}>
              {device.route_candidates.map((route) => (
                <SelectItem key={route.route_id} value={route.route_id}>
                  {routeLabel(route)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <Separator />

        <div className="grid gap-3">
          <h3 className="text-sm font-medium">Remember / merge</h3>
          {!device.known_device && (
            <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
              <Input
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder="Device name"
              />
              <Button
                onClick={() => void createRemembered()}
                disabled={!identifiers.length}
              >
                Remember device
              </Button>
            </div>
          )}
          <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto_auto]">
            <Select
              value={targetDeviceId}
              onValueChange={(value) => setTargetDeviceId(value ?? "")}
            >
              <SelectTrigger>
                <span>
                  {targetDeviceId
                    ? knownDevices.find(
                        (known) => known.device_id === targetDeviceId,
                      )?.display_name
                    : "Select known device"}
                </span>
              </SelectTrigger>
              <SelectContent
                className="max-w-[min(92vw,30rem)]"
                alignItemWithTrigger={false}
              >
                {otherKnownDevices.map((known) => (
                  <SelectItem key={known.device_id} value={known.device_id}>
                    {known.display_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              onClick={() => void attachToExisting()}
              disabled={!targetDeviceId || !identifiers.length}
            >
              Attach IDs
            </Button>
            <Button
              variant="outline"
              onClick={() => void mergeIntoExisting()}
              disabled={!targetDeviceId || !device.known_device}
            >
              Merge
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            Proposed identifiers:{" "}
            {identifiers.map((id) => `${id.kind}:${id.value}`).join(", ") ||
              "-"}
          </p>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
