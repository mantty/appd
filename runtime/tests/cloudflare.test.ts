import assert from "node:assert/strict";
import test from "node:test";

import { DurableObject, env, RpcTarget, setEnvironment, WorkerEntrypoint } from "../src/cloudflare.js";
import { RequestContext } from "../src/context.js";
import { invokeWorker } from "../src/worker.js";

void test("keeps imported environment bindings live", () => {
  setEnvironment({ VALUE: "first" });
  assert.equal(env.VALUE, "first");

  setEnvironment({ VALUE: "second" });
  assert.equal(env.VALUE, "second");
  assert.deepEqual(Object.keys(env), ["VALUE"]);
  assert.equal("VALUE" in env, true);
});

void test("constructs class entrypoints per request", async () => {
  const environment = { VALUE: "binding" };
  const context = new RequestContext();
  let receivedContext: unknown;
  let receivedEnvironment: unknown;
  class EntryPoint extends WorkerEntrypoint {
    fetch(): Response {
      receivedContext = this.ctx;
      receivedEnvironment = this.env;
      return new Response("ok");
    }
  }

  const response = await invokeWorker(
    EntryPoint,
    new Request("https://localhost/"),
    environment,
    context,
  );

  assert.equal(await response.text(), "ok");
  assert.equal(receivedContext, context);
  assert.equal(receivedEnvironment, environment);
});

void test("waitUntil drains pending work and rejects pass-through", async () => {
  const context = new RequestContext();
  let completed = false;
  context.waitUntil(Promise.resolve().then(() => {
    completed = true;
  }));

  await context.drain();

  assert.equal(completed, true);
  assert.throws(() => {
    context.passThroughOnException();
  }, /not supported/);
});

void test("rejects unsupported Cloudflare service constructors", () => {
  assert.throws(() => new DurableObject(), /not supported/);
  assert.throws(() => new RpcTarget(), /not supported/);
});
