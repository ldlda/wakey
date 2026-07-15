import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import { Eraser, PlugZap, RotateCcw, Square, Terminal } from "lucide-react";
import { toast } from "sonner";

import {
  type Agent,
  type TerminalSession,
  attachTerminal,
  closeTerminal,
  createTerminal,
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

function websocketUrl(path: string): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}${path}`;
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
  const intentionalCloseRef = useRef(false);
  const lastTerminalSizeRef = useRef({ rows: 0, cols: 0 });
  const [session, setSession] = useState<TerminalSession | null>(null);
  const [connection, setConnection] = useState<ConnectionState>("idle");

  const selectedAgent = agents.find(
    (agent) => agent.agent_id === selectedAgentId,
  );
  const canStart =
    selectedAgent?.connected &&
    selectedAgent.capabilities.includes("terminal") &&
    connection === "idle";

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
      socketRef.current?.close();
      intentionalCloseRef.current = false;
      setConnection("connecting");
      const socket = new WebSocket(websocketUrl(nextSession.websocket_url));
      socket.binaryType = "arraybuffer";
      socketRef.current = socket;

      socket.onopen = () => {
        socket.send(
          JSON.stringify({
            type: "attach",
            attachment_token: nextSession.attachment_token,
          }),
        );
        fitRef.current?.fit();
        sendResize();
      };
      socket.onmessage = (event) => {
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
        terminalRef.current?.writeln("\r\n[terminal transport error]");
      };
      socket.onclose = () => {
        socketRef.current = null;
        setConnection((current) => {
          if (intentionalCloseRef.current || current === "exited")
            return current;
          return "disconnected";
        });
      };
    },
    [sendResize],
  );

  useEffect(() => {
    if (!hostRef.current) return;
    const terminal = new XTerm({
      cursorBlink: true,
      convertEol: false,
      fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace',
      fontSize: 14,
      lineHeight: 1.1,
      scrollback: 5000,
      theme: {
        background: "#0b1117",
        foreground: "#e5e7eb",
        cursor: "#6ee7a8",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(hostRef.current);
    fit.fit();
    terminalRef.current = terminal;
    fitRef.current = fit;

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

    return () => {
      cancelled = true;
      input.dispose();
      resizeObserver.disconnect();
      window.cancelAnimationFrame(resizeFrame);
      socketRef.current?.close();
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
    };
  }, [sendResize]);

  async function start() {
    if (!selectedAgentId || !terminalRef.current) return;
    setConnection("connecting");
    terminalRef.current.clear();
    try {
      fitRef.current?.fit();
      const created = await createTerminal(
        selectedAgentId,
        terminalRef.current.rows,
        terminalRef.current.cols,
      );
      setSession(created);
      connect(created);
    } catch (error) {
      setConnection("idle");
      toast.error("Could not start terminal", { description: String(error) });
    }
  }

  async function reconnect() {
    if (!session) return;
    try {
      const attached = await attachTerminal(session.terminal_id);
      setSession(attached);
      connect(attached);
    } catch (error) {
      setConnection("exited");
      toast.error("Terminal session is no longer available", {
        description: String(error),
      });
    }
  }

  async function close() {
    if (!session) return;
    intentionalCloseRef.current = true;
    socketRef.current?.send(JSON.stringify({ type: "close" }));
    socketRef.current?.close();
    try {
      await closeTerminal(session.terminal_id);
    } catch (error) {
      toast.error("Terminal cleanup failed", { description: String(error) });
    } finally {
      setSession(null);
      setConnection("idle");
      terminalRef.current?.clear();
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
            disabled={connection !== "idle"}
            className="w-full min-w-0 sm:w-64"
          />
          {connection === "idle" ? (
            <Button type="button" onClick={start} disabled={!canStart}>
              <PlugZap className="size-4" aria-hidden />
              Connect
            </Button>
          ) : null}
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
          <div className="terminal-session-label">
            <Terminal className="size-4" aria-hidden />
            <span>
              {session && selectedAgent
                ? displayAgentLabel(selectedAgent)
                : "No active session"}
            </span>
            <Badge
              variant="outline"
              className="terminal-status"
              data-state={connection}
            >
              <span aria-hidden />
              {connection}
            </Badge>
          </div>

          <div className="terminal-frame-actions">
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
            {session ? (
              <Tooltip>
                <TooltipTrigger>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    className="text-destructive hover:text-destructive"
                    aria-label="Close terminal session"
                    onClick={close}
                  >
                    <Square className="size-3.5 fill-current" aria-hidden />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Close session</TooltipContent>
              </Tooltip>
            ) : null}
          </div>
        </div>
        <div className="terminal-surface" ref={hostRef} />
      </div>
    </section>
  );
}
