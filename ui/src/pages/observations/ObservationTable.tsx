import type { AgentDeviceObservation, KnownDevice } from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ObservationIdentityActions } from "@/pages/observations/ObservationIdentityActions";
import {
  formatSeen,
  observationLabel,
  observationStateLabel,
} from "@/pages/observations/utils";

type Props = {
  observations: AgentDeviceObservation[];
  devices: KnownDevice[];
  selectedDevices: Record<string, string>;
  newNames: Record<string, string>;
  busyKey: string;
  onSelectDevice: (observationKey: string, deviceId: string) => void;
  onNewNameChange: (observationKey: string, name: string) => void;
  onAttachExisting: (observation: AgentDeviceObservation) => void;
  onCreateAndAttach: (observation: AgentDeviceObservation) => void;
  onOpenHistory: (observation: AgentDeviceObservation) => void;
};

export function ObservationTable({
  observations,
  devices,
  selectedDevices,
  newNames,
  busyKey,
  onSelectDevice,
  onNewNameChange,
  onAttachExisting,
  onCreateAndAttach,
  onOpenHistory,
}: Props) {
  return (
    <div className="grid gap-2 overflow-x-auto">
      <div className="grid min-w-[76rem] grid-cols-[minmax(10rem,1.2fr)_minmax(8rem,0.8fr)_minmax(8rem,0.8fr)_minmax(9rem,0.8fr)_minmax(16rem,1.3fr)_7rem] gap-2 rounded-md border bg-muted/60 px-3 py-2 text-sm">
        <span>Device</span>
        <span>Network</span>
        <span>Agent</span>
        <span>Seen</span>
        <span>Known Device</span>
        <span>History</span>
      </div>

      {observations.map((observation) => {
        const busy = busyKey === observation.observation_key;
        return (
          <div
            key={observation.observation_key}
            className="grid min-w-[76rem] grid-cols-[minmax(10rem,1.2fr)_minmax(8rem,0.8fr)_minmax(8rem,0.8fr)_minmax(9rem,0.8fr)_minmax(16rem,1.3fr)_7rem] gap-2 rounded-md border bg-card px-3 py-2 text-sm"
          >
            <div className="min-w-0">
              <div className="truncate font-medium">
                {observationLabel(observation)}
              </div>
              <div className="mt-1 flex flex-wrap gap-1 text-xs text-muted-foreground">
                <Badge variant="outline">{observation.kind}</Badge>
                <Badge
                  variant={
                    observation.last_action === "remove"
                      ? "outline"
                      : "secondary"
                  }
                >
                  {observationStateLabel(observation.last_action)}
                </Badge>
                <Badge variant="outline">{observation.last_action}</Badge>
              </div>
            </div>
            <div className="min-w-0 text-muted-foreground">
              <div className="truncate" title={observation.ip ?? "-"}>
                {observation.ip ?? "-"}
              </div>
              <div className="truncate" title={observation.mac ?? "-"}>
                {observation.mac ?? "-"}
              </div>
            </div>
            <div className="min-w-0 truncate text-muted-foreground">
              {observation.agent_id}
            </div>
            <div className="min-w-0 text-muted-foreground">
              <div className="truncate">
                {formatSeen(observation.last_seen_unix)}
              </div>
            </div>
            <div className="min-w-0">
              {observation.known_device ? (
                <div className="grid gap-1">
                  <div className="flex min-w-0 items-center gap-2">
                    <Badge variant="secondary">known</Badge>
                    <span className="truncate font-medium">
                      {observation.known_device.display_name}
                    </span>
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {observation.known_device.pinned
                      ? "pinned"
                      : "not pinned"}
                  </span>
                </div>
              ) : (
                <ObservationIdentityActions
                  observation={observation}
                  devices={devices}
                  busy={busy}
                  selectedDeviceId={
                    selectedDevices[observation.observation_key] ?? ""
                  }
                  newName={newNames[observation.observation_key] ?? ""}
                  onSelectDevice={(deviceId) =>
                    onSelectDevice(observation.observation_key, deviceId)
                  }
                  onNewNameChange={(name) =>
                    onNewNameChange(observation.observation_key, name)
                  }
                  onAttachExisting={() => onAttachExisting(observation)}
                  onCreateAndAttach={() => onCreateAndAttach(observation)}
                />
              )}
            </div>
            <div className="flex items-start">
              <Button
                variant="outline"
                size="sm"
                onClick={() => onOpenHistory(observation)}
              >
                History
              </Button>
            </div>
          </div>
        );
      })}

      {!observations.length && (
        <div className="px-1 py-2 text-sm text-muted-foreground">
          No observations found
        </div>
      )}
    </div>
  );
}
