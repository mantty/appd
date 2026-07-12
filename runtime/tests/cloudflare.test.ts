import assert from "node:assert/strict";
import test from "node:test";

import { env, setEnvironment } from "../src/cloudflare.js";

void test("keeps imported environment bindings live", () => {
  setEnvironment({ VALUE: "first" });
  assert.equal(env.VALUE, "first");

  setEnvironment({ VALUE: "second" });
  assert.equal(env.VALUE, "second");
  assert.deepEqual(Object.keys(env), ["VALUE"]);
  assert.equal("VALUE" in env, true);
});
