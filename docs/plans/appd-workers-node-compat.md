# Workers JavaScript compatibility

Status: draft

## Objective

Run Workers with the current Workers `nodejs_compat` surface without making
the host operating system, Bare's own process, or unsupported Cloudflare
services visible to worker code.

appd alpha supports exactly one current Workers compatibility contract. It
always includes the current `nodejs_compat` surface; there is no application
opt-in or configuration-driven compatibility choice. Updating appd is how an
application moves to a newer contract.

This plan covers the JavaScript environment inside a normal Wrangler Worker:
Web APIs, Worker execution semantics, `cloudflare:workers`, and Node
compatibility. Cloudflare service bindings, Durable Object storage and routing,
edge metadata, and non-`fetch` event delivery remain separate product work.
WebAssembly is intentionally not part of appd's contract.

## Prerequisite: request-scoped realms

The compatibility work in this document follows
[request-scoped Bare realms and VFS](appd-request-realms-vfs.md); its
implementation begins only after that plan's acceptance criteria pass.
Each request runs in a fresh Realm with a Realm-bound Rust VFS and the Realm
is destroyed after its response, `waitUntil()` work, or WebSocket lifetime
ends. Node compatibility must target that lifecycle rather than adding
request identity to a shared JavaScript Realm.

## Current state

The current implementation still dispatches through a shared runtime Realm.
Request-scoped Realm/VFS isolation is planned separately and is a prerequisite
for the delivery sequence below.

The runtime now owns both compiler aliases and `process.getBuiltinModule()` in
`bare/runtime/src/node-compat/registry.ts`. It provides adapters for
assertions, buffers, console, constants, crypto, diagnostics channels, DNS,
events, HTTP(S), modules, outbound TCP/TLS, OS, paths, performance, process,
punycode, query strings, streams, string decoding, timers, URLs, utilities,
and compression. It maps the Workers non-functional Node modules to explicit
stubs rather than exposing Bare's host-capable equivalents.

`node:test` is importable. Its supported surface is `MockTracker`; test runner
and mock-timer APIs fail explicitly. Compiled runtime module tests import and
use `MockTracker` under Bare.

`cloudflare:workers` provides `env`, `WorkerEntrypoint`, and `ctx.exports` for
in-worker entrypoints. `DurableObject` and `RpcTarget` reject explicitly. The
dispatcher accepts the default `fetch` export or a `WorkerEntrypoint`
subclass; it does not dispatch other Worker handlers.

`caches` provides app-private persistent cache storage. It supports the
default and named caches, `put`, `match`, `delete`, and `keys`, preserves
freshness, `Vary`, conditional requests, and single byte ranges. It is not an edge
cache: cache-tag purging and eviction limits are not implemented.

`EventSource` is available through the global `fetch()` implementation or a
Cloudflare-style `fetcher` option. It parses streamed events, reconnects after
an interrupted stream, and closes permanently after a `204` response. appd
does not have an outbound cookie jar, so `withCredentials` is observable but
does not attach cookies to the default fetch transport.

### Current compatibility gaps

The following gaps are in the JavaScript runtime itself, rather than in
Cloudflare's storage, networking, or deployment products.

Missing APIs:

- `node:async_hooks`, including meaningful `AsyncLocalStorage`; the current
  Bare implementation returns `-1` for async IDs and does not propagate state.
- `HTMLRewriter`.
- Global `FormData`, `Blob`, `File`, `Event`, `EventTarget`, and `CustomEvent`.
- `TextEncoderStream`, `TextDecoderStream`, and `URLPattern`.
- Worker-style global `unhandledrejection` and `rejectionhandled` events.

Present but semantically different:

- `fetch`, timers, and streams are not restricted to an active request context.
- `Date.now()`, `performance.now()`, and `performance.timeOrigin` do not use
  Workers' I/O-oriented timing semantics.

The detailed status of WebSockets, MessageChannel, EventSource, `caches`, and
`cloudflare:workers` is maintained in the runtime API table below.

The current Node adapters are also partial in specific areas: `net` and `tls`
are client-only, `dns` exposes only a subset of resolver operations,
`perf_hooks` is limited to performance timing, `module` has limited resolver
support, `test` only provides `MockTracker`, and `http`/`https` are
fetch-backed rather than complete Node connection and server implementations.

