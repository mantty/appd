import "bare-encoding/global";
import "bare-stream/global";
import "bare-structured-clone/global";

import abort from "bare-abort-controller";
import Buffer from "bare-buffer";
import crypto from "bare-crypto/web";
import fetch from "bare-fetch";
import URL from "bare-url";

import { RuntimeResponse } from "./response.js";
import { installAesGcm } from "./crypto.js";
import * as nodeAssert from "./node-compat/assert.js";
import * as nodeAssertStrict from "./node-compat/assert-strict.js";
import * as nodeBuffer from "./node-compat/buffer.js";
import * as nodeChildProcess from "./node-compat/child-process.js";
import * as nodeConsole from "./node-compat/console.js";
import * as nodeConstants from "./node-compat/constants.js";
import * as nodeCrypto from "./node-compat/crypto.js";
import * as nodeDiagnosticsChannel from "./node-compat/diagnostics-channel.js";
import * as nodeDns from "./node-compat/dns.js";
import * as nodeDnsPromises from "./node-compat/dns-promises.js";
import * as nodeEvents from "./node-compat/events.js";
import * as nodeHttp from "./node-compat/http.js";
import * as nodeHttps from "./node-compat/https.js";
import * as nodeModule from "./node-compat/module.js";
import * as nodeNet from "./node-compat/net.js";
import * as nodeOs from "./node-compat/os.js";
import * as nodePath from "./node-compat/path.js";
import * as nodePathPosix from "./node-compat/path-posix.js";
import * as nodePathWin32 from "./node-compat/path-win32.js";
import * as nodePunycode from "./node-compat/punycode.js";
import * as nodeQuerystring from "./node-compat/querystring.js";
import * as nodeStream from "./node-compat/stream.js";
import * as nodeStreamConsumers from "./node-compat/stream-consumers.js";
import * as nodeStreamDuplex from "./node-compat/stream-duplex.js";
import * as nodeStreamPassThrough from "./node-compat/stream-passthrough.js";
import * as nodeStreamPromises from "./node-compat/stream-promises.js";
import * as nodeStreamReadable from "./node-compat/stream-readable.js";
import * as nodeStreamTransform from "./node-compat/stream-transform.js";
import * as nodeStreamWeb from "./node-compat/stream-web.js";
import * as nodeStreamWritable from "./node-compat/stream-writable.js";
import * as nodeStringDecoder from "./node-compat/string-decoder.js";
import * as nodeTest from "./node-compat/test.js";
import * as nodePerfHooks from "./node-compat/perf-hooks.js";
import * as nodeTimers from "./node-compat/timers.js";
import * as nodeTimersPromises from "./node-compat/timers-promises.js";
import * as nodeTls from "./node-compat/tls.js";
import * as nodeUrl from "./node-compat/url.js";
import * as nodeUtil from "./node-compat/util.js";
import * as nodeUtilTypes from "./node-compat/util-types.js";
import * as nodeZlib from "./node-compat/zlib.js";
import { builtin, type BuiltinNamespace } from "./node-compat/registry.js";
import { WebSocketPair } from "./websocket.js";
import { CompressionStream, DecompressionStream } from "./web-compression.js";
import { MessageChannel, MessageEvent, MessagePort } from "./message-channel.js";
import { caches } from "./cache.js";
import { EventSource } from "./event-source.js";
import { scheduler } from "./scheduler.js";
import { performance } from "./performance.js";

const processEnvironment: Record<string, string> = {};
const processVersions = Object.freeze({
  acorn: "",
  ada: "",
  ares: "",
  brotli: "",
  cjs_module_lexer: "",
  cldr: "",
  icu: "",
  llhttp: "",
  modules: "",
  napi: "",
  nbytes: "",
  ncrypto: "",
  node: "22.19.0",
  openssl: "",
  simdjson: "",
  simdutf: "",
  sqlite: "",
  tz: "",
  undici: "",
  unicode: "",
  uv: "",
  uvwasi: "",
  v8: "",
  zlib: "",
  zstd: "",
});
interface BuiltinModule {
  readonly default: unknown;
}

