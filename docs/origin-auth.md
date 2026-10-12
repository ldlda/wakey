# Origin Authentication With Caddy

Cloudflare Access protects the operator surface at the edge. An origin reachable
directly must also verify Access tokens: a nonempty assertion header is not
authentication. A read-only probe through `mails.ldlda.com`, preserving Wakey's
TLS hostname and HTTP host, returned 403 without a header and 200 with a forged
header on the same origin IP. Do not retain the old header-presence matcher.

## Configure CC

Add this to the deployed CC configuration, using your protected application's
settings, not the bypass application's audience:

```toml
bind = "127.0.0.1:23501"

[access]
issuer = "https://ldlda.cloudflareaccess.com"
audience = "0c8ca0bce40a48ddc5bc065cea8390eaddd75a84458f147bbe249fcc57c3d781"
# Optional; defaults to the issuer's /cdn-cgi/access/certs endpoint.
# jwks_url = "https://ldlda.cloudflareaccess.com/cdn-cgi/access/certs"
```

Issuer and key URLs must use HTTPS without user credentials, query or fragment.
The issuer is an origin without a trailing slash; it must exactly match the JWT
issuer. The audience is not a secret. URLs come only from trusted configuration,
never from an incoming token. `init-config --from-config` preserves `[access]`.

CC requires a loopback bind when `[access]` is configured. With no `[access]`,
the verifier returns 401; existing local development routes remain unchanged.
Changes to Access configuration require a daemon restart, not a state reload.

## Configure Caddy

Use [the Caddy template](../deploy/control-plane.Caddyfile), replacing the domain
and every upstream port with your deployment values. Its protected handler calls
CC's `GET /internal/auth/verify` through `forward_auth`, then proxies the original
request only on success. No additional service or custom Caddy module is needed.

Protect `/`, `/ui*` and `/api/v1/control/*`. This includes operator WebSocket
handshakes; established WebSocket frames are not individually authenticated and
token expiry does not automatically disconnect an existing WebSocket.

Keep `/healthz`, `/api/v1/agents/enroll`, `/api/v1/agent/ws` and
`/api/v1/agent/terminals/*` outside Access verification. They retain Wakey's own
agent/scoped-relay authentication. Cloudflare's bypass application should cover
these paths; terminal child paths must be included. If using HTTP ACME challenges,
ensure `/.well-known/acme-challenge/*` is exempt at Cloudflare; Caddy handles its
own active challenges, not CC.

Do not proxy `/internal/auth/verify` publicly and do not put `forward_auth` in
front of the verifier itself. Do not replace verification with a static header
that Caddy adds to every request. Caddy owns enforcement: direct access to the
CC port would bypass it, so loopback binding and host access controls matter.

## Verification And Cache

The verifier accepts only RS256, verifies signatures, and requires matching
issuer/audience plus expiry. A future `nbf` also rejects the token. No clock-skew
leeway is applied; keep the host clock synchronized. Duplicate assertion headers
and assertions over 16 KiB are rejected. No tokens, claims or upstream bodies
are logged.

Responses have no identity payload:

| Status | Meaning |
| --- | --- |
| 204 | Valid protected-application token |
| 401 | Missing/invalid token, unknown key after successful key retrieval, or unconfigured verifier |
| 503 | Required signing keys cannot be obtained safely |

CC reuses one HTTP client and stores parsed keys with their HTTP cache policy.
`http-cache-semantics` determines freshness from the issuer's `Cache-Control`,
`Age`, `Date` and `Expires`, rather than a built-in key TTL. For example,
`public, max-age=14400, must-revalidate, no-transform` permits four hours of
freshness minus the response's existing age; `must-revalidate` forbids stale
reuse, not every fresh reuse. No heuristic lifetime is invented when freshness
is unspecified. A `no-cache` response requires revalidation before reuse;
`no-store` retains neither keys nor policy after serving the fetching request.

Fresh matching keys verify locally. Stale policies produce conditional requests
using ETag or Last-Modified when available. A matching 304 renews the policy and
reuses parsed keys without downloading/parsing a body; mismatched or unsolicited
304 responses fail closed. An unknown key ID forces conditional revalidation
even when the stored response remains fresh, to discover rotation.

Fetches are single-flight and limited to once per minute, including failures.
Rotation immediately after a fetch may therefore take up to a minute to become
available. If the issuer requires revalidation on every use (or allows no
storage), subsequent callers inside that cooldown receive 503 rather than reuse
keys contrary to the HTTP policy. Successful 200 refresh replaces the entire
set, including removing all keys for a valid empty set. Failed refresh does not
extend freshness: matching keys may be used only while the stored policy still
permits reuse. There is no indefinitely stale fallback, including on outages.

Fetches have a five-second timeout, no redirects, a 256 KiB body limit and at
most 64 keys. These limits are internal safeguards, not new config knobs.

## Rollout

1. Install the new CC binary, add `[access]`, and restart CC. Check the loopback
   verifier returns 401 for an absent or forged assertion.
2. Update Caddy from the template and validate its configuration before reload.
   The repo template uses port 6767; this deployment uses 23501.
3. Confirm a real Access login can use the UI/control APIs and operator terminal
   WebSocket. Confirm agent connections and enrollment still work.
4. Repeat a direct-origin request with Wakey's Host/SNI. Both absent and forged
   assertions must return 401, not the UI. Verify the internal endpoint itself
   returns 404 through the public virtual host.

Do not roll back to header-only acceptance on a verification failure. A UI
build or a CC restart alone does not update the deployed Caddy policy.

References: [Cloudflare token validation](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/authorization-cookie/validating-json/)
and [Caddy forward_auth](https://caddyserver.com/docs/caddyfile/directives/forward_auth).
HTTP policy is provided by [http-cache-semantics](https://docs.rs/http-cache-semantics/latest/http_cache_semantics/struct.CachePolicy.html);
only the single-resource storage, bounded fetch and anti-hammering policy are
application-owned. No general-purpose disk or Moka response cache is needed.