`node:fs` and `node:fs/promises` remain intentionally absent until the
request-scoped Realm/VFS prerequisite passes. They will be backed by the Rust
VFS owned by the current Realm; they must not use the host filesystem. This
does not depend on `node:async_hooks`, because Realm identity provides the VFS
ownership boundary.

The runtime packages Bare modules as implementation dependencies. Direct
`bare-*` imports remain the developer's choice and outside the Workers
compatibility contract; appd does not reject them.

## Workers `nodejs_compat` contract

Workers has three distinct Node compatibility categories.

| Category | Workers behaviour | appd behaviour |
| --- | --- | --- |
| Native module | The runtime provides a functional API, sometimes with documented limits. | Provide the same module and documented limits. |
| Non-functional stub | Import succeeds; the underlying API is not usable. | Import succeeds and fails when used. Do not expose a more capable host API. |
| Wrangler `unenv` shim | Wrangler makes an otherwise unsupported Node import load; invoked APIs throw. | Bundle the same shim contract. |

Every appd alpha application receives the current `nodejs_compat` surface.
`compatibility_date` and `compatibility_flags` in a Wrangler configuration do
not select or alter the appd runtime. They are accepted only so ordinary
Wrangler configurations continue to build. Historical compatibility emulation
is deferred until after alpha.

### Contracted module specifiers

The current contract includes these Node module specifiers. Deferred entries
are not yet implemented by appd. Subpaths shown here are part of the contract.

| Area | Module specifiers |
| --- | --- |
| Assertions and data | `assert`, `assert/strict`, `buffer`, `string_decoder`, `url`, `querystring`, `punycode` |
| Async and events | `async_hooks` (deferred), `diagnostics_channel`, `events`, `timers`, `timers/promises` |
| Streams | `stream`, `stream/consumers`, `stream/promises`, `stream/web`, `_stream_*` |
| Runtime | `console`, `constants`, `module`, `process`, `sys`, `util`, `util/types`, `perf_hooks` |
| Cryptography and compression | `crypto`, `zlib` |
| Networking | `dns`, `dns/promises`, `net`, `tls`, `http`, `https`, `_http_*`, `_tls_*` |
| Files and platform | `os`, `path`, `path/posix`, `path/win32`; `fs` and `fs/promises` are tracked with the separate VFS design |
| Testing | `test` |

The runtime's built-in module registry is authoritative. The table in this
plan is a summary, not a separately maintained module registry.

### Workers non-functional stubs

The current contract exposes these non-functional modules:

- `http2`, `vm`, `cluster`, `domain`, `trace_events`, `wasi`;
- `_stream_wrap`, `dgram`, `inspector`, `inspector/promises`, `sqlite`;
- `child_process`, `readline`, `readline/promises`, `repl`, `tty`, `v8`, and
  `worker_threads`.

Bare has implementations for several of these modules. appd must not route a
Workers stub to them. In particular, it must not expose Bare subprocesses,
UDP, TTYs, inspector, VMs, or worker threads through `nodejs_compat`.

### Wrangler `unenv` shims

Wrangler uses `@cloudflare/unenv-preset` for Node imports which are neither a
Workers native module nor a runtime stub. It makes imports usable by dependency
detection while invoked APIs throw.

The pinned `@cloudflare/unenv-preset` and Wrangler version are test
references for current module routing, global injections, and polyfills. appd
implements that behaviour directly in its built-in registry.

This includes Node builtins that do not appear in the tables above. appd does
not hand-maintain an incomplete list of no-op modules.

## Implementation design

### 1. Ownership and module resolution

`appd-bundle` parses Wrangler configuration once. It accepts and discards
`compatibility_date` and `compatibility_flags`; it resolves the worker entry,
assets, and text or JSON `vars` that appd uses.

All Node compatibility code belongs in `bare/runtime`:

- `src/node-compat/registry.ts` is the sole module registry. It classifies
  every `node:` and unprefixed builtin as native, a runtime stub, a Wrangler
  shim, or unavailable.
- `src/node-compat/` also contains the Node adapters and shared stubs.
- A runtime-packaged worker build entrypoint reads the registry and configures
  esbuild. `appd-cli` passes only compilation paths and invokes that entrypoint;
  it contains no builtin aliases or other Node compatibility decisions.

