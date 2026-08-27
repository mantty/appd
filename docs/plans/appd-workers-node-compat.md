# Workers and Node compatibility

Status: draft

## Objective

Run an ordinary Wrangler Worker with appd's current Workers Web API and
`nodejs_compat` surface on QuickJS-NG without exposing the host operating
system, the appd process, or unsupported Cloudflare services.

appd alpha has one compatibility contract. Every application receives the
current contract; `compatibility_date` and `compatibility_flags` are accepted
by the build for Wrangler compatibility but do not select historical appd
behaviour. A new appd release adopts a newer contract.

This plan covers Worker module execution, Web APIs, `cloudflare:workers`,
`cloudflare:sockets`, the virtual `cloudflare:node` bridge, Node builtins,
static assets, and app-private Cache Storage. Durable Object storage and
routing, service bindings, edge metadata, and non-`fetch` event delivery remain
separate product decisions. WebAssembly is not part of the appd Worker
contract.

## Current QuickJS implementation

`appd/src/quickjs` creates a QuickJS runtime and context for each request and
loads the shared packaged Worker bytecode into it. The packaged bytecode is
owned by an immutable app-wide Worker image. The current JavaScript runtime
contains small `events`, `stream`, `fs`, `cloudflare:workers`, and Web API
modules under capability modules (`appd/src/fs`, `appd/src/streams`,
`appd/src/network`, `appd/src/events`, `appd/src/globals`, and
`appd/src/builtins`).

The current filesystem is only a per-context JavaScript `Map` for `/tmp`.
Each accepted request is now scheduled on Tokio and gets its own QuickJS
runtime/context, so independent requests can execute concurrently without a
fixed appd request queue. This is an implementation baseline, not a claim
that the compatibility checklist is complete. An unchecked item is not
considered implemented merely because an example application works.

## Implementation rules

The engine, Rust/JavaScript ownership, request lifecycle, VFS internals,
gateway, packaging, and platform sequence belong to the
[appd QuickJS runtime plan](appd-quickjs-runtime.md). This plan owns
Worker-visible semantics and their differential fixtures.

- Give each operation one Rust primitive and let related JavaScript surfaces
  share it. Do not implement the same feature separately in the gateway, a
  native module, and a JavaScript shim.
- Never use the host filesystem, shell environment, process controls,
  listeners, certificates, or native handles as Worker-visible fallbacks.
- A QuickJS runtime is the request isolation boundary. Its global object,
  module cache, JavaScript heap, native resource table, and mutable `/tmp`
  belong to one request lifetime. Rust owns the lifetime explicitly; cleanup
  must not depend on QuickJS garbage collection.
- `appd/src/quickjs` schedules each request-owned QuickJS runtime independently
  through Tokio. Compatibility must not depend on scheduling details: no
  request may observe another request's globals, module state, environment,
  handles, timers, response body, or VFS.

## Missing-behaviour checklist

Items are ordered by common application impact first, then implementation
leverage. A later item may still be implemented earlier when its Rust
primitive is a dependency.

### Build, resolution, and request execution

- [ ] Move builtin resolution into one runtime-owned registry used by the
  packer, static `node:` imports, unprefixed imports, and
  `process.getBuiltinModule()`. It must classify every module as functional,
  Workers stub, Wrangler shim, or unavailable.
- [ ] Make the packer resolve every supported `node:` and unprefixed form
  consistently, including subpaths, and fail at build time for unresolved
  runtime builtins. Direct `bare-*` imports are outside this contract and
  must be rejected explicitly; omitting a compatibility alias is not enough.
- [ ] Package the normalized Worker environment manifest. Pass configured
  text and JSON `vars` to the request, apply Workers `process.env` coercion,
  and hide the shell environment, host process, host current directory, and
  host filesystem.
- [ ] Accept Wrangler `compatibility_date` and `compatibility_flags` without
  changing the fixed appd contract. Add a pinned Wrangler/workerd fixture as
  the semantic oracle for each release.
