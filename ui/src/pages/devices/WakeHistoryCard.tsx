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
      <CardHeader className="row-head">
        <CardTitle>Recent Wake Actions</CardTitle>
        <Button variant="outline" size="sm" onClick={onClear}>
          Clear
        </Button>
      </CardHeader>
      <CardContent>
        <div className="list">
          {events.map((event, idx) => (
            <div
              className="row plain"
              key={`${event.ts}-${event.target}-${idx}`}
            >
              <div>
                <strong>{event.target}</strong>
                <div className="muted">
                  {new Date(event.ts).toLocaleString()} on {event.agentId}
                </div>
                <div className="muted">{event.detail}</div>
                {event.requestId && (
                  <Link
                    className="muted linkish"
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
          {!events.length && <div className="empty">No wake actions yet</div>}
        </div>
      </CardContent>
    </Card>
  );
}
