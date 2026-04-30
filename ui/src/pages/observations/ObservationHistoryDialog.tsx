import { useEffect, useState } from "react";

import {
  fetchObservationHistory,
  type AgentDeviceObservation,
  type AgentDeviceObservationEvent,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  formatSeen,
  observationLabel,
  observationStateLabel,
} from "@/pages/observations/utils";

type Props = {
  observation: AgentDeviceObservation | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function ObservationHistoryDialog({
  observation,
  open,
  onOpenChange,
}: Props) {
  const [events, setEvents] = useState<AgentDeviceObservationEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open || !observation) return;
    setLoading(true);
    setError("");
    fetchObservationHistory({
      observationKey: observation.observation_key,
      limit: 100,
    })
      .then(setEvents)
      .catch((err) => {
        setEvents([]);
        setError(String(err));
      })
      .finally(() => setLoading(false));
  }, [open, observation]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Observation History</DialogTitle>
          <DialogDescription>
            {observation
              ? observationLabel(observation)
              : "No observation selected"}
          </DialogDescription>
        </DialogHeader>

        {error && (
          <pre className="max-h-40 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
            {error}
          </pre>
        )}

        <div className="max-h-[60vh] overflow-auto rounded-md border">
          <div className="grid min-w-176 grid-cols-[10rem_8rem_9rem_minmax(8rem,1fr)_minmax(8rem,1fr)] gap-2 border-b bg-muted/60 px-3 py-2 text-xs font-medium text-muted-foreground">
            <span>Time</span>
            <span>State</span>
            <span>Source</span>
            <span>Network</span>
            <span>Known Device</span>
          </div>

          {events.map((event) => (
            <div
              key={event.event_id}
              className="grid min-w-176 grid-cols-[10rem_8rem_9rem_minmax(8rem,1fr)_minmax(8rem,1fr)] gap-2 border-b px-3 py-2 text-sm last:border-b-0"
            >
              <span className="truncate text-muted-foreground">
                {formatSeen(event.ts_unix)}
              </span>
              <span>
                <Badge
                  variant={event.action === "remove" ? "outline" : "secondary"}
                >
                  {observationStateLabel(event.action)}
                </Badge>
              </span>
              <span className="flex min-w-0 gap-1">
                <Badge variant="outline">{event.kind}</Badge>
                <Badge variant="outline">{event.action}</Badge>
              </span>
              <span className="min-w-0 text-muted-foreground">
                <span className="block truncate">{event.ip ?? "-"}</span>
                <span className="block truncate">{event.mac ?? "-"}</span>
                {event.hostname && (
                  <span className="block truncate">{event.hostname}</span>
                )}
              </span>
              <span className="min-w-0 truncate">
                {event.known_device?.display_name ?? "-"}
              </span>
            </div>
          ))}

          {!events.length && (
            <div className="px-3 py-4 text-sm text-muted-foreground">
              {loading ? "Loading history..." : "No history found"}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
