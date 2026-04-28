import type { AgentDeviceObservation, KnownDevice } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import {
  identifierSummary,
  observationLabel,
} from "@/pages/observations/utils";

type Props = {
  observation: AgentDeviceObservation;
  devices: KnownDevice[];
  busy: boolean;
  selectedDeviceId: string;
  newName: string;
  onSelectDevice: (deviceId: string) => void;
  onNewNameChange: (name: string) => void;
  onAttachExisting: () => void;
  onCreateAndAttach: () => void;
};

export function ObservationIdentityActions({
  observation,
  devices,
  busy,
  selectedDeviceId,
  newName,
  onSelectDevice,
  onNewNameChange,
  onAttachExisting,
  onCreateAndAttach,
}: Props) {
  const attachable = Boolean(observation.mac || observation.ip);
  const selectedDevice = devices.find(
    (device) => device.device_id === selectedDeviceId,
  );

  return (
    <div className="grid min-w-0 gap-2">
      <div className="flex min-w-0 gap-2">
        <Select
          value={selectedDeviceId}
          onValueChange={(value: string | null) => onSelectDevice(value ?? "")}
        >
          <SelectTrigger className="min-w-0 flex-1">
            <span className="truncate">
              {selectedDevice?.display_name ?? "Attach to existing"}
            </span>
          </SelectTrigger>
          <SelectContent
            className="max-w-[min(92vw,28rem)]"
            alignItemWithTrigger={false}
          >
            {devices.map((device) => (
              <SelectItem key={device.device_id} value={device.device_id}>
                <span className="grid min-w-0 gap-0.5">
                  <span className="truncate">{device.display_name}</span>
                  <span className="truncate text-xs text-muted-foreground">
                    {identifierSummary(device)}
                  </span>
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          disabled={busy || !attachable || !selectedDeviceId}
          onClick={onAttachExisting}
        >
          Attach
        </Button>
      </div>
      <div className="flex min-w-0 gap-2">
        <Input
          value={newName}
          onChange={(event) => onNewNameChange(event.target.value)}
          placeholder={`Create as ${observationLabel(observation)}`}
          disabled={busy || !attachable}
        />
        <Button
          size="sm"
          disabled={busy || !attachable}
          onClick={onCreateAndAttach}
        >
          Create
        </Button>
      </div>
    </div>
  );
}
