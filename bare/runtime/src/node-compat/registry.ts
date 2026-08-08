export type BuiltinNamespace =
  | "assert"
  | "assertStrict"
  | "buffer"
  | "childProcess"
  | "console"
  | "constants"
  | "crypto"
  | "dns"
  | "dnsPromises"
  | "diagnosticsChannel"
  | "events"
  | "http"
  | "https"
  | "module"
  | "net"
  | "os"
  | "path"
  | "pathPosix"
  | "pathWin32"
  | "punycode"
  | "querystring"
  | "shim"
  | "stream"
  | "streamDuplex"
  | "streamPassThrough"
  | "streamReadable"
  | "streamTransform"
  | "streamWritable"
  | "streamConsumers"
  | "streamPromises"
  | "streamWeb"
  | "stringDecoder"
  | "test"
  | "timers"
  | "timersPromises"
  | "tls"
  | "url"
  | "util"
  | "utilTypes"
  | "perfHooks"
  | "zlib";

export interface Builtin {
  readonly namespace?: BuiltinNamespace;
  readonly path: string;
}

const assert: Builtin = { namespace: "assert", path: "node-compat/assert.js" };
const assertStrict: Builtin = { namespace: "assertStrict", path: "node-compat/assert-strict.js" };
const buffer: Builtin = { namespace: "buffer", path: "node-compat/buffer.js" };
const childProcess: Builtin = { namespace: "childProcess", path: "node-compat/child-process.js" };
const console: Builtin = { namespace: "console", path: "node-compat/console.js" };
const constants: Builtin = { namespace: "constants", path: "node-compat/constants.js" };
const crypto: Builtin = { namespace: "crypto", path: "node-compat/crypto.js" };
const dns: Builtin = { namespace: "dns", path: "node-compat/dns.js" };
const dnsPromises: Builtin = { namespace: "dnsPromises", path: "node-compat/dns-promises.js" };
const diagnosticsChannel: Builtin = {
  namespace: "diagnosticsChannel",
  path: "node-compat/diagnostics-channel.js",
};
const events: Builtin = { namespace: "events", path: "node-compat/events.js" };
const http: Builtin = { namespace: "http", path: "node-compat/http.js" };
const https: Builtin = { namespace: "https", path: "node-compat/https.js" };
const module: Builtin = { namespace: "module", path: "node-compat/module.js" };
const net: Builtin = { namespace: "net", path: "node-compat/net.js" };
const os: Builtin = { namespace: "os", path: "node-compat/os.js" };
const perfHooks: Builtin = { namespace: "perfHooks", path: "node-compat/perf-hooks.js" };
const path: Builtin = { namespace: "path", path: "node-compat/path.js" };
const pathPosix: Builtin = { namespace: "pathPosix", path: "node-compat/path-posix.js" };
const pathWin32: Builtin = { namespace: "pathWin32", path: "node-compat/path-win32.js" };
const stream: Builtin = { namespace: "stream", path: "node-compat/stream.js" };
const streamConsumers: Builtin = { namespace: "streamConsumers", path: "node-compat/stream-consumers.js" };
const streamPromises: Builtin = { namespace: "streamPromises", path: "node-compat/stream-promises.js" };
const streamWeb: Builtin = { namespace: "streamWeb", path: "node-compat/stream-web.js" };
const stringDecoder: Builtin = { namespace: "stringDecoder", path: "node-compat/string-decoder.js" };
const test: Builtin = { namespace: "test", path: "node-compat/test.js" };
const querystring: Builtin = { namespace: "querystring", path: "node-compat/querystring.js" };
const punycode: Builtin = { namespace: "punycode", path: "node-compat/punycode.js" };
const timers: Builtin = { namespace: "timers", path: "node-compat/timers.js" };
const timersPromises: Builtin = { namespace: "timersPromises", path: "node-compat/timers-promises.js" };
const tls: Builtin = { namespace: "tls", path: "node-compat/tls.js" };
const url: Builtin = { namespace: "url", path: "node-compat/url.js" };
const util: Builtin = { namespace: "util", path: "node-compat/util.js" };
const utilTypes: Builtin = { namespace: "utilTypes", path: "node-compat/util-types.js" };
const zlib: Builtin = { namespace: "zlib", path: "node-compat/zlib.js" };
const process: Builtin = { path: "node-compat/process.js" };
const shim: Builtin = { namespace: "shim", path: "node-compat/shim.js" };

