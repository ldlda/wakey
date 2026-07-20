import { Plus, Terminal, X } from "lucide-react";

import type { Agent, TerminalSession } from "@/api";
import { displayAgentLabel } from "@/components/AgentSelector";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { TerminalConnectionState } from "@/pages/terminal/sessionUtils";

type Props = {
  agents: Agent[];
  sessions: TerminalSession[];
  activeTerminalId?: string;
  connection: TerminalConnectionState;
  sessionTitles: Record<string, string>;
  nowUnix: number;
  canRequestStart: boolean;
  agentAtSessionLimit: boolean;
  selectedAgentSessionLimit: number;
  onActivate: (session: TerminalSession) => void;
  onClose: (session: TerminalSession) => void;
  onStart: () => void;
};

function terminalCreatedTime(createdAtUnix: number): string {
  return new Date(createdAtUnix * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function terminalTimestamp(timestampUnix: number): string {
  return new Date(timestampUnix * 1000).toLocaleString();
}

function terminalExpiryDisplay(
  expiresAtUnix: number | null | undefined,
  nowUnix: number,
): { label: string; warning: boolean } | null {
  if (expiresAtUnix == null) return null;
  const remainingSeconds = expiresAtUnix - nowUnix;
  if (remainingSeconds <= 0) return { label: "expired", warning: true };
  if (remainingSeconds < 60) return { label: "<1m left", warning: true };
  if (remainingSeconds < 3600) {
    return {
      label: `${Math.floor(remainingSeconds / 60)}m left`,
      warning: remainingSeconds < 10 * 60,
    };
  }
  if (remainingSeconds < 24 * 3600) {
    return {
      label: `${Math.floor(remainingSeconds / 3600)}h left`,
      warning: false,
    };
  }
  return {
    label: `${Math.floor(remainingSeconds / (24 * 3600))}d left`,
    warning: false,
  };
}

export function TerminalSessionTabs({
  agents,
  sessions,
  activeTerminalId,
  connection,
  sessionTitles,
  nowUnix,
  canRequestStart,
  agentAtSessionLimit,
  selectedAgentSessionLimit,
  onActivate,
  onClose,
  onStart,
}: Props) {
  return (
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
            const active = item.terminal_id === activeTerminalId;
            const locked = item.operator_attached && !active;
            const agentLabel = agent ? displayAgentLabel(agent) : item.agent_id;
            const sessionTitle = sessionTitles[item.terminal_id];
            const tabLabel = sessionTitle
              ? `${agentLabel} · ${sessionTitle}`
              : agentLabel;
            const expiry = terminalExpiryDisplay(item.expires_at_unix, nowUnix);
            const timingTitle =
              item.expires_at_unix != null
                ? `Expires ${terminalTimestamp(item.expires_at_unix)}`
                : "No expiry";
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
                  title={`${locked ? "Attached in another browser\n" : ""}${tabLabel}\nCreated ${terminalTimestamp(item.created_at_unix)}\n${timingTitle}`}
                  onClick={() => onActivate(item)}
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
                    className={
                      expiry?.warning ? "text-amber-400" : "text-[#7f8d9a]"
                    }
                    dateTime={new Date(
                      (item.expires_at_unix ?? item.created_at_unix) * 1000,
                    ).toISOString()}
                  >
                    {expiry?.label ?? terminalCreatedTime(item.created_at_unix)}
                  </time>
                </button>
                <button
                  type="button"
                  className="terminal-tab-close"
                  disabled={locked}
                  aria-label={`Close ${agentLabel} terminal session`}
                  title={
                    locked ? "Attached in another browser" : "Close session"
                  }
                  onClick={() => onClose(item)}
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
            onClick={onStart}
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
  );
}