- [ ] Dispatch the default `fetch` export and the supported
  `WorkerEntrypoint` form with the correct `env` and execution context.
  Preserve module initialization, returned-promise, response-stream,
  `waitUntil()`, WebSocket, cancellation, timeout, disconnect, error, and
  shutdown lifetimes.
- [ ] Evaluate each request from the shared Worker image without sharing
  mutable JavaScript values between requests. Module state and top-level
  effects are request-local; the immutable bytecode and `/bundle` bytes are
  app-wide.
- [ ] Drain `waitUntil()` transitively until no tracked work remains or the
  event lifetime ends. Late completions must not invoke or retain a disposed
  runtime.
- [ ] Implement Worker-style `unhandledrejection` and `rejectionhandled`
  events and deterministic exception reporting.
- [ ] Make request and response bodies Rust-owned streams. Preserve binary
  bytes, streaming, body locking, cloning, backpressure, cancellation,
  disconnects, and bounded buffering; do not turn bodies into UTF-8 strings
  on the gateway hot path.
- [ ] Preserve HTTP details visible to Worker code and clients: repeated
  headers including `Set-Cookie`, `HEAD` responses, status text, connection
  handling, request bodies, response streaming, and cancellation.

### High-use Web APIs and globals

- [ ] Complete `fetch`, `Headers`, `Request`, and `Response` semantics:
  immutable/header guards, duplicate headers, body locking and cloning,
  redirects, aborts, cache modes, status and URL handling, WebSocket
  responses, and streaming bodies.
- [ ] Provide standards-compatible `AbortController` and `AbortSignal`,
  including propagation into Rust-owned fetches, streams, timers, sockets,
  and request teardown.
- [ ] Complete `ReadableStream`, `WritableStream`, and `TransformStream`,
  including queuing, backpressure, cancellation, teeing, locking, BYOB
  behaviour where supported, and Rust-backed source/sink adapters.
- [ ] Provide `TextEncoder`, `TextDecoder`, `TextEncoderStream`, and
  `TextDecoderStream` with correct replacement, fatal, streaming, and binary
  behaviour.
- [ ] Provide `URL`, `URLSearchParams`, and `URLPattern` with Workers URL
  parsing, serialization, matching, and error behaviour.
- [ ] Provide `Blob`, `File`, and `FormData` without corrupting binary data;
  keep multipart parsing and large body movement in Rust.
- [ ] Provide `Event`, `EventTarget`, and `CustomEvent`, including listener
  options, dispatch ordering, cancellation, and error handling.
- [ ] Complete structured cloning and transfer lists for the supported
  Worker values, including `MessagePort`, `ArrayBuffer`, typed arrays, Blob,
  File, and FormData where applicable.
- [ ] Provide global `Buffer` and the Web-compatible `crypto` object. Secure
  randomness must come from Rust or the platform CSPRNG, never `Math.random()`.
- [ ] Complete Web Crypto and Node crypto over one Rust cryptographic core:
  algorithms, key import/export, Web Crypto classes, digest, signing,
  encryption, random values, errors, and unsupported-algorithm behaviour.
- [ ] Provide `CompressionStream` and `DecompressionStream` for gzip, deflate,
  and deflate-raw with streaming backpressure and correct flush/error
  semantics. Provide the matching Node `zlib` APIs without whole-stream
  buffering.
- [ ] Provide global `performance`, `scheduler`, and timing methods with the
  Workers timing and permission semantics. Define the relationship between
  `Date.now()`, `performance.now()`, and `performance.timeOrigin` in the
  differential fixtures.
- [ ] Provide `HTMLRewriter` as a streaming Rust-backed transformation, or
  mark it explicitly unsupported in appd's contract until it exists.
- [ ] Provide the supported `Intl` constructors, prototypes, locale methods,
  options, errors, and CLDR data through one app-wide ICU4X data set and
  request-local wrappers. Do not silently ship QuickJS's incomplete ECMA-402
  surface.
- [ ] Remove the current fake `WebAssembly` fallback. Keep WebAssembly
  outside the contract by rejecting `.wasm` inputs and leaving the global
  absent.

