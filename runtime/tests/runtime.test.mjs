import assert from "node:assert/strict";
import test from "node:test";

const fsModule = new URL("../qjs/fs.mjs", import.meta.url);

async function freshFs(name) {
  globalThis.__appd_tmp = new Map();
  globalThis.__appd_tmp_directories = new Set(["/tmp"]);
  return import(`${fsModule.href}?${name}`);
}

test("fs keeps files inside one request-owned /tmp", async () => {
  const first = await freshFs("first");
  first.mkdirSync("/tmp/data", { recursive: true });
  first.writeFileSync("/tmp/data/value.txt", "hello");
  first.appendFileSync("/tmp/data/value.txt", " world");

  assert.equal(first.readFileSync("/tmp/data/value.txt", "utf8"), "hello world");
  assert.deepEqual(first.readdirSync("/tmp/data"), ["value.txt"]);
  assert.equal(first.statSync("/tmp/data/value.txt").isFile(), true);
  assert.throws(() => first.readFileSync("/bundle/worker.mjs"), /ENOENT/);
  assert.throws(() => first.readFileSync("/tmp/../bundle"), /ENOENT/);

  const second = await freshFs("second");
  assert.equal(second.existsSync("/tmp/data/value.txt"), false);
});

test("fs callback and promise forms use the synchronous core", async () => {
  const fs = await freshFs("async");
  await new Promise((resolve, reject) => {
    fs.writeFile("/tmp/value.txt", "one", (error) => error ? reject(error) : resolve());
  });
  await fs.promises.appendFile("/tmp/value.txt", " two");
  assert.equal(await fs.promises.readFile("/tmp/value.txt", "utf8"), "one two");
  await fs.promises.rename("/tmp/value.txt", "/tmp/renamed.txt");
  assert.equal(fs.existsSync("/tmp/value.txt"), false);
  assert.equal(fs.realpathSync("/tmp/renamed.txt"), "/tmp/renamed.txt");
});

test("web shims preserve the Worker request and response shapes", async () => {
  const { Headers, Request, Response, ReadableStream, URL } = await import("../qjs/web.mjs");
  const headers = new Headers([["X-Test", "one"], ["x-test", "two"]]);
  assert.equal(headers.get("x-test"), "one, two");
  assert.deepEqual([...headers], [["x-test", "one, two"]]);

  const request = new Request("https://example.test/path", { method: "post", body: "payload" });
  assert.equal(request.method, "POST");
  assert.equal(await request.text(), "payload");

  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue("hello");
      controller.enqueue(" world");
      controller.close();
    },
  });
  const response = new Response(stream, { status: 201, headers });
  assert.equal(response.status, 201);
  assert.equal(await response.text(), "hello world");
  assert.equal(new URL("/next", request.url).href, "https://example.test/next");
});

test("Workers builtin registration exposes the fs facade", async () => {
  globalThis.__appd_env = { FLAG: "enabled" };
  const workers = await import("../qjs/cloudflare-workers.mjs?registry");
  assert.equal(workers.env.FLAG, "enabled");
  assert.equal(typeof globalThis.process.getBuiltinModule("node:fs").writeFileSync, "function");
  assert.equal(typeof workers.WebSocketPair, "function");
});

test("WebSocketPair delivers Worker messages to the native bridge", async () => {
  const { WebSocketPair } = await import("../qjs/cloudflare-workers.mjs?websocket");
  const pair = new WebSocketPair();
  const client = pair[0];
  const server = pair[1];
  server.accept();
  server.addEventListener("message", (event) => server.send(`pong ${event.data}`));

  server.__appd_receive("ping 42", false);

  assert.deepEqual(client.__appd_outbox, [{ type: "message", binary: false, data: "pong ping 42" }]);
});
