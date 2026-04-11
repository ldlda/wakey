import { useEffect, useMemo, useState } from "react";
import {
  type Agent,
  type Alert,
  type AlertTransition,
  type AuditEvent,
  type CommandKind,
  fetchAgents,
  fetchAlerts,
  fetchAlertHistory,
  fetchAudit,
  runCommand,
} from "./api";

type LoadState = "idle" | "loading" | "ready" | "error";

export function App() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [history, setHistory] = useState<AlertTransition[]>([]);
  const [audit, setAudit] = useState<AuditEvent[]>([]);

  const [agentId, setAgentId] = useState("");
  const [kind, setKind] = useState<CommandKind>("devs");
  const [query, setQuery] = useState("");
  const [commandOut, setCommandOut] = useState("Select agent and run a command");

  const [state, setState] = useState<LoadState>("idle");
  const [error, setError] = useState<string>("");

  const connectedCount = useMemo(() => agents.filter((a) => a.connected).length, [agents]);

  async function loadAll() {
    setState("loading");
    setError("");
    try {
      const [a, al, h, au] = await Promise.all([
        fetchAgents(),
        fetchAlerts(),
        fetchAlertHistory(20),
        fetchAudit(30),
      ]);
      setAgents(a);
      setAlerts(al);
      setHistory(h);
      setAudit(au);
      if (!agentId && a[0]) setAgentId(a[0].agent_id);
      setState("ready");
    } catch (err) {
      setState("error");
      setError(String(err));
    }
  }

  useEffect(() => {
    void loadAll();
  }, []);

  useEffect(() => {
    const wsUrl = `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}/api/v1/control/alerts/ws`;
    const ws = new WebSocket(wsUrl);
    ws.onmessage = (evt) => {
      try {
        const payload = JSON.parse(String(evt.data)) as {
          alerts?: Alert[];
          recent_transitions?: AlertTransition[];
        };
        if (payload.alerts) setAlerts(payload.alerts);
        if (payload.recent_transitions) setHistory(payload.recent_transitions);
      } catch {
        // Keep stream parse failures isolated from primary UI state.
      }
    };
    ws.onerror = () => {
      // Poll fallback while stream is down.
      const id = window.setInterval(() => {
        void fetchAlerts().then(setAlerts).catch(() => undefined);
      }, 8000);
      ws.onclose = () => window.clearInterval(id);
    };
    return () => ws.close();
  }, []);

  async function onRunCommand(e: React.FormEvent) {
    e.preventDefault();
    if (!agentId) return;

    setCommandOut("Running...");
    try {
      const out = await runCommand(agentId, kind, query.trim());
      setCommandOut(JSON.stringify(out, null, 2));
      const au = await fetchAudit(30);
      setAudit(au);
    } catch (err) {
      setCommandOut(String(err));
    }
  }

  return (
    <div className="app">
      <header className="topbar">
        <div>
          <h1>Wakey Operator UI</h1>
          <p>Fast ops surface for agents, commands, audits, and alerts</p>
        </div>
        <div className={`pill ${state}`}>
          {state === "ready" ? "Ready" : state === "loading" ? "Loading" : state === "error" ? "Error" : "Idle"}
        </div>
      </header>

      {error && <pre className="error">{error}</pre>}

      <section className="stats">
        <div className="card stat"><h3>Agents</h3><strong>{agents.length}</strong></div>
        <div className="card stat"><h3>Connected</h3><strong>{connectedCount}</strong></div>
        <div className="card stat"><h3>Active Alerts</h3><strong>{alerts.length}</strong></div>
        <div className="card stat"><h3>Transitions</h3><strong>{history.length}</strong></div>
      </section>

      <main className="grid">
        <section className="card">
          <div className="row-head"><h2>Agents</h2><button onClick={() => void loadAll()}>Refresh</button></div>
          <div className="list">
            {agents.map((a) => (
              <button key={a.agent_id} className={`row ${agentId === a.agent_id ? "selected" : ""}`} onClick={() => setAgentId(a.agent_id)}>
                <span>{a.agent_id}</span>
                <span>{a.connected ? "connected" : "offline"}</span>
              </button>
            ))}
            {!agents.length && <div className="empty">No agents found</div>}
          </div>
        </section>

        <section className="card">
          <h2>Command Runner</h2>
          <form className="form" onSubmit={onRunCommand}>
            <label>Agent ID<input value={agentId} onChange={(e) => setAgentId(e.target.value)} required /></label>
            <label>Command
              <select value={kind} onChange={(e) => setKind(e.target.value as CommandKind)}>
                <option value="devs">devs</option>
                <option value="status">status</option>
                <option value="leases">leases</option>
                <option value="inventory">inventory</option>
                <option value="wake">wake</option>
              </select>
            </label>
            <label>Query<input value={query} onChange={(e) => setQuery(e.target.value)} /></label>
            <button type="submit">Run command</button>
          </form>
          <pre className="output">{commandOut}</pre>
        </section>

        <section className="card">
          <div className="row-head"><h2>Active Alerts</h2><button onClick={() => void fetchAlerts().then(setAlerts)}>Refresh</button></div>
          <div className="list">
            {alerts.map((a) => (
              <div key={a.alert_id} className="row plain">
                <span>{a.kind}{a.agent_id ? ` (${a.agent_id})` : ""}</span>
                <span>{a.severity}</span>
              </div>
            ))}
            {!alerts.length && <div className="empty">No active alerts</div>}
          </div>
        </section>

        <section className="card">
          <h2>Alert Transitions</h2>
          <pre className="output small">{JSON.stringify(history, null, 2)}</pre>
        </section>

        <section className="card span-2">
          <div className="row-head"><h2>Recent Audit</h2><button onClick={() => void fetchAudit(30).then(setAudit)}>Refresh</button></div>
          <pre className="output">{JSON.stringify(audit, null, 2)}</pre>
        </section>
      </main>
    </div>
  );
}