### Request-scoped virtual filesystem

- [ ] Replace the current JavaScript `Map` with a Rust VFS owned by the
  request execution. Expose it through `node:fs` and `node:fs/promises` and
  route callback, promise, and synchronous forms through one filesystem core.
- [ ] Implement `/tmp` as a mutable in-memory tree private to one request.
  It must survive the handler's returned response, tracked response streams,
  transitive `waitUntil()` work, and accepted WebSocket handlers, then be
  destroyed after normal completion, errors, cancellation, timeout, and
  shutdown.
- [ ] Implement `/bundle` as an immutable view of packaged Worker files,
  sharing the Worker image without copying it into each request.
- [ ] Implement `/dev/null`, `/dev/random`, `/dev/full`, and `/dev/zero` with
  the Workers read/write/error behaviour. Device access must remain request
  scoped and host independent.
- [ ] Implement path normalization, absolute/relative path rules, traversal
  rejection, symlinks within the permitted VFS, `realpath`, and the documented
  error codes without allowing access to host storage, assets, caches, or
  another request.
- [ ] Implement file and directory descriptors and the supported operations:
  open/close, read/write, readv/writev, stat/fstat/lstat, readdir, mkdir,
  rmdir, rename, unlink, copy, truncate, realpath, readlink, and timestamp
  operations, with Node-compatible sync, callback, and promise result shapes.
- [ ] Enforce Workers VFS limits and metadata: 4,096-character paths, 48 path
  segments, 128 MB files, epoch timestamps where specified, storage/memory
  accounting, and deterministic exhaustion errors.
- [ ] Keep watch, glob, permissions, ownership, and other unsupported VFS
  operations explicitly unsupported rather than mapping them to host APIs.
- [ ] Test every teardown path and verify that request VFS bytes, descriptors,
  buffers, and native handles return to zero without a forced garbage
  collection.

### Core Node modules

- [ ] Implement `assert` and `assert/strict`, including deep equality,
  errors, messages, and strict-vs-loose semantics.
- [ ] Implement `buffer`, `string_decoder`, and `punycode`, including binary
  coercion, encodings, limits, and error behaviour.
- [ ] Implement `path`, `path/posix`, `path/win32`, `url`, and `querystring`
  with Node-compatible parsing, formatting, normalization, and edge cases.
- [ ] Implement `events`, including `EventEmitter`, listener limits,
  `captureRejections`, error events, and ordering.
- [ ] Implement `timers`, `timers/promises`, `console`, `constants`, `sys`,
  and the supported `process` surface. Keep scheduling and clocks in Rust;
  keep JavaScript method and event semantics in the adapter.
- [ ] Give `process` deterministic Worker values for `argv`, `cwd`, `pid`,
  `platform`, `arch`, `release`, `version`, `versions`, and `umask`. Make
  `process.exit`, signals, standard input/output/error, and other process
  controls explicitly unsupported or inert according to the pinned
  Workers/workerd oracle; they must never affect the appd process.
- [ ] Implement the limited `os` surface with deterministic Workers values.
  Do not expose the device user, host name, CPU details, network interfaces,
  filesystem locations, shell environment, or host resource state.
- [ ] Implement `util`, `util/types`, and `perf_hooks`, including formatting,
  inspection, type predicates, performance entries, and supported errors.
- [ ] Implement `module` and its supported resolver APIs without exposing the
  host loader, arbitrary host paths, or process-global module state.
- [ ] Implement `stream`, `stream/consumers`, `stream/promises`,
  `stream/web`, and the supported `_stream_*` subpaths over the Web Streams
  and Rust body primitives. Preserve Node backpressure, destruction,
  pipeline, finished, async iteration, and error ordering.
- [ ] Implement `node:async_hooks` with meaningful `AsyncLocalStorage` and
  async resource lifecycle behaviour for supported Worker-visible operations.
  Its state must follow the owning request across promises, timers, streams,
  fetch, sockets, WebSockets, and `waitUntil()` without a process-global
  current-request marker.
