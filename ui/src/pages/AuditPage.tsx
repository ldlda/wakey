import type { AuditEvent } from "@/api";

type Props = {
  events: AuditEvent[];
  onRefresh: () => Promise<void>;
};

export function AuditPage({ events, onRefresh }: Props) {
  return (
    <section className="card span-full">
      <div className="row-head">
        <h2>Recent Audit Events</h2>
        <button onClick={() => void onRefresh()}>Refresh</button>
      </div>
      <pre className="output">{JSON.stringify(events, null, 2)}</pre>
    </section>
  );
}
