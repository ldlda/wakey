import { useState } from "react";
import { Terminal } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { AgentSelector } from "@/components/AgentSelector";
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
      toast.success(`Command "${kind}" completed`);
      if (onAfterCommand) await onAfterCommand();
    } catch (err) {
      setOutput(String(err));
      toast.error("Command failed", { description: String(err) });
    } finally {
      setRunning(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Terminal className="size-5 text-primary" />
          Command Runner
        </CardTitle>
        <CardDescription>
          Debug tool: send raw commands to agents and inspect JSON responses
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form className="grid gap-3" onSubmit={submit}>
          <label htmlFor="cmd-agent" className="grid gap-1 text-sm text-muted-foreground">
            <span>Agent</span>
            <AgentSelector
              agents={agents}
              value={selectedAgentId}
              onChange={onSelectAgent}
            />
          </label>

          <label htmlFor="cmd-kind" className="grid gap-1 text-sm text-muted-foreground">
            <span>Command</span>
            <Select
              value={kind}
              onValueChange={(value) => setKind(value as CommandKind)}
            >
              <SelectTrigger id="cmd-kind" className="w-full">
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

          <label htmlFor="cmd-query" className="grid gap-1 text-sm text-muted-foreground">
            <span>Query</span>
            <Input
              id="cmd-query"
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
