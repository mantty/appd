import assert from "node:assert/strict";
import { test } from "node:test";

import { compilerArguments } from "../src/pack-worker.js";

void test("maps node builtins from the runtime registry", () => {
  const arguments_ = compilerArguments(
    {
      compiler: "/tools/esbuild",
      output: "/app/worklet.cjs",
      worker: "/app/worker.mjs",
    },
    "/runtime",
  );

  assert.deepEqual(builtinAliases(arguments_), expectedBuiltinAliases());
  assert.ok(arguments_.includes("--alias:appd-worker=/app/worker.mjs"));
  assert.ok(arguments_.includes("--alias:cloudflare:node=/runtime/cloudflare-node.js"));
  assert.ok(arguments_.includes("--alias:cloudflare:sockets=/runtime/sockets.js"));
  assert.ok(arguments_.includes("--alias:cloudflare:workers=/runtime/cloudflare.js"));
});

function builtinAliases(arguments_: readonly string[]): Record<string, string> {
  return Object.fromEntries(
    arguments_
      .filter((argument) => argument.startsWith("--alias:"))
      .map((argument) => argument.slice("--alias:".length).split("=", 2) as [string, string])
      .filter(([name]) => !["appd-worker", "cloudflare:node", "cloudflare:sockets", "cloudflare:workers"].includes(name)),
  );
}

function expectedBuiltinAliases(): Record<string, string> {
  const expected: Record<string, string> = {};
  const add = (names: readonly string[], path: string): void => {
    for (const name of names) {
      expected[name] = `/runtime/node-compat/${path}.js`;
      expected[`node:${name}`] = `/runtime/node-compat/${path}.js`;
    }
  };
  const addShim = (names: readonly string[]): void => {
    for (const name of names) {
      expected[name] = "/runtime/node-compat/shim.js";
      expected[`node:${name}`] = "/runtime/node-compat/shim.js";
    }
  };

  add(["assert"], "assert");
  add(["assert/strict"], "assert-strict");
  add(["buffer"], "buffer");
  add(["child_process"], "child-process");
  add(["console"], "console");
  add(["constants"], "constants");
  add(["crypto"], "crypto");
  add(["diagnostics_channel"], "diagnostics-channel");
  add(["dns"], "dns");
  add(["dns/promises"], "dns-promises");
  add(["events"], "events");
  add(["http"], "http");
  add(["https"], "https");
  addShim([
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_wrap",
    "_tls_common",
    "_tls_wrap",
    "cluster",
    "dgram",
    "domain",
    "http2",
    "inspector",
    "inspector/promises",
    "readline",
    "readline/promises",
    "repl",
    "sqlite",
    "trace_events",
    "tty",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
  ]);
  add(["module"], "module");
  add(["net"], "net");
  add(["os"], "os");
  add(["perf_hooks"], "perf-hooks");
  add(["path"], "path");
  add(["path/posix"], "path-posix");
  add(["path/win32"], "path-win32");
  add(["process"], "process");
  add(["punycode"], "punycode");
  add(["querystring"], "querystring");
  add(["stream"], "stream");
  add(["stream/consumers"], "stream-consumers");
  add(["stream/promises"], "stream-promises");
  add(["stream/web"], "stream-web");
  add(["_stream_duplex"], "stream-duplex");
  add(["_stream_passthrough"], "stream-passthrough");
  add(["_stream_readable"], "stream-readable");
  add(["_stream_transform"], "stream-transform");
  add(["_stream_writable"], "stream-writable");
  add(["string_decoder"], "string-decoder");
  add(["test"], "test");
  add(["timers"], "timers");
  add(["timers/promises"], "timers-promises");
  add(["tls"], "tls");
  add(["url"], "url");
  add(["util"], "util");
  add(["util/types"], "util-types");
  add(["sys"], "util");
  add(["zlib"], "zlib");
  return expected;
}