const modules: Readonly<Record<BuiltinNamespace, BuiltinModule>> = {
  assert: nodeAssert,
  assertStrict: nodeAssertStrict,
  buffer: nodeBuffer,
  childProcess: nodeChildProcess,
  console: nodeConsole,
  constants: nodeConstants,
  crypto: nodeCrypto,
  diagnosticsChannel: nodeDiagnosticsChannel,
  dns: nodeDns,
  dnsPromises: nodeDnsPromises,
  events: nodeEvents,
  http: nodeHttp,
  https: nodeHttps,
  module: nodeModule,
  net: nodeNet,
  os: nodeOs,
  perfHooks: nodePerfHooks,
  path: nodePath,
  pathPosix: nodePathPosix,
  pathWin32: nodePathWin32,
  punycode: nodePunycode,
  querystring: nodeQuerystring,
  shim: { default: unavailableBuiltin },
  stream: nodeStream,
  streamConsumers: nodeStreamConsumers,
  streamDuplex: nodeStreamDuplex,
  streamPassThrough: nodeStreamPassThrough,
  streamPromises: nodeStreamPromises,
  streamReadable: nodeStreamReadable,
  streamTransform: nodeStreamTransform,
  streamWeb: nodeStreamWeb,
  streamWritable: nodeStreamWritable,
  stringDecoder: nodeStringDecoder,
  test: nodeTest,
  timers: nodeTimers,
  timersPromises: nodeTimersPromises,
  tls: nodeTls,
  url: nodeUrl,
  util: nodeUtil,
  utilTypes: nodeUtilTypes,
  zlib: nodeZlib,
};
export const processShim = {
  arch: "x64",
  argv: ["workerd"],
  cwd: (): string => "/bundle",
  env: processEnvironment,
  getBuiltinModule(name: string): unknown {
    const implementation = builtin(name);
    if (implementation === undefined) return undefined;
    return implementation.namespace === undefined ? processShim : modules[implementation.namespace].default;
  },
  nextTick(callback: (...arguments_: unknown[]) => void, ...arguments_: unknown[]): void {
    queueMicrotask(() => { callback(...arguments_); });
  },
  pid: 1,
  platform: "linux",
  release: Object.freeze({ headersUrl: "", lts: true, name: "node", sourceUrl: "" }),
  umask: (): number => 0o022,
  version: "v22.19.0",
  versions: processVersions,
};
const consoleShim = Object.assign({}, console);
installAesGcm(crypto as unknown as Parameters<typeof installAesGcm>[0]);
installGetSetCookie();

function unavailableBuiltin(): never {
  throw new Error("This Node API is not implemented");
}

Object.assign(globalThis, {
  AbortController: abort.AbortController,
  AbortSignal: abort.AbortSignal,
  Buffer,
  caches,
  CompressionStream,
  DecompressionStream,
  EventSource,
  Headers: fetch.Headers,
  MessageChannel,
  MessageEvent,
  MessagePort,
  Request: fetch.Request,
  Response: RuntimeResponse,
  URL,
  URLSearchParams: URL.URLSearchParams,
  WebSocketPair,
  atob: Buffer.atob,
  btoa: Buffer.btoa,
  crypto,
  console: consoleShim,
  fetch,
  process: processShim,
  performance,
  scheduler,
});

function installGetSetCookie(): void {
  const prototype = fetch.Headers.prototype as unknown as BareHeaders;
  if (prototype.getSetCookie !== undefined) return;
  Object.defineProperty(prototype, "getSetCookie", {
    value(this: BareHeaders): string[] {
      return this._headers?.get("set-cookie")?.slice() ?? [];
    },
  });
}

export function setProcessEnvironment(environment: Readonly<Record<string, unknown>>): void {
  for (const key of Object.keys(processEnvironment)) Reflect.deleteProperty(processEnvironment, key);
  for (const [key, value] of Object.entries(environment)) {
    if (value === undefined) continue;
    processEnvironment[key] = typeof value === "string" ? value : JSON.stringify(value);
  }
}

interface BareHeaders {
  readonly _headers?: ReadonlyMap<string, string[]>;
  getSetCookie?: () => string[];
}
