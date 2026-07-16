import { useCallback, useEffect, useRef, useState } from "react";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { UnicodeGraphemesAddon } from "@xterm/addon-unicode-graphemes";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal as XTerm } from "@xterm/xterm";
import { Eraser, Plus, RotateCcw, Terminal, X } from "lucide-react";
import { toast } from "sonner";

import {
  APIError,
  type Agent,
  type TerminalSession,
  attachTerminal,
  closeTerminal,
  createTerminal,
  listTerminals,
} from "@/api";
import { AgentSelector, displayAgentLabel } from "@/components/AgentSelector";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
};

type ConnectionState =
  | "idle"
  | "connecting"
  | "ready"
  | "disconnected"
  | "exited";

const REMEMBERED_TERMINAL_KEY = "wakey.active-terminal-id";
const TERMINAL_OPERATOR_KEY = "wakey.terminal-operator-id";

// polyfill for testing in browsers that don't support crypto.randomUUID()
const thing = () =>
  (String(1e7) + -1e3 + -4e3 + -8e3 + -1e11).replace(/[018]/g, (c: string) => {
    const num = Number(c);
    return (
      num ^
      (window.crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (num / 4)))
    ).toString(16);
  });

function terminalOperatorId(): string {
  // Session storage survives route unmounts but remains scoped to this browser
  // tab. The ID coordinates attachment ownership; API authentication remains
  // the security boundary.
  const remembered = window.sessionStorage.getItem(TERMINAL_OPERATOR_KEY);
  if (remembered) return remembered;
  const created = window.crypto.randomUUID?.() ?? thing();
  window.sessionStorage.setItem(TERMINAL_OPERATOR_KEY, created);
  return created;
}

function mergeTerminalSession(
  sessions: TerminalSession[],
  next: TerminalSession,
): TerminalSession[] {
  const existingIndex = sessions.findIndex(
    (item) => item.terminal_id === next.terminal_id,
  );
  if (existingIndex >= 0) {
    // Attaching updates session state, not identity or tab position. Removing
    // and re-inserting here made same-second sessions swap places on click.
    return sessions.map((item, index) =>
      index === existingIndex ? next : item,
    );
  }
  return orderTerminalSessions([...sessions, next]);
}

function compareTerminalSessions(
  left: TerminalSession,
  right: TerminalSession,
): number {
  return (
    left.created_at_unix - right.created_at_unix ||
    left.terminal_id.localeCompare(right.terminal_id)
  );
}

function orderTerminalSessions(sessions: TerminalSession[]): TerminalSession[] {
  return [...sessions].sort(compareTerminalSessions);
}

function reconcileTerminalSessions(
  current: TerminalSession[],
  listed: TerminalSession[],
): TerminalSession[] {
  const listedById = new Map(listed.map((item) => [item.terminal_id, item]));
  const retained = current.flatMap((item) => {
    const updated = listedById.get(item.terminal_id);
    if (!updated) return [];
    listedById.delete(item.terminal_id);
    return [updated];
  });
  return [...retained, ...orderTerminalSessions([...listedById.values()])];
}

function restoreCandidates(
  sessions: TerminalSession[],
  rememberedId: string | null,
  selectedAgentId: string,
): TerminalSession[] {
  const remembered = sessions.find((item) => item.terminal_id === rememberedId);
  const available = sessions.filter(
    (item) => !item.operator_attached && item !== remembered,
  );
  available.sort((left, right) => {
    const leftPreferred = left.agent_id === selectedAgentId ? 1 : 0;
    const rightPreferred = right.agent_id === selectedAgentId ? 1 : 0;
    return (
      rightPreferred - leftPreferred ||
      right.created_at_unix - left.created_at_unix
    );
  });
  return remembered ? [remembered, ...available] : available;
}

