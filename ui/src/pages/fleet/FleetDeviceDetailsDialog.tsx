import { useMemo, useState } from "react";
import { Minus, Plus, Zap } from "lucide-react";
import { toast } from "sonner";

import {
  attachDeviceIdentifier,
  createKnownDevice,
  detachDeviceIdentifier,
  mergeKnownDevice,
  type FleetDevice,
  type KnownDevice,
} from "@/api";
import { Badge } from "@/components/ui/badge";
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
  const [displayName, setDisplayName] = useState(device?.display_name ?? "");
  const [targetDeviceId, setTargetDeviceId] = useState("");
  const [routeId, setRouteId] = useState(device?.recommended_route?.route_id ?? "");
  const [addKind, setAddKind] = useState<"mac" | "ip">("mac");
  const [addValue, setAddValue] = useState("");
  const [error, setError] = useState("");
  const [actionBusy, setActionBusy] = useState(false);

  const rowIdentifiers = device ? identifiersFor(device) : [];

  // Resolve the full KnownDevice object if this fleet row is known
  const fullKnown = knownDevices.find(
    (kd) => kd.device_id === device?.known_device?.device_id,
  );

  // Identifiers from the row that are NOT already on the known device
  const unattachedIdentifiers = useMemo(() => {
    if (!fullKnown) return rowIdentifiers;
    const existing = new Set(
      fullKnown.identifiers.map((id) => `${id.kind}:${id.value}`),
    );
    return rowIdentifiers.filter(
      (id) => !existing.has(`${id.kind}:${id.value}`),
    );
  }, [fullKnown, rowIdentifiers]);

  if (!device) return null;

  const currentDevice = device;

  const otherKnownDevices = knownDevices.filter(
    (kd) => kd.device_id !== currentDevice.known_device?.device_id,
  );

  async function wrap(fn: () => Promise<void | { changed?: boolean }>) {
    setError("");
    setActionBusy(true);
    try {
      const res = await fn();
      if (!res || res.changed !== false) {
        onChanged();
      }
    } catch (err: any) {
      setError(String(err));
      if (err && (err.changed || err.partial)) {
        onChanged();
      }
    } finally {
      setActionBusy(false);
    }
  }

  async function handleCreateRemembered() {
    await wrap(async () => {
      const result = await createKnownDevice({
        display_name: displayName.trim() || currentDevice.display_name,
        pinned: true,
        identifiers: rowIdentifiers,
      });
      toast.success(`Remembered as "${result.display_name}"`);
    });
  }

  async function handleAttachToExisting() {
    if (!targetDeviceId) return;
    const target = knownDevices.find((kd) => kd.device_id === targetDeviceId);
    await wrap(async () => {
      const ids = fullKnown ? unattachedIdentifiers : rowIdentifiers;
      if (ids.length === 0) return;

      const results = await Promise.allSettled(
        ids.map((id) => attachDeviceIdentifier(targetDeviceId, id)),
      );

      const changed = results.some((r) => r.status === "fulfilled");
      const errors = results.filter((r) => r.status === "rejected");

      if (errors.length) {
        const err = errors[0].reason as Error;
        if (changed) (err as any).changed = true;
        throw err;
      }

      toast.success(
        `Attached ${ids.length} identifier(s) to "${target?.display_name ?? targetDeviceId}"`,
      );
      return { changed };
    });
  }

  async function handleMerge() {
    if (!targetDeviceId || !currentDevice.known_device) return;
    const target = knownDevices.find((kd) => kd.device_id === targetDeviceId);
    await wrap(async () => {
      await mergeKnownDevice(
        targetDeviceId,
        currentDevice.known_device!.device_id,
      );
      toast.success(`Merged into "${target?.display_name ?? targetDeviceId}"`);
      onOpenChange(false);
    });
  }

  async function handleAddIdentifier() {
    if (!fullKnown || !addValue.trim()) return;
    await wrap(async () => {
      await attachDeviceIdentifier(fullKnown!.device_id, {
        kind: addKind,
        value: addValue.trim(),
      });
      setAddValue("");
      toast.success(`Added ${addKind} identifier`);
    });
  }

  async function handleDetachIdentifier(identifierKey: string) {
    if (!fullKnown) return;
    await wrap(async () => {
      await detachDeviceIdentifier(fullKnown!.device_id, identifierKey);
      toast.success("Identifier removed");
    });
  }

  const selectedRoute =
    device.route_candidates.find((r) => r.route_id === routeId) ??
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
                  : "Unidentified fleet device"}
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
            values={device.agents.map((a) =>
              agentLabel(a.agent_id, a.nickname),
            )}
            onCopy={onCopy}
          />
        </div>

        <Separator />

        {/* ── Wake route ── */}
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
          <Select value={routeId} onValueChange={(v) => setRouteId(v ?? "")}>
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

        {/* ── Identity section ── */}
        <div className="grid gap-3">
          <h3 className="text-sm font-medium">Identity</h3>

          {/* ── Known device: show metadata + identifiers ── */}
          {fullKnown && (
            <div className="grid gap-3">
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <Badge variant="secondary">known</Badge>
                {fullKnown.pinned && <Badge variant="outline">pinned</Badge>}
                <span className="font-medium">{fullKnown.display_name}</span>
                {fullKnown.notes && (
                  <span className="text-muted-foreground">
                    : {fullKnown.notes}
                  </span>
                )}
              </div>

              {/* Current identifiers with remove buttons */}
              <div className="rounded-md border bg-muted/30 p-3">
                <div className="mb-2 text-xs font-medium uppercase text-muted-foreground">
                  Identifiers
                </div>
                {fullKnown.identifiers.length ? (
                  <div className="grid gap-1">
                    {fullKnown.identifiers.map((id) => (
                      <div
                        key={id.identifier_key}
                        className="flex items-center justify-between gap-2 rounded px-1 py-0.5 text-sm hover:bg-accent"
                      >
                        <span className="min-w-0 truncate">
                          <Badge
                            variant="outline"
                            className="mr-1.5 text-[0.65rem]"
                          >
                            {id.kind}
                          </Badge>
                          {id.value}
                        </span>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          onClick={() =>
                            void handleDetachIdentifier(id.identifier_key)
                          }
                          disabled={actionBusy}
                          aria-label={`Remove ${id.kind} ${id.value}`}
                        >
                          <Minus className="size-3.5" />
                        </Button>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    No identifiers attached
                  </p>
                )}
              </div>

              {/* Quick-add from row values */}
              {unattachedIdentifiers.length > 0 && (
                <div className="grid gap-1.5">
                  <p className="text-xs text-muted-foreground">
                    Add from this device row:
                  </p>
                  <div className="flex flex-wrap gap-1.5">
                    {unattachedIdentifiers.map((id) => (
                      <Button
                        key={`${id.kind}:${id.value}`}
                        variant="outline"
                        size="sm"
                        disabled={actionBusy}
                        onClick={() =>
                          void wrap(async () => {
                            await attachDeviceIdentifier(
                              fullKnown!.device_id,
                              id,
                            );
                            toast.success(`Added ${id.kind}: ${id.value}`);
                          })
                        }
                      >
                        <Plus className="size-3.5" />
                        {id.kind}: {id.value}
                      </Button>
                    ))}
                  </div>
                </div>
              )}

              {/* Manual add identifier */}
              <div className="grid gap-1.5">
                <p className="text-xs text-muted-foreground">
                  Add identifier manually:
                </p>
                <div className="grid grid-cols-[6rem_minmax(0,1fr)_auto] gap-2">
                  <Select
                    value={addKind}
                    onValueChange={(v) =>
                      setAddKind((v as "mac" | "ip") ?? "mac")
                    }
                  >
                    <SelectTrigger>
                      <span>{addKind}</span>
                    </SelectTrigger>
                    <SelectContent alignItemWithTrigger={false}>
                      <SelectItem value="mac">mac</SelectItem>
                      <SelectItem value="ip">ip</SelectItem>
                    </SelectContent>
                  </Select>
                  <Input
                    value={addValue}
                    onChange={(e) => setAddValue(e.target.value)}
                    placeholder={
                      addKind === "mac" ? "aa:bb:cc:dd:ee:ff" : "192.168.1.100"
                    }
                  />
                  <Button
                    variant="outline"
                    onClick={() => void handleAddIdentifier()}
                    disabled={actionBusy || !addValue.trim()}
                  >
                    <Plus className="size-4" />
                    Add
                  </Button>
                </div>
              </div>

              {/* Merge into another known device */}
              <Separator />
              <div className="grid gap-1.5">
                <p className="text-xs text-muted-foreground">
                  Merge this known device into another:
                </p>
                <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
                  <Select
                    value={targetDeviceId}
                    onValueChange={(v) => setTargetDeviceId(v ?? "")}
                  >
                    <SelectTrigger>
                      <span>
                        {targetDeviceId
                          ? knownDevices.find(
                              (kd) => kd.device_id === targetDeviceId,
                            )?.display_name
                          : "Select target device"}
                      </span>
                    </SelectTrigger>
                    <SelectContent
                      className="max-w-[min(92vw,30rem)]"
                      alignItemWithTrigger={false}
                    >
                      {otherKnownDevices.map((kd) => (
                        <SelectItem key={kd.device_id} value={kd.device_id}>
                          {kd.display_name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Button
                    variant="outline"
                    onClick={() => void handleMerge()}
                    disabled={actionBusy || !targetDeviceId}
                  >
                    Merge
                  </Button>
                </div>
              </div>
            </div>
          )}

          {/* ── Unknown device: remember or attach ── */}
          {!device.known_device && (
            <div className="grid gap-3">
              {/* Remember as new known device */}
              <div className="grid gap-1.5">
                <p className="text-xs text-muted-foreground">
                  Remember as a new known device:
                </p>
                <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
                  <Input
                    value={displayName}
                    onChange={(e) => setDisplayName(e.target.value)}
                    placeholder="Device name"
                  />
                  <Button
                    onClick={() => void handleCreateRemembered()}
                    disabled={actionBusy || !rowIdentifiers.length}
                  >
                    Remember device
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Identifiers:{" "}
                  {rowIdentifiers
                    .map((id) => `${id.kind}:${id.value}`)
                    .join(", ") || "none"}
                </p>
              </div>

              {/* Attach to existing known device */}
              <div className="grid gap-1.5">
                <p className="text-xs text-muted-foreground">
                  Or attach to an existing known device:
                </p>
                <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
                  <Select
                    value={targetDeviceId}
                    onValueChange={(v) => setTargetDeviceId(v ?? "")}
                  >
                    <SelectTrigger>
                      <span>
                        {targetDeviceId
                          ? knownDevices.find(
                              (kd) => kd.device_id === targetDeviceId,
                            )?.display_name
                          : "Select known device"}
                      </span>
                    </SelectTrigger>
                    <SelectContent
                      className="max-w-[min(92vw,30rem)]"
                      alignItemWithTrigger={false}
                    >
                      {knownDevices.map((kd) => (
                        <SelectItem key={kd.device_id} value={kd.device_id}>
                          {kd.display_name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Button
                    variant="outline"
                    onClick={() => void handleAttachToExisting()}
                    disabled={
                      actionBusy || !targetDeviceId || !rowIdentifiers.length
                    }
                  >
                    Attach IDs
                  </Button>
                </div>
              </div>
            </div>
          )}
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
