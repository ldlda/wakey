import type { Agent } from "@/api";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";

type Props = {
  agents: Agent[];
  value: string;
  onChange: (agentId: string) => void;
  disabled?: boolean;
  className?: string;
};

export function displayAgentLabel(agent: Agent): string {
  const nickname = agent.nickname?.trim();
  return nickname ? nickname : agent.agent_id;
}

export function AgentSelector({
  agents,
  value,
  onChange,
  disabled,
  className,
}: Props) {
  const selected = agents.find((a) => a.agent_id === value);

  return (
    <Select
      value={value}
      onValueChange={(v) => {
        if (v) onChange(v);
      }}
      disabled={disabled}
    >
      <SelectTrigger className={className ?? "w-full min-w-0"}>
        <span className="flex min-w-0 flex-1 items-center gap-2 text-start">
          {selected ? (
            <span
              className={`size-2 shrink-0 rounded-full ${selected.connected ? "bg-emerald-500" : "bg-zinc-400"}`}
              aria-hidden
            />
          ) : null}
          <span className="min-w-0 truncate">
            {value
              ? selected
                ? displayAgentLabel(selected)
                : value
              : "Select agent"}
          </span>
          {selected ? (
            <span className="shrink-0 text-xs text-muted-foreground">
              {selected.connected ? "connected" : "offline"}
            </span>
          ) : null}
        </span>
      </SelectTrigger>
      <SelectContent
        className="max-w-[min(92vw,30rem)]"
        alignItemWithTrigger={false}
      >
        {agents.map((agent) => (
          <SelectItem
            key={agent.agent_id}
            value={agent.agent_id}
            className="pe-10"
            disabled={!agent.connected}
          >
            <span className="flex min-w-0 flex-1 items-center gap-2">
              <span
                className={`size-2 shrink-0 rounded-full ${agent.connected ? "bg-emerald-500" : "bg-zinc-400"}`}
                aria-hidden
              />
              <span className="min-w-0 truncate">
                {displayAgentLabel(agent)}
              </span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {agent.connected ? "connected" : "offline"}
              </span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
