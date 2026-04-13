import type { Agent } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useState } from "react";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onRevokeAgent: (agentId: string) => Promise<boolean>;
};

export function AgentsPage({
  agents,
  selectedAgentId,
  onSelectAgent,
  onRevokeAgent,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");

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
                className="flex h-auto flex-1 items-center justify-between px-3 py-2 text-left"
                onClick={() => onSelectAgent(agent.agent_id)}
                disabled={busy}
              >
                <span>{agent.agent_id}</span>
                <span className="text-xs text-muted-foreground">
                  {agent.connected ? "connected" : "offline"}
                </span>
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
