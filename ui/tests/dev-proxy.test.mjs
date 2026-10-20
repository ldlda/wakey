import assert from "node:assert/strict";
import { createServer as createHttpServer, request } from "node:http";
import { once } from "node:events";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createServer } from "vite";

test("dev proxy preserves API requests and WebSocket upgrades under /ui base", async () => {
  const sockets = new Set();
  const backend = createHttpServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    res.setHeader("Content-Type", "application/json");
    res.end(
      JSON.stringify({
        method: req.method,
        url: req.url,
        body: Buffer.concat(chunks).toString(),
        host: req.headers.host,
      }),
    );
  });
  backend.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  backend.on("upgrade", (req, socket) => {
    socket.write(
      "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
    );
  });
  backend.listen(0, "127.0.0.1");
  await once(backend, "listening");
  const target = `http://127.0.0.1:${backend.address().port}`;
  const previous = process.env.WAKEY_DEV_API_TARGET;
  process.env.WAKEY_DEV_API_TARGET = target;
  let vite;
  try {
    vite = await createServer({
      root: fileURLToPath(new URL("../", import.meta.url)),
      server: { port: 0 },
    });
    await vite.listen();
    const origin = `http://127.0.0.1:${vite.httpServer.address().port}`;
    const response = await fetch(
      `${origin}/api/v1/control/fleet/devices/refresh?test=1`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: '{"agent_ids":[]}',
      },
    );
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      method: "POST",
      url: "/api/v1/control/fleet/devices/refresh?test=1",
      body: '{"agent_ids":[]}',
      host: new URL(target).host,
    });
    assert.equal((await fetch(`${origin}/healthz`)).status, 200);
    const page = await fetch(`${origin}/ui/`);
    assert.equal(page.status, 200);
    assert.match(await page.text(), /@vite\/client/);
    await new Promise((resolve, reject) => {
      const req = request(
        `${origin}/api/v1/control/terminals/events/ws?test=1`,
        {
          headers: {
            Connection: "Upgrade",
            Upgrade: "websocket",
            "Sec-WebSocket-Version": "13",
            "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==",
          },
        },
      );
      req.on("upgrade", (res, socket) => {
        socket.destroy();
        resolve();
      });
      req.on("response", (res) => {
        res.resume();
        reject(new Error(`Expected WS upgrade, got ${res.statusCode}`));
      });
      req.on("error", reject);
      req.setTimeout(5000, () =>
        req.destroy(new Error("WS upgrade timed out")),
      );
      req.end();
    });
  } finally {
    if (previous === undefined) delete process.env.WAKEY_DEV_API_TARGET;
    else process.env.WAKEY_DEV_API_TARGET = previous;
    for (const socket of sockets) socket.destroy();
    if (vite) await vite.close();
    await new Promise((resolve) => backend.close(resolve));
  }
});
