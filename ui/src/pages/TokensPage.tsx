import { useEffect, useMemo, useState } from "react";

import {
  type EnrollTokenStatus,
  fetchEnrollTokens,
  issueEnrollToken,
  revokeEnrollToken,
} from "@/api";

function formatUnix(unix: number): string {
  return new Date(unix * 1000).toLocaleString();
}

export function TokensPage() {
  const [tokens, setTokens] = useState<EnrollTokenStatus[]>([]);
  const [includeExpired, setIncludeExpired] = useState(false);
  const [ttlSeconds, setTtlSeconds] = useState("86400");
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const activeCount = useMemo(
    () => tokens.filter((t) => !t.expired).length,
    [tokens],
  );

  async function load() {
    setBusy(true);
    setError("");
    try {
      const next = await fetchEnrollTokens(includeExpired);
      setTokens(next);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onIssue() {
    setBusy(true);
    setError("");
    setStatus("");
    try {
      const ttl = Number.parseInt(ttlSeconds, 10);
      const issued = await issueEnrollToken(Number.isFinite(ttl) ? ttl : 86400);
      setStatus(
        `Issued ${issued.enroll_token} (expires ${formatUnix(issued.expires_at_unix)})`,
      );
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function onRevoke(token: string) {
    setBusy(true);
    setError("");
    setStatus("");
    try {
      const result = await revokeEnrollToken(token);
      setStatus(
        result.revoked ? `Revoked ${token}` : `${token} was already absent`,
      );
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void load();
  }, [includeExpired]);

  return (
    <section className="two-col">
      <div className="card">
        <div className="row-head">
          <h2>Enroll Tokens</h2>
          <button onClick={() => void load()} disabled={busy}>
            Refresh
          </button>
        </div>
        <div className="form">
          <label>
            Include expired
            <select
              value={includeExpired ? "yes" : "no"}
              onChange={(e) => setIncludeExpired(e.target.value === "yes")}
            >
              <option value="no">No</option>
              <option value="yes">Yes</option>
            </select>
          </label>
        </div>
        <p className="muted">
          {activeCount} active, {tokens.length} shown
        </p>
        <div className="list">
          {tokens.map((token) => (
            <div className="row" key={token.enroll_token}>
              <div>
                <strong>{token.enroll_token}</strong>
                <div className="muted">
                  expires: {formatUnix(token.expires_at_unix)}
                </div>
              </div>
              <div className="token-actions">
                <span className={`pill ${token.expired ? "error" : "ready"}`}>
                  {token.expired ? "expired" : "active"}
                </span>
                <button
                  onClick={() => void onRevoke(token.enroll_token)}
                  disabled={busy}
                >
                  Revoke
                </button>
              </div>
            </div>
          ))}
          {!tokens.length && <div className="empty">No tokens found</div>}
        </div>
      </div>

      <div className="card">
        <h2>Issue Token</h2>
        <div className="form">
          <label>
            TTL seconds
            <input
              type="number"
              min={1}
              value={ttlSeconds}
              onChange={(e) => setTtlSeconds(e.target.value)}
            />
          </label>
          <button onClick={() => void onIssue()} disabled={busy}>
            Issue
          </button>
        </div>
        {status && <pre className="output">{status}</pre>}
        {error && <pre className="error">{error}</pre>}
      </div>
    </section>
  );
}
