import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { toast } from "sonner";

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
  setAgentNickname,
} from "@/api";
import { Toaster } from "@/components/ui/sonner";
import { Skeleton } from "@/components/ui/skeleton";
import { AppLayout } from "@/layout/AppLayout";
import { AgentsPage } from "@/pages/AgentsPage";
import { AlertsPage } from "@/pages/AlertsPage";
import { AuditPage } from "@/pages/AuditPage";
import { CommandsPage } from "@/pages/CommandsPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { DevicesPage } from "@/pages/DevicesPage";
import { TokensPage } from "@/pages/TokensPage";

const TerminalPage = lazy(() =>
  import("@/pages/TerminalPage").then((module) => ({
    default: module.TerminalPage,
  })),
);
import { WakeToolsPage } from "@/pages/WakeToolsPage";

type LoadState = "idle" | "loading" | "ready" | "error";

export function App() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [history, setHistory] = useState<AlertTransition[]>([]);
  const [audit, setAudit] = useState<AuditEvent[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState("");
  const [alertsHealth, setAlertsHealth] = useState<{
    available: boolean;
    reason?: string;
  } | null>(null);
  const [alertsStreamConnected, setAlertsStreamConnected] = useState(false);
  const streamHealthReceived = useRef(false);
  const alertsRevision = useRef(0);

  const [state, setState] = useState<LoadState>("idle");

  async function loadAlertsWithoutBlocking() {
    try {
      const nextAlerts = await fetchAlerts();
      if (!streamHealthReceived.current) {
        setAlertsHealth({ available: true });
      }
      return nextAlerts;
    } catch (err) {
      if (!streamHealthReceived.current) {
        setAlertsHealth({ available: false, reason: String(err) });
      }
      return null;
    }
  }

  async function loadAll() {
    const revision = alertsRevision.current;
    setState("loading");
    try {
      const [nextAgents, nextAlerts, nextHistory, nextAudit] =
        await Promise.all([
          fetchAgents(),
          loadAlertsWithoutBlocking(),
          fetchAlertHistory(20),
          fetchAudit(30),
        ]);
      setAgents(nextAgents);
      // Streamed state wins over HTTP requests started before its arrival.
      if (revision === alertsRevision.current) {
        if (nextAlerts) setAlerts(nextAlerts);
        setHistory(nextHistory);
      }
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
      toast.error("Failed to load data", { description: String(err) });
    }
  }

  async function onRevokeAgent(agentId: string): Promise<boolean> {
    const result = await revokeAgent(agentId);
    await loadAll();
    if (result.revoked) {
      toast.success(`Revoked ${agentId}`);
    }
    return result.revoked;
  }

  async function onSetAgentNickname(
    agentId: string,
    nickname: string | null,
  ): Promise<boolean> {
    const result = await setAgentNickname(agentId, nickname);
    await loadAll();
    if (result.updated) {
      toast.success(`Nickname updated for ${agentId}`, {
        description: nickname || "(cleared)",
      });
    }
    return result.updated;
  }

  async function refreshAlertsAndHistory() {
    const revision = alertsRevision.current;
    const [nextAlerts, nextHistory] = await Promise.all([
      loadAlertsWithoutBlocking(),
      fetchAlertHistory(20),
    ]);
    if (revision === alertsRevision.current) {
      if (nextAlerts) setAlerts(nextAlerts);
      setHistory(nextHistory);
    }
  }

  useEffect(() => {
    void loadAll();
  }, []);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let disposed = false;
    let attempt = 0;
    let openedAt = 0;
    let reconnectTimer: number | undefined;

    const resync = () => {
      const generation = alertsRevision.current;
      void Promise.all([fetchAlerts(), fetchAlertHistory(20)])
        .then(([nextAlerts, nextHistory]) => {
          // A WS snapshot delivered while this request was in flight is newer.
          if (disposed || generation !== alertsRevision.current) return;
          setAlerts(nextAlerts);
          setHistory(nextHistory);
        })
        .catch(() => undefined);
    };

    const connect = () => {
      if (disposed) return;
      const wsUrl = `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}/api/v1/control/alerts/ws`;
      socket = new WebSocket(wsUrl);

      socket.onopen = () => {
        if (disposed) return;
        openedAt = Date.now();
        setAlertsStreamConnected(true);
        // Pull anything missed while the stream was down.
        resync();
      };

      socket.onmessage = (evt) => {
        if (disposed) return;
        alertsRevision.current += 1;
        try {
          const payload = JSON.parse(String(evt.data)) as {
            type?: string;
            available?: boolean;
            reason?: string;
            alerts?: Alert[];
            recent_transitions?: AlertTransition[];
          };
          if (payload.type === "alerts_health") {
            streamHealthReceived.current = true;
            setAlertsHealth({
              available: payload.available === true,
              reason: payload.reason,
            });
            return;
          }
          if (payload.alerts) setAlerts(payload.alerts);
          if (payload.recent_transitions)
            setHistory(payload.recent_transitions);
        } catch {
          // Ignore malformed stream payloads and keep current UI state.
        }
      };

      socket.onclose = () => {
        socket = null;
        if (disposed) return;
        setAlertsStreamConnected(false);
        // Only a connection that stayed up proves the stream is healthy; an
        // accept-then-close server must not pin us to the first backoff step.
        if (openedAt && Date.now() - openedAt > 10_000) attempt = 0;
        // Browsers never reconnect a WebSocket on their own.
        const delay =
          Math.min(30_000, 1000 * 2 ** attempt) + Math.random() * 1000;
        attempt += 1;
        reconnectTimer = window.setTimeout(connect, delay);
      };
    };

    connect();

    return () => {
      disposed = true;
      if (reconnectTimer !== undefined) window.clearTimeout(reconnectTimer);
      socket?.close();
    };
  }, []);

  return (
    <>
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
                onRefresh={loadAll}
              />
            }
          />
          <Route
            path="wake"
            element={
              <WakeToolsPage
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
                audit={audit}
                loading={state === "loading"}
                alertsHealth={alertsHealth}
                alertsStreamConnected={alertsStreamConnected}
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
                onSetAgentNickname={onSetAgentNickname}
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
                alertsHealth={alertsHealth}
                alertsStreamConnected={alertsStreamConnected}
                onRefresh={refreshAlertsAndHistory}
              />
            }
          />
          <Route path="tokens" element={<TokensPage />} />
          <Route
            path="terminal"
            element={
              <Suspense
                fallback={
                  <Skeleton className="h-[calc(100dvh-2.5rem)] w-full" />
                }
              >
                <TerminalPage
                  agents={agents}
                  selectedAgentId={selectedAgentId}
                  onSelectAgent={setSelectedAgentId}
                />
              </Suspense>
            }
          />

          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
      <Toaster richColors position="bottom-right" />
    </>
  );
}
