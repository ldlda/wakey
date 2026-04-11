import type { Alert, AlertTransition } from "@/api";

type Props = {
  alerts: Alert[];
  transitions: AlertTransition[];
  onRefresh: () => Promise<void>;
};

export function AlertsPage({ alerts, transitions, onRefresh }: Props) {
  return (
    <section className="two-col">
      <div className="card">
        <div className="row-head">
          <h2>Active Alerts</h2>
          <button onClick={() => void onRefresh()}>Refresh</button>
        </div>
        <div className="list">
          {alerts.map((alert) => (
            <div className="row plain" key={alert.alert_id}>
              <span>{alert.kind}{alert.agent_id ? ` (${alert.agent_id})` : ""}</span>
              <span>{alert.severity}</span>
            </div>
          ))}
          {!alerts.length && <div className="empty">No active alerts</div>}
        </div>
      </div>

      <div className="card">
        <h2>Transition History</h2>
        <pre className="output small">{JSON.stringify(transitions, null, 2)}</pre>
      </div>
    </section>
  );
}
