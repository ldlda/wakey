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
        <form className="grid gap-3" onSubmit={submit}>
          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>Agent</span>
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
                  <SelectItem
                    key={agent.agent_id}
                    value={agent.agent_id}
                    disabled={!agent.connected}
                  >
                    <span className="flex min-w-0 flex-1 items-center gap-2">
                      <span
                        className={`size-2 shrink-0 rounded-full ${agent.connected ? "bg-emerald-500" : "bg-zinc-400"}`}
                        aria-hidden
                      />
                      <span className="min-w-0 truncate">{agent.agent_id}</span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {agent.connected ? "connected" : "offline"}
                      </span>
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>

          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>Command</span>
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

          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>Query</span>
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

        <Textarea
          className="mt-3 max-h-80 min-h-48 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs"
          value={output}
          readOnly
        />
      </CardContent>
    </Card>
  );
}