`cloudflare:*` APIs remain in their existing focused runtime modules, not in
the Node compatibility directory. `appd-bundle` continues to parse the app
configuration and has no Node compatibility role.

The packer receives the resolved worker path, output path, compiler path, and
runtime package path; it never parses a Wrangler file. `appd-bundle` writes a
normalized worker-environment manifest into the packaged app from the resolved
`vars` and future bindings. `appd-runtime` reads that manifest and passes it to
Bare at startup. No raw Wrangler configuration is passed beyond the bundle
step, and neither `appd-cli` nor the runtime re-parses it.

The resolver maps both `node:` and unprefixed forms of every registered
builtin consistently. `process.getBuiltinModule()` uses the same registry, so
it cannot return a module that static imports cannot load. Imports that are
not registered builtins, including direct `bare-*` imports, continue through
normal package resolution.

Do not use `bare-node` or `bare-node-runtime` as the application resolver.
They target general Bare applications and deliberately map host-capable Node
APIs outside the Workers contract. Their individual wrapper packages remain
eligible implementation dependencies after API tests prove their behaviour.

### 2. Core Node modules

Implement the high-value, non-host-facing modules first:

- `assert`, `buffer`, `events`, `path`, `querystring`, `string_decoder`,
  `url`, and `timers` from their Bare modules;
- `stream` and its public subpaths from `bare-stream`, with adapters where
  Node and Web stream conversions differ;
- `util`, `util/types`, `console`, `constants`, and `module` from Bare
  modules or small compatibility modules;
- `async_hooks` and `diagnostics_channel` with Realm/request lifetime tests;
- `process` from `bare-process`, filtered to the Workers contract.

`process.env` receives configured text and JSON bindings. Workers fixtures
define coercion and shape. It never exposes the shell environment of the appd
host. `process.exit`, process signals, standard IO, the current directory, and
other host controls follow Workers behaviour rather than Bare's process
behaviour.

### 3. Native-backed modules

Use the existing native Bare addons behind Node-compatible adapters:

These modules are available only when the built-in registry classifies the
requested specifier as functional.

| Workers module | Bare basis | Required appd boundary |
| --- | --- | --- |
| `crypto` and Web Crypto | `bare-crypto` | Match Workers' Node crypto algorithms and Web Crypto classes. Keep unsupported Node algorithms unsupported. |
| `zlib` | `bare-zlib` | Provide Node streaming and convenience APIs without buffering whole streams. |
| `dns`, `dns/promises` | `bare-dns` | Match Workers' supported resolver operations and errors. |
| `net` | `bare-tcp` or `bare-net` | Provide outbound sockets only. `net.Server` remains unsupported. |
| `tls` | `bare-tls` | Provide Workers-supported client TLS behaviour only. |

Every adapter accepts only data supplied by the worker and exposes no runtime
listener, certificate material, or platform-native handle.

### 4. HTTP and HTTPS

Implement `http` and `https` in the same shape as Workers:

- `get()` and `request()` adapt to the worker's global `fetch()` and are valid
  only while a request context is active;
- incoming and outgoing messages are Node streams backed by Web Streams;
- agents are Workers-compatible non-pooling stubs;
- documented unsupported options reject instead of silently using Bare socket
  options.

Use `bare-http1` and `bare-https` only where a protocol parser is required by
the adapter. Do not expose their general-purpose server or socket behaviour as
`node:http` semantics.

Implement `cloudflare:node`, `http.createServer()`, and
`handleAsNodeRequest()` together. `listen(port)` registers an in-worker routing
key; it never opens a host network listener. This permits multiple virtual
servers within a worker and preserves the Workers model.

### 5. Virtual file system and OS module

The [request-scoped Realm/VFS plan](appd-request-realms-vfs.md) owns the VFS
lifecycle. `node:fs` and `node:fs/promises` are adapters over the Rust VFS
bound to the current Realm; they must never resolve to `bare-fs`'s host
filesystem.

Workers fixtures define the VFS surface and device behaviour:

