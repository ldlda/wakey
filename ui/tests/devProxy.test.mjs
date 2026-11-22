import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer as createHttpServer, request } from "node:http";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { createServer as createViteServer } from "vite";

function status(url, options) {
  return new Promise((resolve, reject) => {
    const req = request(url, options, (res) => {
      res.resume();
      resolve(res.statusCode);
    });
    req.on("upgrade", (res, socket) => {
      socket.destroy();
      resolve(res.statusCode);
    });
    req.on("error", reject);
    req.setTimeout(2_000, () => req.destroy(new Error("request timed out")));
    req.end();
  });
}

test("dev proxy only forwards trusted browser traffic", async (t) => {
  let httpRequests = 0;
  let upgrades = 0;
  const backend = createHttpServer((_req, res) => {
    httpRequests++;
    res.writeHead(204, { connection: "close" }).end();
  });
  backend.on("upgrade", (_req, socket) => {
    upgrades++;
    socket.end(
      "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
    );
  });
  backend.listen(0, "127.0.0.1");
  await once(backend, "listening");
  t.after(() => backend.close());
  const envKey = "WAKEY_DEV_API_TARGET";
  const previous = process.env[envKey];
  process.env[envKey] = `http://127.0.0.1:${backend.address().port}`;
  t.after(() => {
    if (previous === undefined) delete process.env[envKey];
    else process.env[envKey] = previous;
  });

  const server = await createViteServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    server: { port: 0, hmr: false },
    logLevel: "silent",
  });
  t.after(() => server.close());
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const url = `${origin}/api/test`;

  assert.equal(await status(url), 204);
  assert.equal(await status(url, { method: "POST", headers: { origin } }), 204);
  assert.equal(await status(url, { method: "POST" }), 403);
  assert.equal(
    await status(url, {
      method: "POST",
      headers: { origin: "http://example.test" },
    }),
    403,
  );
  assert.equal(
    await status(url, { headers: { "sec-fetch-site": "cross-site" } }),
    403,
  );
  assert.equal(await status(url, { headers: { host: "example.test" } }), 403);

  const headers = { connection: "Upgrade", upgrade: "websocket" };
  assert.equal(await status(url, { headers: { ...headers, origin } }), 101);
  assert.equal(await status(url, { headers }), 403);
  assert.equal(
    await status(url, {
      headers: { ...headers, origin: "http://example.test" },
    }),
    403,
  );
  assert.equal(httpRequests, 2);
  assert.equal(upgrades, 1);
});
