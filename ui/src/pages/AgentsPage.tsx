import type { Agent } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { useState } from "react";
import { toast } from "sonner";
import { Bot, Inbox } from "lucide-react";
import { displayAgentLabel } from "@/components/AgentSelector";

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
  const [drafts, setDrafts] = useState<Record<string, string>>({});

  async function onRevoke(agentId: string) {
    if (!window.confirm(`Revoke agent credentials for ${agentId}?`)) {
      return;
    }
    setBusy(true);
    try {
      await onRevokeAgent(agentId);
    } catch (err) {
      toast.error("Revoke failed", { description: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function onSaveNickname(agent: Agent) {
    const draft = drafts[agent.agent_id] ?? agent.nickname ?? "";
    setBusy(true);
    try {
      await onSetAgentNickname(agent.agent_id, draft);
    } catch (err) {
      toast.error("Failed to update nickname", { description: String(err) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Bot className="size-5 text-primary" />
          Agents
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid gap-2">
          {agents.map((agent) => (
            <div
              className={`rounded-lg border px-4 py-3 transition-colors ${
                selectedAgentId === agent.agent_id
                  ? "border-primary/40 bg-primary/5"
                  : "bg-card"
              }`}
              key={agent.agent_id}
            >
              <div className="flex items-start justify-between gap-3">
                <button
                  type="button"
                  className="flex min-w-0 flex-1 flex-col items-start gap-1 text-left"
                  onClick={() => onSelectAgent(agent.agent_id)}
                  disabled={busy}
                >
                  <span className="flex items-center gap-2">
                    <span
                      className={`size-2 shrink-0 rounded-full ${agent.connected ? "bg-emerald-500" : "bg-zinc-400"}`}
                      aria-hidden
                    />
                    <span className="text-sm font-medium">
                      {displayAgentLabel(agent)}
                    </span>
                    <Badge
                      variant={agent.connected ? "secondary" : "outline"}
                      className="text-[0.6rem] px-1.5 py-0"
                    >
                      {agent.connected ? "connected" : "offline"}
                    </Badge>
                  </span>
                  <code className="text-[0.65rem] text-muted-foreground/70 font-mono truncate max-w-full">
                    {agent.agent_id}
                  </code>
                </button>

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

              <div className="mt-2 flex items-center gap-2">
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
                  className="flex-1"
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void onSaveNickname(agent)}
                  disabled={busy}
                >
                  Save
                </Button>
              </div>
            </div>
          ))}
          {!agents.length && (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <Inbox className="size-10 text-muted-foreground/30" />
              <p className="mt-3 text-sm text-muted-foreground">
                No agents enrolled
              </p>
              <p className="text-xs text-muted-foreground/70">
                Issue a token and enroll an agent to get started
              </p>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
