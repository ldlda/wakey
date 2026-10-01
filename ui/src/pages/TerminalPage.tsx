import { useEffect, useRef, useState } from "react";
import { Eraser, Keyboard, RotateCcw, Terminal } from "lucide-react";
import { toast } from "sonner";

import type { Agent } from "@/api";
import { AgentSelector, displayAgentLabel } from "@/components/AgentSelector";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { TerminalAccessoryBar } from "@/pages/terminal/TerminalAccessoryBar";
import { TerminalSessionTabs } from "@/pages/terminal/TerminalSessionTabs";
import { useTerminalSession } from "@/pages/terminal/useTerminalSession";
import { useTerminalSessions } from "@/pages/terminal/useTerminalSessions";
import { useXterm } from "@/pages/terminal/useXterm";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
};

type TerminalTouch = {
  pointerId: number;
  startX: number;
  startY: number;
  moved: boolean;
};

const TERMINAL_TOUCH_SLOP_PX = 8;

export function TerminalPage({
  agents,
  selectedAgentId,
  onSelectAgent,
}: Props) {
  const pageRef = useRef<HTMLElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalTouchRef = useRef<TerminalTouch | null>(null);
  const terminalInputFocusedBeforeAccessoryRef = useRef(false);
  const [nowUnix, setNowUnix] = useState(() => Math.floor(Date.now() / 1000));
  const [accessoriesOpen, setAccessoriesOpen] = useState(
    () => window.matchMedia("(pointer: coarse), (max-width: 640px)").matches,
  );

  const catalog = useTerminalSessions();
  const { sessions } = catalog;
  const terminal = useXterm({
    pageRef,
    hostRef,
  });
  const terminalSession = useTerminalSession({
    terminal: terminal.terminal,
    catalog,
    selectedAgentId,
  });

  useEffect(() => {
    const clock = window.setInterval(
      () => setNowUnix(Math.floor(Date.now() / 1000)),
      30_000,
    );
    return () => window.clearInterval(clock);
  }, []);

  const selectedAgent = agents.find(
    (agent) => agent.agent_id === selectedAgentId,
  );
  const selectedAgentSessionCount = sessions.filter(
    (item) => item.agent_id === selectedAgentId,
  ).length;
  const selectedAgentSessionLimit = Math.max(
    1,
    selectedAgent?.capability_options?.terminal?.max_sessions ?? 2,
  );
  const agentAtSessionLimit =
    selectedAgentSessionCount >= selectedAgentSessionLimit;
  const canRequestStart = Boolean(
    selectedAgent?.connected &&
    selectedAgent.capabilities.includes("terminal") &&
    terminalSession.connection !== "connecting" &&
    terminal.terminal.ready,
  );
  const terminalInputReady = terminalSession.connection === "ready";

  function start() {
    if (agentAtSessionLimit) {
      toast.error("Terminal session limit reached", {
        description: `${selectedAgent ? displayAgentLabel(selectedAgent) : selectedAgentId} already reached the active session limit. Close one before opening another.`,
      });
      return;
    }
    void terminalSession.start();
  }

  function toggleTerminalKeyboard() {
    const terminalInstance = terminal.terminal;
    const blurTerminalInput = () => {
      terminalInstance.blurInput();
      const activeElement = document.activeElement;
      if (
        activeElement instanceof HTMLElement &&
        hostRef.current?.contains(activeElement)
      ) {
        activeElement.blur();
      }
    };
    const shouldHide =
      terminalInputFocusedBeforeAccessoryRef.current ||
      Boolean(hostRef.current?.contains(document.activeElement));
    terminalInputFocusedBeforeAccessoryRef.current = false;
    if (shouldHide) {
      blurTerminalInput();
      // Base UI may restore the previously focused element after pointer handling.
      window.requestAnimationFrame(blurTerminalInput);
    } else {
      terminalInstance.focus();
    }
  }

  return (
    <section
      className="terminal-page"
      aria-label="Remote terminal"
      ref={pageRef}
    >
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
            disabled={terminalSession.connection === "connecting"}
            className="w-full min-w-0 sm:w-64"
          />
        </div>
      </header>

      {!selectedAgent && terminalSession.connection === "idle" ? (
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
          <TerminalSessionTabs
            agents={agents}
            sessions={sessions}
            activeTerminalId={terminalSession.session?.terminal_id}
            connection={terminalSession.connection}
            sessionTitles={terminal.sessionTitles}
            nowUnix={nowUnix}
            canRequestStart={canRequestStart}
            agentAtSessionLimit={agentAtSessionLimit}
            selectedAgentSessionLimit={selectedAgentSessionLimit}
            onActivate={(item) => void terminalSession.activate(item)}
            onClose={(item) => void terminalSession.close(item)}
            onStart={start}
          />

          <div className="terminal-frame-actions">
            <Badge
              variant="outline"
              className="terminal-status"
              data-state={terminalSession.connection}
            >
              <span aria-hidden />
              {terminalSession.connection}
            </Badge>
            {terminalSession.connection === "disconnected" ? (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => void terminalSession.reconnect()}
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
                  className="terminal-accessories-toggle"
                  data-active={accessoriesOpen || undefined}
                  aria-label={
                    accessoriesOpen
                      ? "Hide terminal controls"
                      : "Show terminal controls"
                  }
                  aria-expanded={accessoriesOpen}
                  onClick={() => setAccessoriesOpen((current) => !current)}
                >
                  <Keyboard className="size-4" aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Terminal controls</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger>
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  disabled={terminalSession.connection === "idle"}
                  aria-label="Clear terminal"
                  onClick={terminal.terminal.clear}
                >
                  <Eraser className="size-4" aria-hidden />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Clear terminal</TooltipContent>
            </Tooltip>
          </div>
        </div>
        {accessoriesOpen ? (
          <TerminalAccessoryBar
            inputReady={terminalInputReady}
            modifiers={terminalSession.modifiers}
            fontSize={terminal.fontSize}
            onRememberInputFocus={() => {
              terminalInputFocusedBeforeAccessoryRef.current = Boolean(
                hostRef.current?.contains(document.activeElement),
              );
            }}
            onToggleModifier={terminalSession.toggleModifier}
            onWriteKey={terminalSession.writeAccessoryKey}
            onScrollPages={terminal.terminal.scrollPages}
            onCopy={() => void terminal.terminal.copy()}
            onPaste={() =>
              void terminal.terminal.paste(terminalSession.writeTerminalData)
            }
            onToggleKeyboard={toggleTerminalKeyboard}
            onChangeFontSize={terminal.terminal.changeFontSize}
          />
        ) : null}
        <div
          className="terminal-surface"
          ref={hostRef}
          onPointerDown={(event) => {
            if (event.pointerType !== "touch") {
              terminal.terminal.focus();
              return;
            }
            terminalTouchRef.current = {
              pointerId: event.pointerId,
              startX: event.clientX,
              startY: event.clientY,
              moved: false,
            };
          }}
          onPointerMove={(event) => {
            const touch = terminalTouchRef.current;
            if (!touch || touch.pointerId !== event.pointerId || touch.moved) {
              return;
            }
            touch.moved =
              Math.hypot(
                event.clientX - touch.startX,
                event.clientY - touch.startY,
              ) > TERMINAL_TOUCH_SLOP_PX;
          }}
          onPointerUp={(event) => {
            const touch = terminalTouchRef.current;
            terminalTouchRef.current = null;
            if (
              touch?.pointerId === event.pointerId &&
              !touch.moved &&
              event.pointerType === "touch"
            ) {
              terminal.terminal.focus();
            }
          }}
          onPointerCancel={() => {
            terminalTouchRef.current = null;
          }}
        />
      </div>
    </section>
  );
}
