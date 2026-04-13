import { useMemo, useState } from "react";

import type { Alert, AlertTransition } from "@/api";

type Props = {
  alerts: Alert[];
  transitions: AlertTransition[];
  onRefresh: () => Promise<void>;
};

export function AlertsPage({ alerts, transitions, onRefresh }: Props) {
  const [severity, setSeverity] = useState("all");
  const [status, setStatus] = useState("all");
  const [kind, setKind] = useState("all");
  const [transitionQ, setTransitionQ] = useState("");

  const severities = useMemo(
    () => ["all", ...Array.from(new Set(alerts.map((a) => a.severity))).sort()],
    [alerts],
  );
  const statuses = useMemo(
    () => ["all", ...Array.from(new Set(alerts.map((a) => a.status))).sort()],
    [alerts],
  );
  const kinds = useMemo(
    () => ["all", ...Array.from(new Set(alerts.map((a) => a.kind))).sort()],
    [alerts],
  );

  const filteredAlerts = useMemo(
    () =>
      alerts.filter(
        (a) =>
          (severity === "all" || a.severity === severity) &&
          (status === "all" || a.status === status) &&
          (kind === "all" || a.kind === kind),
      ),
    [alerts, severity, status, kind],
  );

  const filteredTransitions = useMemo(() => {
    const q = transitionQ.trim().toLowerCase();
    if (!q) return transitions;
    return transitions.filter(
      (t) =>
        t.kind.toLowerCase().includes(q) ||
        t.to_status.toLowerCase().includes(q) ||
        t.message.toLowerCase().includes(q) ||
        (t.agent_id || "").toLowerCase().includes(q),
    );
  }, [transitions, transitionQ]);

  return (
    <section className="two-col">
      <div className="card">
        <div className="row-head">
          <h2>Active Alerts</h2>
          <button onClick={() => void onRefresh()}>Refresh</button>
        </div>
        <div className="grid-3 compact">
          <label>
            Severity
            <select
              value={severity}
              onChange={(e) => setSeverity(e.target.value)}
            >
              {severities.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
          <label>
            Status
            <select value={status} onChange={(e) => setStatus(e.target.value)}>
              {statuses.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
          <label>
            Kind
            <select value={kind} onChange={(e) => setKind(e.target.value)}>
              {kinds.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="muted">
          Showing {filteredAlerts.length} of {alerts.length}
        </p>
        <div className="list">
          {filteredAlerts.map((alert) => (
            <div className="row plain" key={alert.alert_id}>
              <span>
                {alert.kind}
                {alert.agent_id ? ` (${alert.agent_id})` : ""}
              </span>
              <span>{alert.severity}</span>
            </div>
          ))}
          {!filteredAlerts.length && (
            <div className="empty">No active alerts</div>
          )}
        </div>
      </div>

      <div className="card">
        <div className="row-head">
          <h2>Transition History</h2>
          <span className="muted">{filteredTransitions.length} shown</span>
        </div>
        <label>
          Search
          <input
            value={transitionQ}
            onChange={(e) => setTransitionQ(e.target.value)}
            placeholder="kind, status, message, agent"
          />
        </label>
        <pre className="output small">
          {JSON.stringify(filteredTransitions, null, 2)}
        </pre>
      </div>
    </section>
  );
}
