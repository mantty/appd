import assert from "node:assert/strict";
import test from "node:test";

test("web shims preserve the Worker request and response shapes", async () => {
  const { Headers, Request, Response } = await import("../../src/network/fetch.mjs");
  const { ReadableStream } = await import("../../src/streams/web.mjs");
  const { URL } = await import("../../src/network/url.mjs");
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

test("Workers builtin registration preserves the Worker environment", async () => {
  globalThis.__appd_env = { FLAG: "enabled" };
  const workers = await import("../../src/builtins/cloudflare-workers.mjs?registry");
  assert.equal(workers.env.FLAG, "enabled");
  assert.equal(typeof globalThis.process.getBuiltinModule("node:events").EventEmitter, "function");
  assert.equal(typeof workers.WebSocketPair, "function");
});

test("Readable emits end after pushed data", async () => {
  const { Readable } = await import("../../src/streams/node.mjs?end-event");
  const events = [];
  const stream = new Readable();
  stream.on("data", (value) => events.push(`data:${value}`));
  stream.on("end", () => events.push("end"));
  stream.push("value");
  stream.push(null);
  assert.deepEqual(events, ["data:value", "end"]);

  const bufferedEvents = [];
  const buffered = new Readable();
  buffered.push("buffered");
  buffered.push(null);
  buffered.on("end", () => bufferedEvents.push("end"));
  buffered.on("data", (value) => bufferedEvents.push(`data:${value}`));
  assert.deepEqual(bufferedEvents, ["data:buffered", "end"]);

  const ended = new Readable();
  const lateValues = [];
  ended.on("data", (value) => lateValues.push(value));
  ended.push(null);
  assert.equal(ended.push("late"), false);
  assert.deepEqual(lateValues, []);
});

test("WebSocketPair delivers Worker messages to the native bridge", async () => {
  const { WebSocketPair } = await import("../../src/builtins/cloudflare-workers.mjs?websocket");
  const pair = new WebSocketPair();
  const client = pair[0];
  const server = pair[1];
  server.accept();
  server.addEventListener("message", (event) => server.send(`pong ${event.data}`));

  server.__appd_receive("ping 42", false);

  assert.deepEqual(client.__appd_outbox, [{ type: "message", binary: false, data: "pong ping 42" }]);
});
