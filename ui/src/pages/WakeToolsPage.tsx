import { useState } from "react";
import { toast } from "sonner";
import { Zap, Inbox } from "lucide-react";

import { runCommand, type Agent } from "@/api";
import { AgentSelector } from "@/components/AgentSelector";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";

type WakeEvent = {
  ts: number;
  target: string;
  agentId: string;
  outcome: string;
  detail: string;
};

type Props = {
  agents: Agent[];
  selectedAgentId: string;
  onSelectAgent: (agentId: string) => void;
  onAfterWake?: () => Promise<void>;
};

export function WakeToolsPage({
  agents,
  selectedAgentId,
  onSelectAgent,
  onAfterWake,
}: Props) {
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const [recentWakes, setRecentWakes] = useState<WakeEvent[]>([]);

  async function handleWake(e?: React.FormEvent) {
    e?.preventDefault();
    const trimmed = target.trim();
    if (!trimmed || !selectedAgentId || busy) return;

    setBusy(true);
    try {
      const response = await runCommand(selectedAgentId, "wake", trimmed);
      const detail = JSON.stringify(response);
      const ok = detail.includes('"ok"') || detail.includes('"success"');
      setRecentWakes((prev) =>
        [
          {
            ts: Date.now(),
            target: trimmed,
            agentId: selectedAgentId,
            outcome: ok ? "ok" : "fail",
            detail,
          },
          ...prev,
        ].slice(0, 20),
      );
      if (ok) {
        toast.success(`Woke ${trimmed}`);
      } else {
        toast.error(`Wake failed: ${trimmed}`, { description: detail });
      }
      if (onAfterWake) await onAfterWake();
      setTarget("");
    } catch (err) {
      setRecentWakes((prev) =>
        [
          {
            ts: Date.now(),
            target: trimmed,
            agentId: selectedAgentId,
            outcome: "error",
            detail: String(err),
          },
          ...prev,
        ].slice(0, 20),
      );
      toast.error("Wake failed", { description: String(err) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(20rem,1fr)]">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Zap className="size-5 text-primary" />
            Wake Target
          </CardTitle>
          <CardDescription>
            Send a Wake-on-LAN packet by hostname, IP, or MAC address
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <label htmlFor="wake-agent" className="grid gap-1.5 text-sm text-muted-foreground">
            <span>Agent</span>
            <AgentSelector
              agents={agents}
              value={selectedAgentId}
              onChange={onSelectAgent}
            />
          </label>

          <form onSubmit={handleWake} className="grid gap-3">
            <label htmlFor="wake-target" className="grid gap-1.5 text-sm text-muted-foreground">
              <span>Target</span>
              <Input
                id="wake-target"
                value={target}
                onChange={(e) => setTarget(e.target.value)}
                placeholder="bedroom-pc, 192.168.1.100, or aa:bb:cc:dd:ee:ff"
                disabled={busy}
              />
            </label>
            <Button
              type="submit"
              disabled={!selectedAgentId || !target.trim() || busy}
              className="gap-2"
            >
              <Zap className="size-4" />
              {busy ? "Sending..." : "Wake"}
            </Button>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between gap-2 gap-y-0">
          <CardTitle>Recent Wakes</CardTitle>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setRecentWakes([])}
          >
            Clear
          </Button>
        </CardHeader>
        <CardContent>
          {recentWakes.length > 0 ? (
            <div className="grid gap-0">
              {recentWakes.map((event, i) => (
                <div
                  className={`flex items-start justify-between gap-3 py-2.5 ${i < recentWakes.length - 1 ? "border-b border-border/50" : ""}`}
                  key={`${event.ts}-${event.target}-${event.agentId}`}
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium">
                        {event.target}
                      </span>
                      <Badge
                        variant={
                          event.outcome === "ok" ? "secondary" : "destructive"
                        }
                        className="text-[0.6rem] px-1.5 py-0"
                      >
                        {event.outcome}
                      </Badge>
                    </div>
                    <p className="text-[0.65rem] text-muted-foreground/70">
                      {new Date(event.ts).toLocaleString()} via {event.agentId}
                    </p>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center py-8 text-center">
              <Inbox className="size-8 text-muted-foreground/40" />
              <p className="mt-2 text-sm text-muted-foreground">
                No wake actions yet
              </p>
            </div>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
