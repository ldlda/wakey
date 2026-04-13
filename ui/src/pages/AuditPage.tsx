import { useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";

import type { AuditEvent } from "@/api";

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
    <section className="card span-full">
      <div className="row-head">
        <h2>Recent Audit Events</h2>
        <button onClick={() => void onRefresh()}>Refresh</button>
      </div>
      <div className="grid-3 compact">
        <label>
          Event type
          <select
            value={eventType}
            onChange={(e) => setEventType(e.target.value)}
          >
            {eventTypes.map((v) => (
              <option key={v} value={v}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label>
          Outcome
          <select value={outcome} onChange={(e) => setOutcome(e.target.value)}>
            {outcomes.map((v) => (
              <option key={v} value={v}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label>
          Search
          <input
            value={needle}
            onChange={(e) => setNeedle(e.target.value)}
            placeholder="message, agent, actor"
          />
        </label>
      </div>
      <p className="muted">
        Showing {filtered.length} of {events.length}
      </p>
      <pre className="output">{JSON.stringify(filtered, null, 2)}</pre>
    </section>
  );
}