- [ ] Implement `diagnostics_channel` with request lifetime, subscriber,
  publish, and error semantics.
- [ ] Implement `node:test`'s supported `MockTracker` surface and explicit
  failures for test-runner and mock-timer APIs that Workers does not provide.

### Networking and HTTP compatibility

- [ ] Implement outbound `dns` and `dns/promises` over a Rust resolver with
  the supported lookup operations, address ordering, errors, cancellation,
  and Workers address policy.
- [ ] Implement outbound `net` clients over Rust TCP. `net.Server` and host
  listeners remain unsupported; sockets must be request-owned, cancellable,
  backpressured, and unavailable after request teardown.
- [ ] Implement outbound `tls` clients over Rust TLS with supported options,
  certificate validation, errors, StartTLS transitions, and no access to
  appd's server certificates or private keys.
- [ ] Implement `http`, `https`, `_http_*`, and `_tls_*` over the Rust fetch
  and socket primitives. `get()` and `request()` must expose Node message and
  stream shapes, Workers restrictions, cancellation, and documented option
  failures; agents must not create host-level connection pools.
- [ ] Implement `cloudflare:sockets` TCP, TLS, and StartTLS sockets with
  correct readable/writable streams, close/error/backpressure behaviour, and
  invalidation of old streams after StartTLS. Creation must obey request
  lifetime and address policy.
- [ ] Implement `cloudflare:node`, `http.createServer()`, and
  `handleAsNodeRequest()` as in-worker virtual routing. `listen()` registers
  a routing key and never opens a host network listener; multiple virtual
  servers may coexist.
- [ ] Complete `WebSocket` and `WebSocketPair`: transport bridging,
  `accept()`, ready states, `bufferedAmount`, protocol/extensions, Event and
  MessageEvent instances, handler properties, text/binary data, fragmented
  messages, ping/pong control frames, close validation, close code/reason/
  clean status, backpressure, errors, and lifetime through close.
- [ ] Implement `EventSource` over the Rust fetch stream, including event
  parsing, lifecycle events, reconnect delay and `Last-Event-ID`, interrupted
  streams, permanent close on `204`, and the Workers `fetcher` option.
  `withCredentials` must match the reference contract; appd has no implicit
  outbound cookie jar unless that contract requires one.

### Worker services and appd-owned APIs

- [ ] Complete `cloudflare:workers` `env`, `WorkerEntrypoint`, `ctx.exports`,
  `waitUntil`, and `passThroughOnException` semantics. `ctx.exports` must
  preserve in-worker routing, arguments, errors, and request lifetime without
  exposing host services.
- [ ] Keep `DurableObject` and `RpcTarget` import/use failures explicit until
  their storage and routing services have a separate contract. Do the same
  for R2, D1, KV, Queues, service bindings, AI, Workflows, email, and other
  binding-backed APIs.
- [ ] Implement app-private persistent `CacheStorage` and `caches`, including
  the default and named caches, `put`, `match`, `delete`, `keys`, freshness,
  `Vary`, conditional requests, cookies, single byte ranges, body streaming,
  and cache errors. It must be app-scoped Rust storage, not `/tmp` and not an
  edge cache.
- [ ] Preserve the current explicit limits for cache-tag purging, eviction
  policy, and other edge-cache features until a separate service contract
  exists.
- [ ] Implement static asset serving in Rust with the packaged manifest and
  streaming bytes. Match Wrangler HTML handling, direct assets, `.html`
  candidates, SPA fallback, configured 404 pages, nearest 404 handling,
  traversal protection, content types, and `env.ASSETS.fetch()`.
- [ ] Implement `MessageChannel`, `MessagePort`, `messageerror`, close, and
  supported transfer-list semantics without exposing worker threads.
- [ ] Make `EventSource`, `MessageChannel`, WebSockets, timers, scheduler,
  streams, fetch, cache, assets, and `ctx.exports` obey the same request
  lifetime and cancellation rules.

### Workers stubs and Wrangler shims

