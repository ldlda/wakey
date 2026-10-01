import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import {
  APIError,
  attachTerminal,
  closeTerminal,
  createTerminal,
  type TerminalSession,
} from "@/api";
import {
  NO_TERMINAL_MODIFIERS,
  applyTerminalModifiers,
  orderTerminalSessions,
  restoreCandidates,
  type TerminalConnectionState,
  type TerminalModifiers,
} from "@/pages/terminal/sessionUtils";
import type { TerminalCatalog } from "@/pages/terminal/useTerminalSessions";
import type { TerminalPort } from "@/pages/terminal/useXterm";

const REMEMBERED_TERMINAL_KEY = "wakey.active-terminal-id";
const TERMINAL_OPERATOR_KEY = "wakey.terminal-operator-id";

// Fallback for browsers that do not implement crypto.randomUUID().
const randomId = () =>
  (String(1e7) + -1e3 + -4e3 + -8e3 + -1e11).replace(/[018]/g, (c: string) => {
    const num = Number(c);
    return (
      num ^
      (window.crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (num / 4)))
    ).toString(16);
  });

function terminalOperatorId(): string {
  const remembered = window.sessionStorage.getItem(TERMINAL_OPERATOR_KEY);
  if (remembered) return remembered;
  const created = window.crypto.randomUUID?.() ?? randomId();
  window.sessionStorage.setItem(TERMINAL_OPERATOR_KEY, created);
  return created;
}