- `/bundle` is a read-only view of files included in the worker bundle;
- `/tmp` is private to the Realm and discarded when the Realm is destroyed;
- `/dev/null`, `/dev/random`, `/dev/full`, and `/dev/zero` have Workers
  behaviour;
- permissions, owners, clocks, file-size and path limits match the Workers
  VFS contract;
- watch and glob APIs remain explicitly unsupported.

The VFS is identified by Realm ownership, not by a process-global current
request or `AsyncLocalStorage`. The `os` adapter reports the limited, safe
Workers surface and does not reveal the device's user, host name, network
interfaces, or filesystem locations.

### 6. Stubs and shims

Use one shared runtime helper for Workers stubs and Wrangler shims. It carries
the module name and emits a deterministic unsupported-API error when a method,
constructor, or proxy member is used.

The resolver controls whether a module is absent, a Wrangler shim, or a
Workers runtime stub for the current contract. The implementation does not
substitute a real Bare module merely because one exists.

### 7. Other Workers JavaScript APIs

Complete the non-service runtime surface alongside Node compatibility.
The following table is the authoritative current state for these APIs.

| API | Current appd state | Planned boundary |
| --- | --- | --- |
| Fetch, Headers, Request, Response | Present, with appd response adapters. | Differentially test immutable headers, body locking, aborting, redirect and cache-mode behaviour. |
| Web Crypto and Web Streams | Present through Bare modules. | Bring algorithms, cloning, backpressure and cancellation to Workers behaviour. |
| WebSocket and `WebSocketPair` | Partial. | Worker pairs, transport bridging, event instances, ready states and close validation are present. The Bare transport does not expose a remote close code, reason, or clean-close status, so appd cannot yet report those accurately. |
| `CacheStorage` / `caches` | Partial. | App-private persistent caches support default and named caches, `put`, `match`, `delete`, `keys`, freshness, `Vary`, conditional requests, and single byte ranges. Cache-tag purging and eviction limits remain unavailable. |
| `HTMLRewriter` | Missing. | Add a streaming HTML transformation implementation or leave it explicitly unavailable until it meets the Workers API. |
| `CompressionStream` / `DecompressionStream` | Present. | gzip, deflate and deflate-raw are backed by `bare-zlib` Web Streams. |
| `MessageChannel`, `MessagePort`, structured cloning | Partial. | Match Workers' limited transfer-list, `messageerror`, and close semantics without exposing worker threads. |
| `EventSource`, `scheduler`, `performance` | Partial. | `EventSource` supports the standard event stream lifecycle and the Workers `fetcher` extension. `withCredentials` is observable but cannot attach cookies until appd has an outbound cookie jar. |
| `cloudflare:sockets` | Partial. | Outbound TCP, TLS and StartTLS sockets are available; StartTLS invalidates the old socket streams. Socket creation is not yet restricted to request handlers. |
| `cloudflare:workers` | Partial. | `env`, `WorkerEntrypoint`, and `ctx.exports` are available. `passThroughOnException()` has no equivalent, and `ctx.exports` currently recognises only `WorkerEntrypoint` subclasses. Service-backed classes continue to fail explicitly until their services exist. |

Durable Objects, R2, D1, KV, Queues, service bindings, AI, Workflows, email,
and other binding-backed APIs are not part of this runtime plan. Each needs a
separate local service contract. `DurableObject` and `RpcTarget` continue to
fail explicitly until then.

## Delivery sequence

1. Complete [request-scoped Bare realms and VFS](appd-request-realms-vfs.md),
   including the pre-warmed Realm, request bridge, Rust VFS, and lifecycle
   tests. No remaining compatibility work is considered complete before this
   prerequisite passes.
2. Move the current CLI builtin aliases into the runtime-owned registry and
   runtime-packaged worker build entrypoint. The CLI passes compilation paths
   only. Add a CLI-to-packer integration test importing `node:assert`, which
   is not in the current CLI alias list and must resolve through the runtime
   registry.
3. Add `bare/runtime/tests/fixtures/node-compat/wrangler.json` with the fixed
   Wrangler reference date and flags. Add the normalized worker-environment
   manifest, including text and JSON `vars`. Reject unavailable Node imports
   at build time. Add a packaged-runtime test proving those vars reach
   `process.env` and host environment variables do not.
