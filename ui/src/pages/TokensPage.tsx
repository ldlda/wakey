import { useEffect, useMemo, useState } from "react";

import {
  type EnrollTokenStatus,
  fetchEnrollTokens,
  issueEnrollToken,
  revokeEnrollToken,
} from "@/api";
import { Badge } from "@/components/ui/badge";
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
    <section className="grid gap-3 xl:grid-cols-2">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
          <CardTitle>Enroll Tokens</CardTitle>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void load()}
            disabled={busy}
          >
            Refresh
          </Button>
        </CardHeader>
        <CardContent className="space-y-3">
          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>Include expired</span>
            <Select
              value={includeExpired ? "yes" : "no"}
              onValueChange={(value) => setIncludeExpired(value === "yes")}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Choose option" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="no">No</SelectItem>
                <SelectItem value="yes">Yes</SelectItem>
              </SelectContent>
            </Select>
          </label>
          <p className="text-sm text-muted-foreground">
            {activeCount} active, {tokens.length} shown
          </p>
          <div className="grid gap-2">
            {tokens.map((token) => (
              <div
                className="flex items-start justify-between gap-3 rounded-md border bg-card px-3 py-2"
                key={token.enroll_token}
              >
                <div>
                  <strong>{token.enroll_token}</strong>
                  <div className="text-xs text-muted-foreground">
                    expires: {formatUnix(token.expires_at_unix)}
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <Badge variant={token.expired ? "destructive" : "secondary"}>
                    {token.expired ? "expired" : "active"}
                  </Badge>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void onRevoke(token.enroll_token)}
                    disabled={busy}
                  >
                    Revoke
                  </Button>
                </div>
              </div>
            ))}
            {!tokens.length && (
              <div className="px-1 py-2 text-sm text-muted-foreground">
                No tokens found
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Issue Token</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-3">
          <label className="grid gap-1 text-sm text-muted-foreground">
            <span>TTL seconds</span>
            <Input
              type="number"
              min={1}
              value={ttlSeconds}
              onChange={(e) => setTtlSeconds(e.target.value)}
            />
          </label>
          <Button onClick={() => void onIssue()} disabled={busy}>
            Issue
          </Button>
          {status && (
            <pre className="max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 text-xs">
              {status}
            </pre>
          )}
          {error && (
            <pre className="max-h-80 overflow-auto rounded-md border border-destructive/60 bg-destructive/10 p-3 text-xs text-destructive">
              {error}
            </pre>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
