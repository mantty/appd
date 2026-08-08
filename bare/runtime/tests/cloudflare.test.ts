import assert from "node:assert/strict";
import test from "node:test";

import {
  DurableObject,
  env,
  RpcTarget,
  setEnvironment,
  setWorkerExports,
  WorkerEntrypoint,
} from "../src/cloudflare.js";
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

void test("routes ctx.exports through named WorkerEntrypoints", async () => {
  class Greeter extends WorkerEntrypoint<{ VALUE: string }, { greeting: string }> {
    greet(name: string): string {
      return `${this.ctx.props?.greeting ?? "Hello"}, ${name} from ${this.env.VALUE}`;
    }
  }

  class FailingEntryPoint extends WorkerEntrypoint {
    fail(): never {
      throw new Error("appd failure");
    }
  }

  setEnvironment({ VALUE: "binding" });
  setWorkerExports({ FailingEntryPoint, Greeter });
  const context = new RequestContext();
  const exports = context.exports as {
    readonly Greeter: {
      greet(name: string): Promise<string>;
    } & ((options: { props: { greeting: string } }) => {
      greet(name: string): Promise<string>;
    });
    readonly FailingEntryPoint: {
      fail(): Promise<never>;
    };
  };

  assert.equal(await exports.Greeter.greet("World"), "Hello, World from binding");
  assert.equal(await exports.Greeter({ props: { greeting: "Welcome" } }).greet("World"), "Welcome, World from binding");
  await assert.rejects(exports.FailingEntryPoint.fail(), /appd failure/);
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
