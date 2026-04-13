import type { Agent } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { useState } from "react";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onRevokeAgent: (agentId: string) => Promise<boolean>;
  onSetAgentNickname: (
    agentId: string,
    nickname: string | null,
  ) => Promise<boolean>;
};

export function AgentsPage({
  agents,
  selectedAgentId,
  onSelectAgent,
  onRevokeAgent,
  onSetAgentNickname,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [drafts, setDrafts] = useState<Record<string, string>>({});

  function displayLabel(agent: Agent): string {
    const nickname = agent.nickname?.trim();
    return nickname ? nickname : agent.agent_id;
  }

  async function onRevoke(agentId: string) {
    if (!window.confirm(`Revoke agent credentials for ${agentId}?`)) {
      return;
    }
    setBusy(true);
    setStatus("");
    setError("");
    try {
      const revoked = await onRevokeAgent(agentId);
      setStatus(
        revoked ? `Revoked ${agentId}` : `${agentId} was already absent`,
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onSaveNickname(agent: Agent) {
    const draft = drafts[agent.agent_id] ?? agent.nickname ?? "";
    setBusy(true);
    setStatus("");
    setError("");
    try {
      const updated = await onSetAgentNickname(agent.agent_id, draft);
      setStatus(
        updated
          ? `Updated nickname for ${agent.agent_id}`
          : `${agent.agent_id} not found`,
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Agents</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid gap-2">
          {agents.map((agent) => (
            <div
              className="flex items-start justify-between gap-3 rounded-md border bg-card px-3 py-2"
              key={agent.agent_id}
            >
              <Button
                variant={
                  selectedAgentId === agent.agent_id ? "secondary" : "outline"
                }
                className="flex h-auto flex-1 items-center justify-between gap-3 px-3 py-2 text-left"
                onClick={() => onSelectAgent(agent.agent_id)}
                disabled={busy}
              >
                <span className="min-w-0 truncate">{displayLabel(agent)}</span>
                <span className="text-xs text-muted-foreground">
                  {agent.connected ? "connected" : "offline"}
                </span>
              </Button>
              <div className="grid min-w-52 gap-2">
                <Input
                  value={drafts[agent.agent_id] ?? agent.nickname ?? ""}
                  onChange={(e) =>
                    setDrafts((prev) => ({
                      ...prev,
                      [agent.agent_id]: e.target.value,
                    }))
                  }
                  placeholder="nickname (optional)"
                  disabled={busy}
                />
                <div className="flex items-center justify-end gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void onSaveNickname(agent)}
                    disabled={busy}
                  >
                    Save Name
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="shrink-0 border-destructive/50 text-destructive hover:bg-destructive/10"
                    onClick={() => void onRevoke(agent.agent_id)}
                    disabled={busy}
                  >
                    Revoke
                  </Button>
                </div>
              </div>
            </div>
          ))}
          {!agents.length && (
            <div className="px-1 py-2 text-sm text-muted-foreground">
              No agents enrolled
            </div>
          )}
        </div>
        {status && (
          <pre className="max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 text-xs">
            {status}
          </pre>
        )}
        {error && (
          <pre className="max-h-80 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
            {error}
          </pre>
        )}
      </CardContent>
    </Card>
  );
}