function websocketUrl(path: string): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}${path}`;
}

type Options = {
  terminal: TerminalPort;
  catalog: TerminalCatalog;
  selectedAgentId: string;
};

export function useTerminalSession({
  terminal,
  catalog,
  selectedAgentId,
}: Options) {
  const socketRef = useRef<WebSocket | null>(null);
  const activeSessionRef = useRef<TerminalSession | null>(null);
  const mountedRef = useRef(false);
  const selectedAgentIdRef = useRef(selectedAgentId);
  const catalogRef = useRef(catalog);
  const [session, setSession] = useState<TerminalSession | null>(null);
  const [connection, setConnection] = useState<TerminalConnectionState>("idle");
  const [modifiers, setModifiers] = useState<TerminalModifiers>(
    NO_TERMINAL_MODIFIERS,
  );
  const [operatorId] = useState(terminalOperatorId);

  selectedAgentIdRef.current = selectedAgentId;
  catalogRef.current = catalog;
  activeSessionRef.current = session;

  const detachTransport = useCallback(() => {
    const socket = socketRef.current;
    socketRef.current = null;
    socket?.close();
  }, []);

  const writeTerminalData = useCallback(
    (data: string, consumeModifiers = false) => {
      const socket = socketRef.current;
      if (socket?.readyState !== WebSocket.OPEN) return;
      const output = consumeModifiers
        ? applyTerminalModifiers(data, modifiersRef.current)
        : data;
      socket.send(new TextEncoder().encode(output));
      if (
        consumeModifiers &&
        (modifiersRef.current.ctrl || modifiersRef.current.meta)
      ) {
        modifiersRef.current = NO_TERMINAL_MODIFIERS;
        setModifiers(NO_TERMINAL_MODIFIERS);
      }
    },
    [],
  );

  const modifiersRef = useRef(modifiers);
  modifiersRef.current = modifiers;

  const connect = useCallback(
    (nextSession: TerminalSession) => {
      if (!nextSession.attachment_token) {
        throw new Error(
          "Control plane did not issue a terminal attachment token",
        );
      }
      detachTransport();
      setSession(nextSession);
      activeSessionRef.current = nextSession;
      terminal.setActiveTerminalId(nextSession.terminal_id);
      catalogRef.current.merge({ ...nextSession, operator_attached: true });
      window.sessionStorage.setItem(
        REMEMBERED_TERMINAL_KEY,
        nextSession.terminal_id,
      );
      setConnection("connecting");
      const socket = new WebSocket(websocketUrl(nextSession.websocket_url));
      socket.binaryType = "arraybuffer";
      socketRef.current = socket;

      socket.onopen = () => {
        if (socketRef.current !== socket) return;
        socket.send(
          JSON.stringify({
            type: "attach",
            attachment_token: nextSession.attachment_token,
            operator_id: operatorId,
          }),
        );
        terminal.fit();
        terminal.sendResize((message) => socket.send(message));
      };
      socket.onmessage = (event) => {
        if (socketRef.current !== socket) return;
        if (typeof event.data !== "string") {
          terminal.write(new Uint8Array(event.data as ArrayBuffer));
          return;
        }
        try {
          const control = JSON.parse(event.data) as {
            type: string;
            exit_code?: number | null;
            message?: string;
          };
          if (control.type === "ready") {
            setConnection("ready");
            void catalogRef.current.refresh().catch(() => undefined);
            window.requestAnimationFrame(() => {
              terminal.fit();
              terminal.sendResize((message) => socket.send(message), true);
              terminal.focus();
            });
          } else if (control.type === "exited") {
            setConnection("exited");
            terminal.writeLine(
              `\r\n[process exited${control.exit_code == null ? "" : ` ${control.exit_code}`}]`,
            );
          } else if (control.type === "error") {
            setConnection("exited");
            terminal.writeLine(
              `\r\n[terminal error: ${control.message ?? "unknown error"}]`,
            );
          }
        } catch {
          terminal.writeLine("\r\n[invalid terminal control frame]");
        }
      };
      socket.onerror = () => {
        if (socketRef.current !== socket) return;
        terminal.writeLine("\r\n[terminal transport error]");
      };
      socket.onclose = () => {
        if (socketRef.current !== socket) return;
        socketRef.current = null;
        catalogRef.current.markDetached(nextSession.terminal_id);
        setConnection((current) =>
          current === "exited" ? current : "disconnected",
        );
        void catalogRef.current.refresh().catch(() => undefined);
      };
    },
    [detachTransport, operatorId, terminal],
  );

  const activate = useCallback(
    async (nextSession: TerminalSession) => {
      if (nextSession.terminal_id === activeSessionRef.current?.terminal_id)
        return;
      if (nextSession.operator_attached) return;

      const previousSession = activeSessionRef.current;
      detachTransport();
      if (previousSession)
        catalogRef.current.markDetached(previousSession.terminal_id);
      setSession(null);
      activeSessionRef.current = null;
      terminal.setActiveTerminalId(undefined);
      setConnection("connecting");
      terminal.reset();
      window.sessionStorage.setItem(
        REMEMBERED_TERMINAL_KEY,
        nextSession.terminal_id,
      );

      try {
        const attached = await attachTerminal(
          nextSession.terminal_id,
          operatorId,
        );
        if (!mountedRef.current) return;
        connect(attached);
      } catch (error) {
        if (!mountedRef.current) return;
        setConnection("idle");
        toast.error("Could not attach terminal session", {
          description: String(error),
        });
        void catalogRef.current.refresh().catch(() => undefined);
      }
    },
    [connect, detachTransport, operatorId, terminal],
  );

  const start = useCallback(async () => {
    const dimensions = terminal.dimensions();
    if (!selectedAgentIdRef.current || !dimensions) return;
    const previousSession = activeSessionRef.current;
    detachTransport();
    if (previousSession)
      catalogRef.current.markDetached(previousSession.terminal_id);
    setConnection("connecting");
    terminal.reset();
    try {
      terminal.fit();
      const fittedDimensions = terminal.dimensions() ?? dimensions;
      const created = await createTerminal(
        selectedAgentIdRef.current,
        fittedDimensions.rows,
        fittedDimensions.cols,
      );
      if (!mountedRef.current) return;
      connect(created);
    } catch (error) {
      if (!mountedRef.current) return;
      setSession(null);
      activeSessionRef.current = null;
      terminal.setActiveTerminalId(undefined);
      setConnection("idle");
      toast.error("Could not start terminal", { description: String(error) });
    }
  }, [connect, detachTransport, terminal]);

  const reconnect = useCallback(async () => {
    const currentSession = activeSessionRef.current;
    if (!currentSession) return;
    detachTransport();
    try {
      terminal.reset();
      setConnection("connecting");
      const attached = await attachTerminal(
        currentSession.terminal_id,
        operatorId,
      );
      if (!mountedRef.current) return;
      connect(attached);
    } catch (error) {
      if (!mountedRef.current) return;
      setConnection("disconnected");
      toast.error("Terminal session is no longer available", {
        description: String(error),
      });
    }
  }, [connect, detachTransport, operatorId, terminal]);

  const close = useCallback(
    async (closingSession: TerminalSession) => {
      const closingId = closingSession.terminal_id;
      const closesActiveSession =
        closingId === activeSessionRef.current?.terminal_id;
      const remaining = catalogRef.current
        .getSessions()
        .filter((item) => item.terminal_id !== closingId);
      const fallback = remaining.find((item) => !item.operator_attached);

      if (closesActiveSession) {
        if (socketRef.current?.readyState === WebSocket.OPEN) {
          socketRef.current.send(JSON.stringify({ type: "close" }));
        }
        detachTransport();
      }
      try {
        await closeTerminal(closingId);
      } catch (error) {
        if (!mountedRef.current) return;
        toast.error("Terminal cleanup failed", { description: String(error) });
        void catalogRef.current.refresh().catch(() => undefined);
        return;
      }
      if (!mountedRef.current) return;

      catalogRef.current.remove(closingId);
      terminal.forgetTitle(closingId);
      if (
        window.sessionStorage.getItem(REMEMBERED_TERMINAL_KEY) === closingId
      ) {
        window.sessionStorage.removeItem(REMEMBERED_TERMINAL_KEY);
      }
      if (closesActiveSession) {
        setSession(null);
        activeSessionRef.current = null;
        terminal.setActiveTerminalId(undefined);
        setConnection("idle");
        // reset discards the complete screen and terminal modes; clear() does not.
        terminal.reset();
        if (fallback) void activate(fallback);
      }
    },
    [activate, detachTransport, terminal],
  );

  const toggleModifier = useCallback(
    (modifier: keyof TerminalModifiers) => {
      setModifiers((current) => {
        const next = { ...current, [modifier]: !current[modifier] };
        modifiersRef.current = next;
        return next;
      });
      window.requestAnimationFrame(() => terminal.focus());
    },
    [terminal],
  );

  const writeAccessoryKey = useCallback(
    (data: string) => {
      writeTerminalData(data);
      window.requestAnimationFrame(() => terminal.focus());
    },
    [terminal, writeTerminalData],
  );

  // Transport input/resize subscriptions and restoration begin only once xterm is usable.
  useEffect(() => {
    if (!terminal.ready) return;
    mountedRef.current = true;
    const input = terminal.onInput((data) => writeTerminalData(data, true));
    const resize = terminal.onResize(() => {
      const socket = socketRef.current;
      if (socket?.readyState === WebSocket.OPEN) {
        terminal.sendResize((message) => socket.send(message));
      }
    });

    let cancelled = false;
    async function restoreTerminalSession() {
      try {
        const listed = await catalogRef.current.refresh();
        if (cancelled) return;
        const rememberedId = window.sessionStorage.getItem(
          REMEMBERED_TERMINAL_KEY,
        );
        const candidates = restoreCandidates(
          orderTerminalSessions(listed),
          rememberedId,
          selectedAgentIdRef.current,
        );

        for (const candidate of candidates) {
          if (cancelled) return;
          setConnection("connecting");
          try {
            const attached = await attachTerminal(
              candidate.terminal_id,
              operatorId,
            );
            if (cancelled) return;
            terminal.reset();
            connect(attached);
            return;
          } catch (error) {
            if (
              !(error instanceof APIError) ||
              error.code !== "terminal_operator_already_attached"
            ) {
              throw error;
            }
          }
        }
        if (!cancelled) setConnection("idle");
      } catch (error) {
        if (cancelled) return;
        setConnection("idle");
        toast.error("Could not restore terminal session", {
          description: String(error),
        });
      }
    }

    void restoreTerminalSession();
    return () => {
      cancelled = true;
      mountedRef.current = false;
      input.dispose();
      resize.dispose();
      detachTransport();
    };
  }, [
    terminal.ready,
    terminal.onInput,
    terminal.onResize,
    terminal.reset,
    terminal.sendResize,
    terminal,
    connect,
    detachTransport,
    operatorId,
    writeTerminalData,
  ]);

  return {
    session,
    connection,
    modifiers,
    start,
    activate,
    reconnect,
    close,
    writeTerminalData,
    writeAccessoryKey,
    toggleModifier,
  };
}
