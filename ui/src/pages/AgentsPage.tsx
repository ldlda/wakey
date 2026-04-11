import type { Agent } from "@/api";

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
};

export function AgentsPage({ agents, selectedAgentId, onSelectAgent }: Props) {
  return (
    <section className="card">
      <h2>Agents</h2>
      <div className="list">
        {agents.map((agent) => (
          <button
            key={agent.agent_id}
            className={`row ${selectedAgentId === agent.agent_id ? "selected" : ""}`}
            onClick={() => onSelectAgent(agent.agent_id)}
          >
            <span>{agent.agent_id}</span>
            <span>{agent.connected ? "connected" : "offline"}</span>
          </button>
        ))}
        {!agents.length && <div className="empty">No agents enrolled</div>}
      </div>
    </section>
  );
}
