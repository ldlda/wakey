import { useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";

import type { AuditEvent } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

type Props = {
  events: AuditEvent[];
  onRefresh: () => Promise<void>;
};

export function AuditPage({ events, onRefresh }: Props) {
  const [searchParams] = useSearchParams();
  const initialEventType = searchParams.get("event_type") || "all";
  const initialOutcome = searchParams.get("outcome") || "all";
  const initialNeedle = searchParams.get("q") || "";

  const [eventType, setEventType] = useState(initialEventType);
  const [outcome, setOutcome] = useState(initialOutcome);
  const [needle, setNeedle] = useState(initialNeedle);

  const eventTypes = useMemo(
    () => [
      "all",
      ...Array.from(new Set(events.map((e) => e.event_type))).sort(),
    ],
    [events],
  );

  const outcomes = useMemo(
    () => ["all", ...Array.from(new Set(events.map((e) => e.outcome))).sort()],
    [events],
  );

  const filtered = useMemo(() => {
    const q = needle.trim().toLowerCase();
    return events.filter((e) => {
      if (eventType !== "all" && e.event_type !== eventType) return false;
      if (outcome !== "all" && e.outcome !== outcome) return false;
      if (!q) return true;
      return (
        e.message.toLowerCase().includes(q) ||
        e.event_type.toLowerCase().includes(q) ||
        e.outcome.toLowerCase().includes(q) ||
        e.request_id?.toLowerCase().includes(q) || // why does the one flow from devicespage use this
        (e.actor_id || "").toLowerCase().includes(q) ||
        (e.agent_id || "").toLowerCase().includes(q)
      );
    });
  }, [events, eventType, outcome, needle]);

  return (
    <Card size="sm">
      <CardHeader className="flex items-center justify-between gap-2">
        <CardTitle>Recent Audit Events</CardTitle>
        <Button size="sm" variant="outline" onClick={() => void onRefresh()}>
          Refresh
        </Button>
      </CardHeader>

      <CardContent className="space-y-3">
        <div className="grid gap-2 sm:grid-cols-3">
          <label className="grid gap-1 text-sm text-muted-foreground">
            Event type
            <Select
              value={eventType}
              onValueChange={(value) => {
                if (value) setEventType(value);
              }}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Event type" />
              </SelectTrigger>
              <SelectContent>
                {eventTypes.map((v) => (
                  <SelectItem key={v} value={v}>
                    {v}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>

          <label className="grid gap-1 text-sm text-muted-foreground">
            Outcome
            <Select
              value={outcome}
              onValueChange={(value) => {
                if (value) setOutcome(value);
              }}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Outcome" />
              </SelectTrigger>
              <SelectContent>
                {outcomes.map((v) => (
                  <SelectItem key={v} value={v}>
                    {v}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>

          <label className="grid gap-1 text-sm text-muted-foreground">
            Search
            <Input
              value={needle}
              onChange={(e) => setNeedle(e.target.value)}
              placeholder="message, agent, actor"
            />
          </label>
        </div>

        <p className="text-sm text-muted-foreground">
          Showing {filtered.length} of {events.length}
        </p>

        <Textarea
          className="min-h-90 font-mono text-xs"
          readOnly
          value={JSON.stringify(filtered, null, 2)}
        />
      </CardContent>
    </Card>
  );
}
