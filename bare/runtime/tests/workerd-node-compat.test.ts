import assert from "node:assert/strict";
import { test } from "node:test";

import { Miniflare } from "miniflare";

const worker = `
  import strictAssert from "node:assert/strict";
  import { spawn } from "node:child_process";
  import nodeConsole, { Console, log } from "node:console";
  import { O_RDONLY, SIGINT } from "node:constants";
  import { createHash } from "node:crypto";
  import { channel } from "node:diagnostics_channel";
  import dns, { promises as dnsPromises } from "node:dns";
  import nodeModule, { builtinModules, createRequire, isBuiltin } from "node:module";
  import net from "node:net";
  import tls from "node:tls";
  import { performance } from "node:perf_hooks";
  import utility from "node:util";
  import types from "node:util/types";
  import sys from "node:sys";
  import { gunzipSync, gzipSync } from "node:zlib";
  import os from "node:os";
  import path from "node:path";
  import process from "node:process";
  import * as url from "node:url";
  import Stream from "node:stream";

  export default {
    fetch() {
      return Response.json({
        assert: typeof strictAssert,
        path: {
          resolve: path.resolve("worker"),
          posix: typeof path.posix.join,
          win32: typeof path.win32.join,
        },
        stream: typeof Stream.Readable,
        url: {
          URL: typeof url.URL,
          URLSearchParams: typeof url.URLSearchParams,
          fileURLToPath: typeof url.fileURLToPath,
        },
        process: {
          arch: process.arch,
          argv: process.argv,
          cwd: process.cwd(),
          pid: process.pid,
          platform: process.platform,
          umask: process.umask(),
          version: process.version,
          versions: process.versions.node,
        },
        childProcess: unsupported(spawn),
        console: {
          Console: typeof Console,
          default: typeof nodeConsole,
          defaultConsole: typeof nodeConsole.Console,
          log: typeof log,
        },
        constants: { O_RDONLY, SIGINT },
        crypto: createHash("sha256").update("appd").digest("hex"),
        diagnostics: diagnostics(channel),
        dns: {
          addressConfig: dns.ADDRCONFIG,
          lookup: typeof dns.lookup,
          promises: typeof dnsPromises.lookup,
          resolveTxt: typeof dns.resolveTxt,
        },
        module: {
          assert: typeof createRequire("/bundle/worker.mjs")("node:assert"),
          builtin: builtinModules.includes("assert"),
          default: typeof nodeModule,
          isBuiltin: isBuiltin("node:assert"),
        },
        net: { connection: typeof net.createConnection, ip: net.isIP("127.0.0.1") },
        tls: {
          connection: typeof tls.connect,
          minimum: tls.DEFAULT_MIN_VERSION,
        },
        perfHooks: typeof performance.now(),
        util: { date: types.isDate(new Date()), format: utility.format("appd %s", "bare") },
        sys: sys.format("appd %s", "bare"),
        zlib: gunzipSync(gzipSync("appd")).toString(),
        os: {
          arch: os.arch(),
          cpus: os.cpus(),
          hostname: os.hostname(),
          signal: os.constants.signals.SIGINT,
          tmpdir: os.tmpdir(),
        },
      });
    },
  };

  function unsupported(method) {
    try {
      method("echo");
      return null;
    } catch (error) {
      return error.message;
    }
  }

  function diagnostics(channel) {
    const current = channel("appd");
    let received = null;
    const listener = message => { received = message };
    current.subscribe(listener);
    current.publish("appd");
    current.unsubscribe(listener);
    return received;
  }
`;

void test("pins the current workerd node-compat contract", async () => {
  const runtime = new Miniflare({
    compatibilityDate: "2026-08-05",
    compatibilityFlags: ["nodejs_compat"],
    modules: true,
    script: worker,
  });

  const response = await runtime.dispatchFetch("https://appd.test/");
  const body = await response.text();
  assert.equal(response.status, 200, body);
  const result = JSON.parse(body) as {
    assert: string;
    path: { resolve: string; posix: string; win32: string };
    stream: string;
    url: { URL: string; URLSearchParams: string; fileURLToPath: string };
  };

  assert.deepEqual(result, {
    assert: "function",
    path: { resolve: "/worker", posix: "function", win32: "function" },
    stream: "function",
    process: {
      arch: "x64",
      argv: ["workerd"],
      cwd: "/bundle",
      pid: 1,
      platform: "linux",
      umask: 0o022,
      version: "v22.19.0",
      versions: "22.19.0",
    },
    url: { URL: "function", URLSearchParams: "function", fileURLToPath: "function" },
    childProcess: "The child_process.spawn method is not implemented",
    console: { Console: "function", default: "object", defaultConsole: "function", log: "function" },
    constants: { O_RDONLY: 0, SIGINT: 2 },
    crypto: "202f02b0b11359d092bdff94a65202a11070abf00c75113b0e99f8a1ca387ceb",
    diagnostics: "appd",
    dns: { addressConfig: 1024, lookup: "function", promises: "function", resolveTxt: "function" },
    module: { assert: "function", builtin: true, default: "function", isBuiltin: true },
    net: { connection: "function", ip: 4 },
    tls: { connection: "function", minimum: "TLSv1.2" },
    perfHooks: "number",
    util: { date: true, format: "appd bare" },
    sys: "appd bare",
    zlib: "appd",
    os: { arch: "x64", cpus: [], hostname: "localhost", signal: 2, tmpdir: "/tmp/" },
  });
});