function websocketUrl(path: string): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}${path}`;
}

function terminalCreatedTime(createdAtUnix: number): string {
  return new Date(createdAtUnix * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function TerminalPage({
  agents,
  selectedAgentId,
  onSelectAgent,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const activeSessionRef = useRef<TerminalSession | null>(null);
  const selectedAgentIdRef = useRef(selectedAgentId);
  const lastTerminalSizeRef = useRef({ rows: 0, cols: 0 });
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const [session, setSession] = useState<TerminalSession | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("idle");
  const [sessionTitles, setSessionTitles] = useState<Record<string, string>>(
    {},
  );
  const [operatorId] = useState(terminalOperatorId);

  activeSessionRef.current = session;
  selectedAgentIdRef.current = selectedAgentId;

  const selectedAgent = agents.find(
    (agent) => agent.agent_id === selectedAgentId,
  );
  const selectedAgentSessionCount = sessions.filter(
    (item) => item.agent_id === selectedAgentId,
  ).length;
  const selectedAgentSessionLimit =
    selectedAgent?.capability_options?.terminal?.max_sessions ?? 2;
  const agentAtSessionLimit =
    selectedAgentSessionCount >= selectedAgentSessionLimit;
  const canRequestStart =
    selectedAgent?.connected &&
    selectedAgent.capabilities.includes("terminal") &&
    connection !== "connecting";

  const detachTransport = useCallback(() => {
    const socket = socketRef.current;
    socketRef.current = null;
    socket?.close();
  }, []);

  const sendResize = useCallback(() => {
    const terminal = terminalRef.current;
    const socket = socketRef.current;
    if (!terminal || socket?.readyState !== WebSocket.OPEN) return;
    if (
      lastTerminalSizeRef.current.rows === terminal.rows &&
      lastTerminalSizeRef.current.cols === terminal.cols
    ) {
      return;
    }
    lastTerminalSizeRef.current = {
      rows: terminal.rows,
      cols: terminal.cols,
    };
    socket.send(
      JSON.stringify({
        type: "resize",
        rows: terminal.rows,
        cols: terminal.cols,
      }),
    );
  }, []);

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
      setSessions((current) =>
        mergeTerminalSession(current, {
          ...nextSession,
          operator_attached: true,
        }),
      );
      window.sessionStorage.setItem(
        REMEMBERED_TERMINAL_KEY,
        nextSession.terminal_id,
      );
      setConnection("connecting");
      const socket = new WebSocket(websocketUrl(nextSession.websocket_url));
      socket.binaryType = "arraybuffer";
      socketRef.current = socket;

      socket.onopen = () => {
        socket.send(
          JSON.stringify({
            type: "attach",
            attachment_token: nextSession.attachment_token,
            operator_id: operatorId,
          }),
        );
        fitRef.current?.fit();
        sendResize();
      };
      socket.onmessage = (event) => {
        if (socketRef.current !== socket) return;
        if (typeof event.data !== "string") {
          terminalRef.current?.write(new Uint8Array(event.data as ArrayBuffer));
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
            window.requestAnimationFrame(() => {
              fitRef.current?.fit();
              lastTerminalSizeRef.current = { rows: 0, cols: 0 };
              sendResize();
              terminalRef.current?.focus();
            });
          } else if (control.type === "exited") {
            setConnection("exited");
            terminalRef.current?.writeln(
              `\r\n[process exited${control.exit_code == null ? "" : ` ${control.exit_code}`}]`,
            );
          } else if (control.type === "error") {
            setConnection("exited");
            terminalRef.current?.writeln(
              `\r\n[terminal error: ${control.message ?? "unknown error"}]`,
            );
          }
        } catch {
          terminalRef.current?.writeln("\r\n[invalid terminal control frame]");
        }
      };
      socket.onerror = () => {
        if (socketRef.current !== socket) return;
        terminalRef.current?.writeln("\r\n[terminal transport error]");
      };
      socket.onclose = () => {
        if (socketRef.current !== socket) return;
        socketRef.current = null;
        setSessions((current) =>
          current.map((item) =>
            item.terminal_id === nextSession.terminal_id
              ? { ...item, operator_attached: false }
              : item,
          ),
        );
        setConnection((current) => {
          if (current === "exited") return current;
          return "disconnected";
        });
      };
    },
    [detachTransport, operatorId, sendResize],
  );

  useEffect(() => {
    if (!hostRef.current) return;
    const terminal = new XTerm({
      cursorBlink: true,
      convertEol: false,
      fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace',
      fontSize: 14,
      lineHeight: 1.1,
      // xterm uses this width for its internal scrollbar as well as the
      // overview ruler. Reserve a larger touch target on coarse pointers.
      overviewRuler: {
        width: window.matchMedia("(pointer: coarse)").matches ? 22 : 16,
      },
      scrollback: 5000,
      theme: {
        background: "#0b1117",
        foreground: "#e5e7eb",
        cursor: "#6ee7a8",
        scrollbarSliderBackground: "#52617099",
        scrollbarSliderHoverBackground: "#6b7c8dcc",
        scrollbarSliderActiveBackground: "#8294a6",
      },
      allowProposedApi: true, // this for unicode11 addon. FOR SOME reason xtermjs doesnt tell me this in the readme.
    });
    terminal.loadAddon(new Unicode11Addon());
    terminal.unicode.activeVersion = "11";
    terminal.loadAddon(new UnicodeGraphemesAddon());
    terminal.loadAddon(new WebLinksAddon());
    const fit = new FitAddon();
    terminal.loadAddon(new ClipboardAddon());
    terminal.loadAddon(fit);
    terminal.open(hostRef.current);
    terminal.attachCustomKeyEventHandler((event) => {
      const copiesTerminalSelection =
        event.type === "keydown" &&
        event.ctrlKey &&
        event.shiftKey &&
        event.code === "KeyC";
      if (!copiesTerminalSelection) return true;

      // Chromium reserves Ctrl+Shift+C for DevTools. Other clipboard shortcuts
      // fall through to xterm's native copy/paste event handlers.
      event.preventDefault();
      event.stopPropagation();
      if (terminal.hasSelection()) {
        if (!navigator.clipboard) {
          toast.error("Clipboard access requires HTTPS or localhost");
        } else {
          void navigator.clipboard
            .writeText(terminal.getSelection())
            .catch((error) =>
              toast.error("Clipboard access was denied", {
                description: String(error),
              }),
            );
        }
      }
      return false;
    });
    const titleChange = terminal.onTitleChange((title) => {
      const terminalId = activeSessionRef.current?.terminal_id;
      if (!terminalId) return;
      const normalized = title.trim();
      setSessionTitles((current) => {
        if (current[terminalId] === normalized) return current;
        if (!normalized) {
          const next = { ...current };
          delete next[terminalId];
          return next;
        }
        return { ...current, [terminalId]: normalized };
      });
    });
    fit.fit();
    terminalRef.current = terminal;
    fitRef.current = fit;

    async function restoreTerminalSession() {
      try {
        const listed = await listTerminals();
        if (cancelled) return;
        setSessions(orderTerminalSessions(listed));
        const rememberedId = window.sessionStorage.getItem(
          REMEMBERED_TERMINAL_KEY,
        );
        const candidates = restoreCandidates(
          listed,
          rememberedId,
          selectedAgentIdRef.current,
        );

        for (const candidate of candidates) {
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
        setConnection("idle");
      } catch (error) {
        if (cancelled) return;
        setConnection("idle");
        toast.error("Could not restore terminal session", {
          description: String(error),
        });
      }
    }

    let cancelled = false;
    void document.fonts.ready.then(() => {
      if (cancelled) return;
      fit.fit();
      lastTerminalSizeRef.current = { rows: 0, cols: 0 };
      sendResize();
    });

    const input = terminal.onData((data) => {
      const socket = socketRef.current;
      if (socket?.readyState === WebSocket.OPEN) {
        socket.send(new TextEncoder().encode(data));
      }
    });
    let resizeFrame = 0;
    let lastHostWidth = 0;
    let lastHostHeight = 0;
    const resizeObserver = new ResizeObserver(([entry]) => {
      const width = Math.round(entry.contentRect.width);
      const height = Math.round(entry.contentRect.height);
      if (width === lastHostWidth && height === lastHostHeight) return;
      lastHostWidth = width;
      lastHostHeight = height;
      window.cancelAnimationFrame(resizeFrame);
      resizeFrame = window.requestAnimationFrame(() => {
        fit.fit();
        sendResize();
      });
    });
    resizeObserver.observe(hostRef.current);
    void restoreTerminalSession();
    const sessionRefresh = window.setInterval(() => {
      void listTerminals()
        .then((listed) => {
          if (!cancelled) {
            setSessions((current) =>
              reconcileTerminalSessions(current, listed),
            );
          }
        })
        .catch(() => {
          // The attached terminal transport remains authoritative while a
          // background list refresh is temporarily unavailable.
        });
    }, 5000);

    return () => {
      cancelled = true;
      input.dispose();
      titleChange.dispose();
      resizeObserver.disconnect();
      window.cancelAnimationFrame(resizeFrame);
      window.clearInterval(sessionRefresh);
      detachTransport();
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
    };
  }, [connect, detachTransport, sendResize]);

  async function start() {
    if (!selectedAgentId || !terminalRef.current) return;
    if (agentAtSessionLimit) {
      toast.error("Terminal session limit reached", {
        description: `${selectedAgent ? displayAgentLabel(selectedAgent) : selectedAgentId} already reached the active session limit. Close one before opening another.`,
      });
      return;
    }
    const previousSession = activeSessionRef.current;
    detachTransport();
    if (previousSession) {
      setSessions((current) =>
        current.map((item) =>
          item.terminal_id === previousSession.terminal_id
            ? { ...item, operator_attached: false }
            : item,
        ),
      );
    }
    setConnection("connecting");
    terminalRef.current.reset();
    try {
      fitRef.current?.fit();
      const created = await createTerminal(
        selectedAgentId,
        terminalRef.current.rows,
        terminalRef.current.cols,
      );
      connect(created);
    } catch (error) {
      setSession(null);
      activeSessionRef.current = null;
      setConnection("idle");
      toast.error("Could not start terminal", { description: String(error) });
    }
  }

  async function activateSession(nextSession: TerminalSession) {
    if (nextSession.terminal_id === session?.terminal_id) return;
    if (nextSession.operator_attached) return;

    const previousSession = activeSessionRef.current;
    detachTransport();
    if (previousSession) {
      setSessions((current) =>
        current.map((item) =>
          item.terminal_id === previousSession.terminal_id
            ? { ...item, operator_attached: false }
            : item,
        ),
      );
    }
    setSession(null);
    activeSessionRef.current = null;
    setConnection("connecting");
    terminalRef.current?.reset();
    window.sessionStorage.setItem(
      REMEMBERED_TERMINAL_KEY,
      nextSession.terminal_id,
    );

    try {
      const attached = await attachTerminal(
        nextSession.terminal_id,
        operatorId,
      );
      connect(attached);
    } catch (error) {
      setConnection("idle");
      toast.error("Could not attach terminal session", {
        description: String(error),
      });
      void listTerminals()
        .then((listed) =>
          setSessions((current) => reconcileTerminalSessions(current, listed)),
        )
        .catch(() => undefined);
    }
  }

  async function reconnect() {
    if (!session) return;
    detachTransport();
    try {
      terminalRef.current?.reset();
      setConnection("connecting");
      const attached = await attachTerminal(session.terminal_id, operatorId);
      connect(attached);
    } catch (error) {
      setConnection("disconnected");
      toast.error("Terminal session is no longer available", {
        description: String(error),
      });
    }
  }

  async function closeSession(closingSession: TerminalSession) {
    const closingId = closingSession.terminal_id;
    const closesActiveSession =
      closingId === activeSessionRef.current?.terminal_id;
    const remaining = sessions.filter((item) => item.terminal_id !== closingId);
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
      toast.error("Terminal cleanup failed", { description: String(error) });
      void listTerminals()
        .then((listed) =>
          setSessions((current) => reconcileTerminalSessions(current, listed)),
        )
        .catch(() => undefined);
      return;
    }

    setSessions((current) =>
      current.filter((item) => item.terminal_id !== closingId),
    );
    setSessionTitles((current) => {
      const next = { ...current };
      delete next[closingId];
      return next;
    });
    if (window.sessionStorage.getItem(REMEMBERED_TERMINAL_KEY) === closingId) {
      window.sessionStorage.removeItem(REMEMBERED_TERMINAL_KEY);
    }
    if (closesActiveSession) {
      setSession(null);
      activeSessionRef.current = null;
      setConnection("idle");
      // xterm.clear() deliberately preserves the active cursor line. Closing
      // a session should discard its complete screen and terminal modes.
      terminalRef.current?.reset();
      if (fallback) void activateSession(fallback);
    }
  }

  return (
    <section className="terminal-page" aria-label="Remote terminal">
      <header className="terminal-toolbar">
        <div className="terminal-heading">
          <Terminal className="size-5 text-primary" aria-hidden />
          <h1>Remote Terminal</h1>
        </div>

        <div className="terminal-controls">
          <AgentSelector
            agents={agents.filter(
              (agent) =>
                agent.connected && agent.capabilities.includes("terminal"),
            )}
            value={selectedAgentId}
            onChange={onSelectAgent}
            disabled={connection === "connecting"}
            className="w-full min-w-0 sm:w-64"
          />
        </div>
      </header>

      {!selectedAgent && connection === "idle" ? (
        <p className="terminal-notice">
          No connected terminal-capable agent is selected.
        </p>
      ) : selectedAgent && !selectedAgent.capabilities.includes("terminal") ? (
        <p className="terminal-notice">
          This agent has not enabled remote terminal capability.
        </p>
      ) : null}

      <div className="terminal-frame">
        <div className="terminal-framebar">
          <div className="terminal-tabs">
            <div
              className="terminal-tab-list"
              role="tablist"
              aria-label="Terminal sessions"
            >
              {sessions.length === 0 ? (
                <div className="terminal-session-label">
                  <Terminal className="size-4" aria-hidden />
                  <span>No active session</span>
                </div>
              ) : (
                sessions.map((item) => {
                  const agent = agents.find(
                    (candidate) => candidate.agent_id === item.agent_id,
                  );
                  const active = item.terminal_id === session?.terminal_id;
                  const locked = item.operator_attached && !active;
                  const agentLabel = agent
                    ? displayAgentLabel(agent)
                    : item.agent_id;
                  const sessionTitle = sessionTitles[item.terminal_id];
                  const tabLabel = sessionTitle
                    ? `${agentLabel} · ${sessionTitle}`
                    : agentLabel;
                  return (
                    <div
                      key={item.terminal_id}
                      role="presentation"
                      className="terminal-tab"
                      data-active={active || undefined}
                      data-locked={locked || undefined}
                    >
                      <button
                        type="button"
                        role="tab"
                        aria-selected={active}
                        className="terminal-tab-select"
                        disabled={locked || connection === "connecting"}
                        title={
                          locked ? "Attached in another browser" : tabLabel
                        }
                        onClick={() => void activateSession(item)}
                      >
                        <span
                          className="terminal-tab-dot"
                          data-state={
                            active
                              ? connection
                              : locked
                                ? "attached"
                                : item.agent_attached
                                  ? "detached"
                                  : "disconnected"
                          }
                          aria-hidden
                        />
                        <span className="terminal-tab-label">{tabLabel}</span>
                        <time
                          dateTime={new Date(
                            item.created_at_unix * 1000,
                          ).toISOString()}
                        >
                          {terminalCreatedTime(item.created_at_unix)}
                        </time>
                      </button>
                      <button
                        type="button"
                        className="terminal-tab-close"
                        disabled={locked}
                        aria-label={`Close ${agentLabel} terminal session`}
                        title={
                          locked
                            ? "Attached in another browser"
                            : "Close session"
                        }
                        onClick={() => void closeSession(item)}
                      >
                        <X className="size-3.5" aria-hidden />
                      </button>
                    </div>
                  );
                })
              )}
            </div>
            <Tooltip>
              <TooltipTrigger>
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  className="terminal-new-tab"
                  disabled={!canRequestStart}
                  aria-label="New terminal session"
                  onClick={start}
                >
                  <Plus className="size-4" aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {agentAtSessionLimit
                  ? `${selectedAgentSessionLimit}-session limit reached`
                  : "New session"}
              </TooltipContent>
            </Tooltip>
          </div>

          <div className="terminal-frame-actions">
            <Badge
              variant="outline"
              className="terminal-status"
              data-state={connection}
            >
              <span aria-hidden />
              {connection}
            </Badge>
            {connection === "disconnected" ? (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={reconnect}
              >
                <RotateCcw className="size-4" aria-hidden />
                Reconnect
              </Button>
            ) : null}
            <Tooltip>
              <TooltipTrigger>
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  disabled={connection === "idle"}
                  aria-label="Clear terminal"
                  onClick={() => terminalRef.current?.clear()}
                >
                  <Eraser className="size-4" aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Clear terminal</TooltipContent>
            </Tooltip>
          </div>
        </div>
        <div className="terminal-surface" ref={hostRef} />
      </div>
    </section>
  );
}
