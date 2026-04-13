import { Link } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { WakeEvent } from "@/pages/devices/types";

type Props = {
  events: WakeEvent[];
  onClear: () => void;
};

export function WakeHistoryCard({ events, onClear }: Props) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
        <CardTitle>Recent Wake Actions</CardTitle>
        <Button variant="outline" size="sm" onClick={onClear}>
          Clear
        </Button>
      </CardHeader>
      <CardContent>
        <div className="grid gap-2">
          {events.map((event, idx) => (
            <div
              className="flex items-start justify-between gap-3 rounded-md border bg-card px-3 py-2"
              key={`${event.ts}-${event.target}-${idx}`}
            >
              <div>
                <strong>{event.target}</strong>
                <div className="text-xs text-muted-foreground">
                  {new Date(event.ts).toLocaleString()} on {event.agentId}
                </div>
                <div className="text-xs text-muted-foreground">
                  {event.detail}
                </div>
                {event.requestId && (
                  <Link
                    className="linkish text-xs text-muted-foreground"
                    to={`/audit?q=${encodeURIComponent(event.requestId)}`}
                  >
                    View related audit ({event.requestId})
                  </Link>
                )}
              </div>
              <Badge
                variant={event.outcome === "ok" ? "secondary" : "destructive"}
              >
                {event.outcome}
              </Badge>
            </div>
          ))}
          {!events.length && (
            <div className="px-1 py-2 text-sm text-muted-foreground">
              No wake actions yet
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
