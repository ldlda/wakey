import { useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";

import {
  type Agent,
  type Alert,
  type AlertTransition,
  type AuditEvent,
  fetchAgents,
  fetchAlertHistory,
  fetchAlerts,
  fetchAudit,
  revokeAgent,
} from "@/api";
import { AppLayout } from "@/layout/AppLayout";
import { AgentsPage } from "@/pages/AgentsPage";
import { AlertsPage } from "@/pages/AlertsPage";
import { AuditPage } from "@/pages/AuditPage";
import { CommandsPage } from "@/pages/CommandsPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { DevicesPage } from "@/pages/DevicesPage";
import { TokensPage } from "@/pages/TokensPage";

type LoadState = "idle" | "loading" | "ready" | "error";

export function App() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [history, setHistory] = useState<AlertTransition[]>([]);
  const [audit, setAudit] = useState<AuditEvent[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState("");

  const [state, setState] = useState<LoadState>("idle");
  const [error, setError] = useState("");

  async function loadAll() {
    setState("loading");
    setError("");
    try {
      const [nextAgents, nextAlerts, nextHistory, nextAudit] =
        await Promise.all([
          fetchAgents(),
          fetchAlerts(),
          fetchAlertHistory(20),
          fetchAudit(30),
        ]);
      setAgents(nextAgents);
      setAlerts(nextAlerts);
      setHistory(nextHistory);
      setAudit(nextAudit);
      const firstConnectedAgentId =
        nextAgents.find((agent) => agent.connected)?.agent_id ?? "";
      if (!nextAgents.length) {
        setSelectedAgentId("");
      } else if (
        !selectedAgentId ||
        !nextAgents.some(
          (agent) => agent.agent_id === selectedAgentId && agent.connected,
        )
      ) {
        setSelectedAgentId(firstConnectedAgentId);
      }
      setState("ready");
    } catch (err) {
      setState("error");
      setError(String(err));
    }
  }

  async function onRevokeAgent(agentId: string): Promise<boolean> {
    const result = await revokeAgent(agentId);
    await loadAll();
    return result.revoked;
  }

  async function refreshAlertsAndHistory() {
    const [nextAlerts, nextHistory] = await Promise.all([
      fetchAlerts(),
      fetchAlertHistory(20),
    ]);
    setAlerts(nextAlerts);
    setHistory(nextHistory);
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
        // Ignore malformed stream payloads and keep current UI state.
      }
    };

    ws.onerror = () => {
      const id = window.setInterval(() => {
        void refreshAlertsAndHistory().catch(() => undefined);
      }, 8000);
      ws.onclose = () => window.clearInterval(id);
    };

    return () => ws.close();
  }, []);

  return (
    <>
      {error && <pre className="error">{error}</pre>}
      <Routes>
        <Route path="/" element={<AppLayout />}>
          <Route
            index
            element={
              <DevicesPage
                agents={agents}
                selectedAgentId={selectedAgentId}
                onSelectAgent={setSelectedAgentId}
                onAfterWake={loadAll}
              />
            }
          />
          <Route
            path="dashboard"
            element={
              <DashboardPage
                agents={agents}
                alerts={alerts}
                transitions={history}
                loading={state === "loading"}
                onRefresh={loadAll}
              />
            }
          />
          <Route
            path="agents"
            element={
              <AgentsPage
                agents={agents}
                selectedAgentId={selectedAgentId}
                onSelectAgent={setSelectedAgentId}
                onRevokeAgent={onRevokeAgent}
              />
            }
          />
          <Route
            path="commands"
            element={
              <CommandsPage
                agents={agents}
                selectedAgentId={selectedAgentId}
                onSelectAgent={setSelectedAgentId}
                onAfterCommand={loadAll}
              />
            }
          />
          <Route
            path="audit"
            element={
              <AuditPage
                events={audit}
                onRefresh={() => fetchAudit(30).then(setAudit)}
              />
            }
          />
          <Route
            path="alerts"
            element={
              <AlertsPage
                alerts={alerts}
                transitions={history}
                onRefresh={refreshAlertsAndHistory}
              />
            }
          />
          <Route path="tokens" element={<TokensPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </>
  );
}
