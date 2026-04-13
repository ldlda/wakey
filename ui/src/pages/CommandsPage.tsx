import { useState } from "react";

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
    <section className="card">
      <h2>Command Runner</h2>
      <form className="form" onSubmit={submit}>
        <label>
          Agent
          <select
            value={selectedAgentId}
            onChange={(e) => onSelectAgent(e.target.value)}
            required
          >
            <option value="" disabled>
              Select agent
            </option>
            {agents.map((agent) => (
              <option key={agent.agent_id} value={agent.agent_id}>
                {agent.agent_id} ({agent.connected ? "connected" : "offline"})
              </option>
            ))}
          </select>
        </label>
        <label>
          Command
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as CommandKind)}
          >
            <option value="devs">devs</option>
            <option value="leases">leases</option>
            <option value="inventory">inventory</option>
            <option value="wake">wake</option>
          </select>
        </label>
        <label>
          Query
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="optional"
          />
        </label>
        <button type="submit" disabled={running || !selectedAgentId}>
          {running ? "Running..." : "Run command"}
        </button>
      </form>
      <pre className="output">{output}</pre>
    </section>
  );
}
