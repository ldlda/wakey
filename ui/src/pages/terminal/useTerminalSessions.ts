import { useCallback, useEffect, useRef, useState } from "react";

import { listTerminals, type TerminalSession } from "@/api";
import {
  mergeTerminalSession,
  reconcileTerminalSessions,
} from "@/pages/terminal/sessionUtils";

function websocketUrl(path: string): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}${path}`;
}

export function useTerminalSessions() {
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const sessionsRef = useRef(sessions);
  const refreshRef = useRef<() => Promise<TerminalSession[]>>(async () => []);

  sessionsRef.current = sessions;

  const refresh = useCallback(() => refreshRef.current(), []);
  const merge = useCallback((session: TerminalSession) => {
    setSessions((current) => mergeTerminalSession(current, session));
  }, []);
  const markDetached = useCallback((terminalId: string) => {
    setSessions((current) =>
      current.map((item) =>
        item.terminal_id === terminalId
          ? { ...item, operator_attached: false }
          : item,
      ),
    );
  }, []);
  const remove = useCallback((terminalId: string) => {
    setSessions((current) =>
      current.filter((item) => item.terminal_id !== terminalId),
    );
  }, []);
  const reconcile = useCallback((listed: TerminalSession[]) => {
    setSessions((current) => reconcileTerminalSessions(current, listed));
  }, []);
  const getSessions = useCallback(() => sessionsRef.current, []);

  // The catalog stream and fallback timer live for the page mount, independent of active sessions.
  useEffect(() => {
    let cancelled = false;
    let refreshInFlight: Promise<TerminalSession[]> | null = null;
    let refreshRequested = false;
    let eventsSocket: WebSocket | null = null;
    let reconnectTimer = 0;
    let healthyTimer = 0;
    let reconnectAttempt = 0;

    const refreshSessions = (): Promise<TerminalSession[]> => {
      if (refreshInFlight) {
        // Coalesce bursts and fetch again if another invalidation arrives in flight.
        refreshRequested = true;
        return refreshInFlight;
      }
      refreshInFlight = (async () => {
        let listed: TerminalSession[] = [];
        do {
          refreshRequested = false;
          listed = await listTerminals();
          if (!cancelled) {
            setSessions((current) =>
              reconcileTerminalSessions(current, listed),
            );
          }
        } while (refreshRequested && !cancelled);
        return listed;
      })().finally(() => {
        refreshInFlight = null;
        if (refreshRequested && !cancelled) {
          void refreshSessions().catch(() => undefined);
        }
      });
      return refreshInFlight;
    };

    refreshRef.current = refreshSessions;

    const connectEvents = () => {
      if (cancelled) return;
      const socket = new WebSocket(
        websocketUrl("/api/v1/control/terminals/events/ws"),
      );
      eventsSocket = socket;
      socket.onmessage = (event) => {
        if (cancelled || eventsSocket !== socket) return;
        if (typeof event.data !== "string") return;
        try {
          const message = JSON.parse(event.data) as { type?: string };
          if (message.type === "sessions_changed") {
            void refreshSessions().catch(() => undefined);
          }
        } catch {
          // Invalidations are hints; the periodic recovery fetch remains active.
        }
      };
      socket.onopen = () => {
        void refreshSessions().catch(() => undefined);
        window.clearTimeout(healthyTimer);
        healthyTimer = window.setTimeout(() => {
          if (eventsSocket === socket && socket.readyState === WebSocket.OPEN) {
            reconnectAttempt = 0;
          }
        }, 10_000);
      };
      socket.onclose = () => {
        if (cancelled || eventsSocket !== socket) return;
        window.clearTimeout(healthyTimer);
        const ceiling = Math.min(500 * 2 ** reconnectAttempt, 12_000);
        const delay = Math.min(
          Math.round(ceiling * (0.8 + Math.random() * 0.4)),
          15_000,
        );
        reconnectAttempt = Math.min(reconnectAttempt + 1, 5);
        reconnectTimer = window.setTimeout(connectEvents, delay);
      };
      socket.onerror = () => socket.close();
    };

    connectEvents();
    void refreshSessions().catch(() => undefined);
    const refreshTimer = window.setInterval(() => {
      void refreshSessions().catch(() => undefined);
    }, 15_000);
    const refreshOnFocus = () => {
      void refreshSessions().catch(() => undefined);
    };
    window.addEventListener("focus", refreshOnFocus);

    return () => {
      cancelled = true;
      window.clearInterval(refreshTimer);
      window.removeEventListener("focus", refreshOnFocus);
      window.clearTimeout(reconnectTimer);
      window.clearTimeout(healthyTimer);
      eventsSocket?.close();
      eventsSocket = null;
      refreshRef.current = async () => [];
    };
  }, []);

  return {
    sessions,
    refresh,
    merge,
    markDetached,
    remove,
    reconcile,
    getSessions,
  };
}

export type TerminalCatalog = ReturnType<typeof useTerminalSessions>;