4. Add the native/stub/shim implementations and parity tests for import
   behaviour and globals.
5. Ship the core data, stream, utility, process, async-context, and console
   modules.
6. Ship crypto, compression, DNS, outbound TCP, and TLS adapters.
7. Ship fetch-backed HTTP/HTTPS and the virtual `cloudflare:node` server
   bridge.
8. Ship the Realm-bound `node:fs`/`node:fs/promises` adapters and limited OS
   module. VFS ownership comes from the Realm; it does not depend on
   `node:async_hooks`.
9. Complete the non-service Workers APIs in the order in the table above.
10. Keep every unavailable API either absent when Workers leaves it absent or
   importable-and-failing when Workers provides a stub.

## Test strategy

Maintain a compatibility fixture suite with the same worker sources executed
by Wrangler/workerd and by packaged appd runtimes.

- Run Realm lifecycle tests before compatibility fixtures: fresh globals and
  module state, Realm-bound VFS ownership, pre-warm replacement, stream
  cancellation, `waitUntil()`, and WebSocket lifetime.
- Build the same fixtures through the pinned Wrangler version using the
  reference date and flags in
  `bare/runtime/tests/fixtures/node-compat/wrangler.json`, then execute its
  workerd output. Compare it with appd; the fixtures are the semantic oracle.
- Assert import outcome, exported names, global injection, first use, and
  `process.getBuiltinModule()` for every Node module and subpath in the
  contract. Cover both `node:` and unprefixed imports.
- Independently vary configured compatibility dates and flags and assert they
  cannot change appd's fixture outcome.
- Assert the runtime module registry matches the compiler's resolution.
- Package a worker through the CLI which imports `node:assert`; this proves
  the CLI invokes the runtime packer rather than retaining builtin aliases.
- Assert configured text and JSON Worker vars have the current Workers
  `process.env` coercion and shape, while the ambient appd process environment
  remains hidden.
- Through a packaged runtime, assert the worker-environment manifest reaches
  `process.env` with that same coercion and the ambient host environment is
  still hidden.
- Test functional APIs with values, errors, stream cancellation, and request
  lifetime semantics rather than import success alone.
- Test `async_hooks` and `diagnostics_channel` propagation across microtasks,
  timers, concurrent requests, and `waitUntil()` work.
- Test Realm-bound VFS isolation with concurrent requests, bundle read-only
  paths, device files, and cleanup after response and `waitUntil()` completion;
  do not use a process-global request marker.
- Test HTTP clients, virtual HTTP servers, WebSocket close, error,
  ready-state and backpressure behaviour, TCP, TLS, and compression through
  the normal packaged runtime path.
- Test `cloudflare:sockets` and `cloudflare:workers` `ctx.exports` directly,
  including their request-lifetime and in-worker routing boundaries.
- Prove virtual HTTP servers can coexist without a host listener, and prove
  worker code cannot access the host environment, current directory, file
  system, listeners, native handles, or TLS material.
- Normalize random values, clocks, network addresses, stack traces, paths,
  and runtime-version error details before comparison so differential tests
  fail only for semantic differences.
- Run the suite on macOS arm64 and x64, iOS device, both iOS simulators,
  Android arm64 hardware or an arm64 emulator, and Windows x64.

## Acceptance criteria

- Every ordinary request executes in a fresh Realm with a fresh VFS, and one
  fully bootstrapped Realm is pre-warmed before normal traffic starts.
- A worker resolves the same Node module category in appd as it does through
  the pinned Wrangler/workerd version using the checked-in reference config.
- Application code cannot read host files, host environment variables other
  than explicitly configured Worker vars, process state, certificates,
  listeners, or native UI handles through Node modules.
- Node APIs backed by Bare preserve the Workers restrictions, not Bare's
  broader host capabilities.
- The Node surface changes only when a new appd release adopts a newer current
  Workers contract; configured compatibility dates and flags cannot change it.
- Every stub and shim imports successfully only when Workers does, and fails
  clearly on use.
- `node:http` and `node:https` use worker fetch semantics, while virtual
  servers route in-process without opening a network port.
- `/tmp` data cannot cross Realm boundaries and `/bundle` is read-only.
- A packaged app passes the compatibility suite on every supported platform.
