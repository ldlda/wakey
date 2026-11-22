import type { IncomingMessage } from "node:http";
import type { Plugin, ProxyOptions, ViteDevServer } from "vite";

const API = /^\/api\//;
const HEALTH = /^\/healthz$/;
export const DEV_HOST = "127.0.0.1";

function isAllowed(req: IncomingMessage, upgrade = false): boolean {
  const { host, origin, "sec-fetch-site": site } = req.headers;
  if (
    !host ||
    !/^(?:127\.0\.0\.1|localhost)(?::[0-9]+)?$/.test(host) ||
    (site && site !== "same-origin" && site !== "none")
  ) {
    return false;
  }
  if (origin !== undefined) return origin === `http://${host}`;

  // Same-origin GETs and local diagnostics may omit Origin. Require it for
  // writes and WebSockets, where browsers send it even on the same origin.
  return !upgrade && (req.method === "GET" || req.method === "HEAD");
}

export function createDevProxy(target: string): {
  plugin: Plugin;
  proxy: Record<string, ProxyOptions>;
} {
  let server: ViteDevServer;
  const http: ProxyOptions = { target, changeOrigin: true };
  return {
    plugin: {
      name: "wakey-dev-proxy",
      configureServer(devServer) {
        if (devServer.config.server.host !== DEV_HOST) {
          throw new Error(`Wakey's dev proxy requires --host ${DEV_HOST}`);
        }
        server = devServer;
        server.middlewares.use((req, res, next) => {
          const url = req.url ?? "";
          if ((API.test(url) || HEALTH.test(url)) && !isAllowed(req)) {
            res.writeHead(403).end();
            return;
          }
          next();
        });
      },
    },
    proxy: {
      [API.source]: {
        ...http,
        // Vite's automatic WS forwarding skips HTTP middleware.
        configure(proxy) {
          server.httpServer?.on("upgrade", (req, socket, head) => {
            if (!API.test(req.url ?? "")) return;
            if (!isAllowed(req, true)) {
              socket.end("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");
              return;
            }
            proxy.ws(req, socket, head);
          });
        },
      },
      [HEALTH.source]: http,
    },
  };
}
