import type { Agent } from "@/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
};

export function AgentsPage({ agents, selectedAgentId, onSelectAgent }: Props) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Agents</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid gap-2">
          {agents.map((agent) => (
            <Button
              key={agent.agent_id}
              variant={
                selectedAgentId === agent.agent_id ? "secondary" : "outline"
              }
              className="flex h-auto w-full items-center justify-between px-3 py-2 text-left"
              onClick={() => onSelectAgent(agent.agent_id)}
            >
              <span>{agent.agent_id}</span>
              <span className="text-xs text-muted-foreground">
                {agent.connected ? "connected" : "offline"}
              </span>
            </Button>
          ))}
          {!agents.length && (
            <div className="px-1 py-2 text-sm text-muted-foreground">
              No agents enrolled
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
