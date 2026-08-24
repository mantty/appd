# Bare appd codebase review

This review covers the Bare-based appd branch as it currently stands. The
native/TypeScript/Rust split is directionally good, and the example app does
not require app-specific runtime boilerplate. The following issues are the
main risks to address before expanding the feature surface.

> Historical review of the removed Bare runtime. The current QuickJS
> architecture is documented in [the QuickJS runtime plan](plans/appd-quickjs-runtime.md).

## Findings

### 1. Vendored WebSocket protocol code — completed

`bare/vendor/bare-ws/lib/frame.js` now encodes RSV1, RSV2, and RSV3 independently,
and `bare/vendor/bare-ws/lib/socket.js` validates masking on zero-length frames.
`bare/runtime/tests/bare-ws.test.ts` covers both protocol cases.

### 2. The mTLS boundary has compiled handshake integration coverage — completed

`bare/tests/test_mtls.py` generates a temporary certificate hierarchy and runs
valid, missing-client, wrong-CA, hostname, and expired-certificate handshakes
against the compiled Bare TLS addon. The macOS case runs in the weekly SDK
workflow and fails if the compiled artifact is unavailable.

### 3. HTTP and asset handling — completed

Request bodies now use a pull-driven `ReadableStream`, responses are written
chunk by chunk with backpressure, and assets use `fs.createReadStream()`.
Manifest loading remains synchronous at startup only.

The runtime streams request and response bodies through a small adapter at the
runtime boundary. It keeps buffering only where the worker API requires it.
This is not an app-level
choice or configuration knob: the app already expresses the choice through
the standard body API. A `Response` with a `ReadableStream` body should be
forwarded chunk by chunk; an app that calls `response.text()` or
`response.arrayBuffer()` has chosen to buffer before returning. A fixed string
or byte body can be written directly without first aggregating it. There is no
size threshold: request and response streaming semantics must be preserved for
every payload size, with backpressure carried through the runtime boundary.

Astro's streamed components therefore work without special handling. Astro
returns a response whose body is a stream, and appd's job is to connect that
stream to Bare HTTP. The same applies to incoming requests: expose the Bare
HTTP async iterator as a `ReadableStream` in the worker `Request` instead of
calling `collect()` first. The app can still explicitly buffer by consuming
the request body, but the runtime must not do so on its behalf.

### 4. Cloudflare compatibility contract — defined

The supported core contract now includes class-based `WorkerEntrypoint`
construction with request-scoped `ctx` and `env`, and awaited `waitUntil()`
completion. Unsupported Durable Objects and RPC targets fail explicitly when
constructed instead of silently acting like supported APIs.

The following remain intentionally outside the supported contract:

- `passThroughOnException`, because appd has no origin server to pass a request
  through to.
- Durable Object storage/routing/hibernation and RPC dispatch.
- Full Web Platform/Cloudflare additions such as `HTMLRewriter` and
  Cloudflare-specific request/response extensions.
- Colo/request metadata and `Request.cf` behavior.

The compatibility package should continue to expose only APIs with defined
appd behavior, or fail explicitly as these unsupported features do.

### 5. Non-`fetch` Worker event handlers are not implemented

The runtime currently dispatches only the default `fetch(request, env, ctx)`
export. It does not provide event types or dispatch for `scheduled`, email,
tail, trace, queue, or other non-`fetch` handlers. Their event-specific
lifecycle, retry, acknowledgement, and failure semantics are also absent.

Queue handlers additionally depend on the Queue service integration described
below.

### 6. Additional Cloudflare services are not implemented

These are separate product integrations beyond the core Workers runtime:

- `DurableObject`, `DurableObjectState`, `DurableObjectId`,
  `DurableObjectNamespace`, storage, transactions, alarms,
  `blockConcurrencyWhile`, WebSocket hibernation, and object request routing.
- `RpcTarget` RPC exposure, serialization, method dispatch, errors, and
  lifetime management.
- KV, R2, D1, Queues, service bindings, Cache API, Hyperdrive, Vectorize,
  Workers AI, Analytics Engine, mTLS certificate bindings, secrets, and other
  environment-specific bindings.
- The consistency, retry, and lifecycle semantics associated with these
  services.

The compatibility contract should name which of these appd supports rather
than implying that a generic `env` object provides them automatically.

### 7. Core Cloudflare semantics — substantially improved

The runtime now awaits `waitUntil()` work, constructs class entrypoints per
request, streams request/response bodies, derives request URLs from the Host
header, returns a generic 500 response without leaking exception text, and
uses Bare's standard process compatibility module for `process.env` and
`process.nextTick`.

Remaining semantic differences are explicit:

- `passThroughOnException()` fails explicitly because no origin fallback exists.
- Imported `env` bindings are runtime-scoped; class entrypoints receive the
  same bindings explicitly per request.
- WebSocket close metadata is tracked by appd, but the vendored transport does
  not expose all wire-level close details or backpressure metrics.
- Cloudflare request metadata such as `Request.cf` is unavailable locally.

### 8. Certificate caching — completed

The cache now requires a completion marker and non-empty regular files, validates
cached certificate/key material, atomically replaces each file, and writes the
marker last. Private-key files and the cache directory use restrictive Unix
permissions. The unused PKCS#12 cache artifact and fixed password were removed.

### 9. iOS deployment is hardcoded to 17.0

The deployment target appears in the toolchain, build script, plist, and Rust
configuration. This is a product compatibility decision hidden in build
plumbing and blocks older devices.

### 10. `cli/src/build/mod.rs` responsibilities — completed

Build orchestration, worker compilation, target-pack handling, shared helpers,
and Apple bundle/signing code now live in separate focused modules.

### 11. Native builds depend on package-manager layout — completed

CMake now consumes a generated, target-specific isolated native-module
deployment, matching the dependency layout used by packaging.
The pinned BareKit dependencies remain resolved from the pinned BareKit source;
appd's native addons no longer depend on the repository's root `node_modules`.

### 12. Missing target packs no longer trigger a source build — completed

The app build path requires a bundled or explicitly supplied target pack. Source
target-pack generation is available only through the maintainer-only
`cargo run -p xtask -- target-pack` command.

### 13. Vendored dependencies lack synchronization metadata

The repository contains full copies of `bare-tls`, `bare-ws`, and
`cmake-toolchains`, but does not record a machine-checkable upstream revision or
divergence manifest. Future upgrades will be difficult to audit.

### 14. Generated output leaks into the working tree — completed

Nested tool dependency and target directories are now ignored. Target-pack
outputs and deploy destinations are cleared before each build, including stale
symlinks, and the target-pack directory is cleared before source compilation so
failed builds cannot leave an older pack appearing current.

### 15. Addon discovery is implicit

The CLI scans every dependency for `addon: true`, derives framework names, and
special-cases `bare-tls` separately. An explicit target-pack addon manifest
would make the native artifact contract deterministic.

### 16. CI misses some riskiest paths

Pull-request CI does not build the native Bare SDK or run a real bundled app.
There is no macOS end-to-end test, physical iOS smoke test, or WebSocket
latency regression benchmark.

## Recommended order

1. Decide which non-`fetch` handlers and additional Cloudflare services are in
   scope.
2. Make iOS deployment compatibility an explicit product decision.
3. Close the remaining CI coverage gaps.

The Bare architecture remains viable. These are boundary and contract issues,
not evidence that the core technology choice is wrong.