- [ ] Expose the Workers non-functional modules and fail on use with stable,
  useful errors: `http2`, `vm`, `cluster`, `domain`, `trace_events`, `wasi`,
  `_stream_wrap`, `dgram`, `inspector`, `inspector/promises`, `sqlite`,
  `child_process`, `readline`, `readline/promises`, `repl`, `tty`, `v8`, and
  `worker_threads`.
- [ ] Do not route a stub to a more capable Rust, QuickJS, or historical Bare
  implementation. In particular, do not expose subprocesses, UDP, TTYs,
  inspector access, arbitrary VMs, SQLite, or worker threads.
- [ ] Implement the pinned Wrangler `unenv` shim category for Node imports
  that Wrangler makes load but does not make functional. Imports must resolve
  through the same registry and invoked APIs must fail with the expected
  shape; do not maintain a hand-written list that silently misses builtins.

## Delivery

Follow the [appd QuickJS runtime plan](appd-quickjs-runtime.md) for engine,
gateway, request ownership, VFS, packaging, and platform work. Compatibility
work is:

1. Add the baseline fixtures and registry tests before expanding the runtime.
   Run the same Worker sources through the pinned Wrangler/workerd reference
   and the packaged QuickJS runtime.
2. Implement the checklist in order, moving each feature once. Give each
   feature all required JavaScript compatibility surfaces and differential
   tests.
3. Keep the runtime-owned registry as the only builtin compatibility table;
   the CLI must not grow a second one.
4. Remove the current JS-only filesystem map and all compatibility code that
   assumes a shared JavaScript context as each Rust-backed replacement lands.

## Verification

- [ ] Every checklist item has a fixture covering import, exported names,
  globals, successful use, errors, cancellation, and request lifetime where
  applicable.
- [ ] Assert that compatibility dates and flags do not alter any fixture
  outcome, and that the runtime registry and packer's resolver agree for every
  builtin and subpath.
- [ ] Package a Worker importing `node:assert` through the normal CLI path;
  this proves the CLI uses the runtime-owned registry instead of a second
  builtin alias table.
- [ ] Assert the exact Worker-visible `process` values and verify that exit,
  signals, standard I/O, `cwd`, and host environment access cannot affect or
  reveal the appd process.
- [ ] Test both `node:` and unprefixed forms and
  `process.getBuiltinModule()` for every supported module and subpath.
- [ ] Compare values, errors, headers, streams, random/timing-normalized
  output, cancellation, and lifetimes with the pinned Wrangler/workerd
  reference.
- [ ] Exercise streaming backpressure, binary bodies, body locking/cloning,
  repeated headers, HEAD, WebSocket controls, HTTP clients, virtual servers,
  DNS, TLS, compression, cache, assets, and `cloudflare:sockets` through the
  normal packaged runtime path.
- [ ] Use the QuickJS runtime plan's isolation, teardown, platform, and profiling
  checks; this plan adds only Worker-visible assertions.

## Acceptance criteria

- [ ] The entire checklist is implemented or has the same explicit
  unsupported/stub behaviour as the pinned Workers contract.
- [ ] Requests have independent Worker-visible runtime and VFS state.
- [ ] `/tmp` is writable, in-memory, private per request, and destroyed on
  every completion path without forced garbage collection.
- [ ] No supported API can reach host storage, host environment variables,
  host listeners, certificates, subprocesses, arbitrary native handles, or
  another request.
- [ ] The existing Astro application runs unchanged through the normal appd
  commands on all supported client targets.

## References

- [Cloudflare Workers Node.js compatibility](https://developers.cloudflare.com/workers/runtime-apis/nodejs/)
- [Cloudflare Workers VFS](https://developers.cloudflare.com/workers/runtime-apis/nodejs/fs/)
- [Cloudflare Workers request context](https://developers.cloudflare.com/workers/runtime-apis/context/)
- [QuickJS-NG developer guide](https://quickjs-ng.github.io/quickjs/developer-guide/intro/)
- [`rquickjs` documentation](https://docs.rs/rquickjs/latest/rquickjs/)
- [ICU4X Rust components](https://docs.rs/icu/latest/icu/)
