import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Key, Copy, Inbox } from "lucide-react";

import {
  type EnrollTokenStatus,
  fetchEnrollTokens,
  issueEnrollToken,
  revokeEnrollToken,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";

function formatUnix(unix: number): string {
  return new Date(unix * 1000).toLocaleString();
}

const TTL_PRESETS = [
  { label: "1 hour", seconds: 3600 },
  { label: "12 hours", seconds: 43200 },
  { label: "1 day", seconds: 86400 },
  { label: "7 days", seconds: 604800 },
] as const;

export function TokensPage() {
  const [tokens, setTokens] = useState<EnrollTokenStatus[]>([]);
  const [ttlSeconds, setTtlSeconds] = useState(86400);
  const [customTtl, setCustomTtl] = useState("");
  const [useCustom, setUseCustom] = useState(false);
  const [busy, setBusy] = useState(false);
  const [lastIssued, setLastIssued] = useState<{
    token: string;
    expires: string;
  } | null>(null);

  const activeCount = useMemo(
    () => tokens.filter((t) => !t.expired).length,
    [tokens],
  );

  async function load() {
    setBusy(true);
    try {
      const next = await fetchEnrollTokens();
      setTokens(next);
    } catch (err) {
      toast.error("Failed to load tokens", { description: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function onIssue() {
    setBusy(true);
    try {
      const effectiveTtl = useCustom
        ? Number.parseInt(customTtl, 10) || 86400
        : ttlSeconds;
      const issued = await issueEnrollToken(
        Number.isFinite(effectiveTtl) ? effectiveTtl : 86400,
      );
      setLastIssued({
        token: issued.enroll_token,
        expires: formatUnix(issued.expires_at_unix),
      });
      toast.success("Token issued", {
        description: `Expires ${formatUnix(issued.expires_at_unix)}`,
      });
      await load();
    } catch (err) {
      toast.error("Failed to issue token", { description: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function onRevoke(token: string) {
    setBusy(true);
    try {
      const result = await revokeEnrollToken(token);
      if (result.revoked) {
        toast.success(`Revoked ${token}`);
      } else {
        toast.info(`${token} was already absent`);
      }
      await load();
    } catch (err) {
      toast.error("Failed to revoke token", { description: String(err) });
    } finally {
      setBusy(false);
    }
  }

  async function copyToClipboard(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied to clipboard");
    } catch {
      toast.error("Copy failed");
    }
  }

  useEffect(() => {
    void load();
  }, []);

  const controlPlaneUrl =
    window.location.origin.replace(/:\d+$/, "") || "https://cp.example.com";

  return (
    <section className="grid gap-4 xl:grid-cols-2">
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Key className="size-5 text-primary" />
              Enroll Tokens
            </CardTitle>
            <CardDescription className="mt-1">
              {activeCount} active, {tokens.length} shown
            </CardDescription>
          </div>
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
          <div className="grid gap-2">
            {tokens.map((token) => (
              <div
                className="flex items-start justify-between gap-3 rounded-md border bg-card px-3 py-2"
                key={token.enroll_token}
              >
                <div className="min-w-0">
                  <strong className="block truncate text-sm">
                    {token.enroll_token}
                  </strong>
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
              <div className="flex flex-col items-center justify-center py-8 text-center">
                <Inbox className="size-8 text-muted-foreground/40" />
                <p className="mt-2 text-sm text-muted-foreground">
                  No tokens found
                </p>
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      <div className="grid gap-4 content-start">
        <Card>
          <CardHeader>
            <CardTitle>Issue Token</CardTitle>
            <CardDescription>
              Create a new enrollment token for agents
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            <div>
              <p className="mb-2 text-sm text-muted-foreground">
                Token lifetime
              </p>
              <div className="flex flex-wrap gap-2">
                {TTL_PRESETS.map((preset) => (
                  <Button
                    key={preset.seconds}
                    variant={
                      !useCustom && ttlSeconds === preset.seconds
                        ? "secondary"
                        : "outline"
                    }
                    size="sm"
                    onClick={() => {
                      setTtlSeconds(preset.seconds);
                      setUseCustom(false);
                    }}
                  >
                    {preset.label}
                  </Button>
                ))}
                <Button
                  variant={useCustom ? "secondary" : "outline"}
                  size="sm"
                  onClick={() => setUseCustom(true)}
                >
                  Custom
                </Button>
              </div>
              {useCustom && (
                <label className="mt-2 grid gap-1 text-sm text-muted-foreground">
                  <span>TTL (seconds)</span>
                  <Input
                    type="number"
                    min={1}
                    value={customTtl}
                    onChange={(e) => setCustomTtl(e.target.value)}
                    placeholder="86400"
                  />
                </label>
              )}
            </div>
            <Button onClick={() => void onIssue()} disabled={busy}>
              Issue Token
            </Button>
          </CardContent>
        </Card>

        {/* Install command after issuing */}
        {lastIssued && (
          <Card className="border-primary/30 bg-primary/5">
            <CardHeader>
              <CardTitle className="text-base">Agent Installation</CardTitle>
              <CardDescription>
                Token: <code className="text-xs">{lastIssued.token}</code>
                <br />
                Expires: {lastIssued.expires}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-2">
              <p className="text-xs text-muted-foreground">
                Run this on the target device to enroll:
              </p>
              <div className="relative">
                <pre className="overflow-auto rounded-md border bg-muted/60 p-3 pr-10 font-mono text-xs">
                  {`wakey-agent enroll \\
  --server-url ${controlPlaneUrl} \\
  --token ${lastIssued.token}`}
                </pre>
                <Button
                  variant="ghost"
                  size="sm"
                  className="absolute right-1.5 top-1.5 size-7 p-0"
                  onClick={() =>
                    void copyToClipboard(
                      `wakey-agent enroll --server-url ${controlPlaneUrl} --token ${lastIssued.token}`,
                    )
                  }
                >
                  <Copy className="size-3.5" />
                </Button>
              </div>
            </CardContent>
          </Card>
        )}
      </div>
    </section>
  );
}
