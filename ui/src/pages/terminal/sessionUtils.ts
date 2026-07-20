import type { Terminal as XTerm } from "@xterm/xterm";

import type { TerminalSession } from "@/api";

export type TerminalConnectionState =
  | "idle"
  | "connecting"
  | "ready"
  | "disconnected"
  | "exited";

export type TerminalModifiers = {
  ctrl: boolean;
  meta: boolean;
};

export const NO_TERMINAL_MODIFIERS: TerminalModifiers = {
  ctrl: false,
  meta: false,
};

export function mergeTerminalSession(
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

export function orderTerminalSessions(
  sessions: TerminalSession[],
): TerminalSession[] {
  return [...sessions].sort(compareTerminalSessions);
}

export function reconcileTerminalSessions(
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

export function restoreCandidates(
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

export function applyTerminalModifiers(
  data: string,
  modifiers: TerminalModifiers,
): string {
  let modified = data;
  if (modifiers.ctrl && modified.length === 1) {
    const code = modified.toUpperCase().charCodeAt(0);
    if (code >= 64 && code <= 95) {
      modified = String.fromCharCode(code & 0x1f);
    }
  }
  return modifiers.meta ? `\x1b${modified}` : modified;
}

export function visibleTerminalText(terminal: XTerm): string {
  const buffer = terminal.buffer.active;
  const firstLine = buffer.viewportY;
  const lastLine = Math.min(buffer.length, firstLine + terminal.rows);
  const lines: string[] = [];
  for (let index = firstLine; index < lastLine; index += 1) {
    lines.push(buffer.getLine(index)?.translateToString(true) ?? "");
  }
  while (lines.at(-1) === "") lines.pop();
  return lines.join("\n");
}