const builtins: Readonly<Record<string, Builtin>> = {
  assert,
  "node:assert": assert,
  "assert/strict": assertStrict,
  "node:assert/strict": assertStrict,
  buffer,
  "node:buffer": buffer,
  "child_process": childProcess,
  "node:child_process": childProcess,
  cluster: shim,
  "node:cluster": shim,
  console,
  "node:console": console,
  constants,
  "node:constants": constants,
  crypto,
  "node:crypto": crypto,
  dns,
  "node:dns": dns,
  "dns/promises": dnsPromises,
  "node:dns/promises": dnsPromises,
  dgram: shim,
  "node:dgram": shim,
  domain: shim,
  "node:domain": shim,
  "diagnostics_channel": diagnosticsChannel,
  "node:diagnostics_channel": diagnosticsChannel,
  events,
  "node:events": events,
  "_http_agent": shim,
  "node:_http_agent": shim,
  "_http_client": shim,
  "node:_http_client": shim,
  "_http_common": shim,
  "node:_http_common": shim,
  "_http_incoming": shim,
  "node:_http_incoming": shim,
  "_http_outgoing": shim,
  "node:_http_outgoing": shim,
  "_http_server": shim,
  "node:_http_server": shim,
  http,
  "node:http": http,
  http2: shim,
  "node:http2": shim,
  https,
  "node:https": https,
  inspector: shim,
  "node:inspector": shim,
  "inspector/promises": shim,
  "node:inspector/promises": shim,
  module,
  "node:module": module,
  net,
  "node:net": net,
  os,
  "node:os": os,
  perf_hooks: perfHooks,
  "node:perf_hooks": perfHooks,
  path,
  "node:path": path,
  "path/posix": pathPosix,
  "node:path/posix": pathPosix,
  "path/win32": pathWin32,
  "node:path/win32": pathWin32,
  process,
  "node:process": process,
  punycode,
  "node:punycode": punycode,
  querystring,
  "node:querystring": querystring,
  readline: shim,
  "node:readline": shim,
  "readline/promises": shim,
  "node:readline/promises": shim,
  repl: shim,
  "node:repl": shim,
  sqlite: shim,
  "node:sqlite": shim,
  stream,
  "node:stream": stream,
  "stream/consumers": streamConsumers,
  "node:stream/consumers": streamConsumers,
  "stream/promises": streamPromises,
  "node:stream/promises": streamPromises,
  "stream/web": streamWeb,
  "node:stream/web": streamWeb,
  "_stream_duplex": { namespace: "streamDuplex", path: "node-compat/stream-duplex.js" },
  "node:_stream_duplex": { namespace: "streamDuplex", path: "node-compat/stream-duplex.js" },
  "_stream_passthrough": { namespace: "streamPassThrough", path: "node-compat/stream-passthrough.js" },
  "node:_stream_passthrough": { namespace: "streamPassThrough", path: "node-compat/stream-passthrough.js" },
  "_stream_readable": { namespace: "streamReadable", path: "node-compat/stream-readable.js" },
  "node:_stream_readable": { namespace: "streamReadable", path: "node-compat/stream-readable.js" },
  "_stream_transform": { namespace: "streamTransform", path: "node-compat/stream-transform.js" },
  "node:_stream_transform": { namespace: "streamTransform", path: "node-compat/stream-transform.js" },
  "_stream_writable": { namespace: "streamWritable", path: "node-compat/stream-writable.js" },
  "node:_stream_writable": { namespace: "streamWritable", path: "node-compat/stream-writable.js" },
  "_stream_wrap": shim,
  "node:_stream_wrap": shim,
  string_decoder: stringDecoder,
  "node:string_decoder": stringDecoder,
  test,
  "node:test": test,
  timers,
  "node:timers": timers,
  "timers/promises": timersPromises,
  "node:timers/promises": timersPromises,
  tls,
  "node:tls": tls,
  "_tls_common": shim,
  "node:_tls_common": shim,
  "_tls_wrap": shim,
  "node:_tls_wrap": shim,
  trace_events: shim,
  "node:trace_events": shim,
  tty: shim,
  "node:tty": shim,
  url,
  "node:url": url,
  util,
  "node:util": util,
  "util/types": utilTypes,
  "node:util/types": utilTypes,
  v8: shim,
  "node:v8": shim,
  vm: shim,
  "node:vm": shim,
  wasi: shim,
  "node:wasi": shim,
  worker_threads: shim,
  "node:worker_threads": shim,
  sys: util,
  "node:sys": util,
  zlib,
  "node:zlib": zlib,
};

export function builtin(name: string): Builtin | undefined {
  return builtins[name];
}

export function builtinNames(): readonly string[] {
  return Object.keys(builtins).filter((name) => !name.startsWith("node:"));
}

export function compilerAliases(resolveInternal: (path: string) => string): string[] {
  return Object.entries(builtins)
    .map(([name, implementation]) => `--alias:${name}=${resolveInternal(implementation.path)}`);
}
