import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { runCommand, type Agent, type CommandKind } from "@/api";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onAfterCommand?: () => Promise<void>;
};

export function CommandsPage({
  agents,
  selectedAgentId,
  onSelectAgent,
  onAfterCommand,
}: Props) {
  const [kind, setKind] = useState<CommandKind>("devs");
  const [query, setQuery] = useState("");
  const [output, setOutput] = useState("Select agent and run a command");
  const [running, setRunning] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!selectedAgentId) return;
    setRunning(true);
    setOutput("Running...");
    try {
      const out = await runCommand(selectedAgentId, kind, query.trim());
      setOutput(JSON.stringify(out, null, 2));
      if (onAfterCommand) await onAfterCommand();
    } catch (err) {
      setOutput(String(err));
    } finally {
      setRunning(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Command Runner</CardTitle>
      </CardHeader>
      <CardContent>
        <form className="form" onSubmit={submit}>
          <label>
            Agent
            <Select
              value={selectedAgentId}
              onValueChange={(value) => {
                if (value) onSelectAgent(value);
              }}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Select agent" />
              </SelectTrigger>
              <SelectContent>
                {agents.map((agent) => (
                  <SelectItem key={agent.agent_id} value={agent.agent_id}>
                    {agent.agent_id} (
                    {agent.connected ? "connected" : "offline"})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>

          <label>
            Command
            <Select
              value={kind}
              onValueChange={(value) => setKind(value as CommandKind)}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Pick command" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="devs">devs</SelectItem>
                <SelectItem value="leases">leases</SelectItem>
                <SelectItem value="inventory">inventory</SelectItem>
                <SelectItem value="wake">wake</SelectItem>
              </SelectContent>
            </Select>
          </label>

          <label>
            Query
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="optional"
            />
          </label>

          <Button type="submit" disabled={running || !selectedAgentId}>
            {running ? "Running..." : "Run command"}
          </Button>
        </form>

        <Textarea className="output" value={output} readOnly />
      </CardContent>
    </Card>
  );
}
