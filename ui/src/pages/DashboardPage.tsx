import type { Agent, Alert, AlertTransition } from "@/api";

type Props = {
  agents: Agent[];
  alerts: Alert[];
  transitions: AlertTransition[];
  loading: boolean;
  onRefresh: () => void;
};

export function DashboardPage({
  agents,
  alerts,
  transitions,
  loading,
  onRefresh,
}: Props) {
  const connected = agents.filter((a) => a.connected).length;
  return (
    <section className="card-grid">
      <div className="card stat">
        <h3>Agents</h3>
        <strong>{agents.length}</strong>
      </div>
      <div className="card stat">
        <h3>Connected</h3>
        <strong>{connected}</strong>
      </div>
      <div className="card stat">
        <h3>Active Alerts</h3>
        <strong>{alerts.length}</strong>
      </div>
      <div className="card stat">
        <h3>Transitions</h3>
        <strong>{transitions.length}</strong>
      </div>
      <div className="card span-full">
        <div className="row-head">
          <h2>Overview</h2>
          <button onClick={onRefresh} disabled={loading}>
            {loading ? "Refreshing..." : "Refresh"}
          </button>
        </div>
        <p className="muted">
          Use tabs to run commands, inspect audits, and monitor alerts in real
          time.
        </p>
      </div>
    </section>
  );
}
