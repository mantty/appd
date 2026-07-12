# Bare appd codebase review

This review covers the Bare-based appd branch as it currently stands. The
native/TypeScript/Rust split is directionally good, and the example app does
not require app-specific runtime boilerplate. The following issues are the
main risks to address before expanding the feature surface.

## Findings

### 1. Vendored WebSocket protocol code — completed

`vendor/bare-ws/lib/frame.js` now encodes RSV1, RSV2, and RSV3 independently,
and `vendor/bare-ws/lib/socket.js` validates masking on zero-length frames.
`runtime/tests/bare-ws.test.ts` covers both protocol cases.

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

### 4. The Cloudflare compatibility surface is much smaller than it appears

The following are missing or only stubs within the current `cloudflare:workers`
and Worker-runtime surface:

- `WorkerEntrypoint` lifecycle and dispatch semantics, including a real `ctx`
  and `env` passed to methods.
- `ExecutionContext` lifecycle, `waitUntil` completion/error handling,
  `passThroughOnException`, and request properties such as `ctx.props`.
- Full Web Platform/Cloudflare additions such as `HTMLRewriter`, complete
  WebSocket APIs, and Cloudflare-specific request/response extensions.
- Colo/request metadata and `Request.cf` behavior.

This is not a problem if appd deliberately defines a small supported contract;
it is a problem if the compatibility package presents these names as if they
were supported.

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

### 7. Cloudflare semantics are not yet matched

The missing behavior is broader than missing names:

- `waitUntil()` is fire-and-forget; `drain()` does not await or retain pending
  work through the request lifecycle.
- `passThroughOnException()` does nothing, and there is no platform fallback
  response to pass through to.
- `env` is a mutable global rather than an isolate/request-scoped binding
  environment.
- Class-based entrypoint lifecycle and dispatch semantics are not implemented.
- WebSocket close events are emitted locally and remotely immediately, rather
  than following wire close state. There is no `readyState`, close code/reason,
  property event handlers, or complete backpressure/error behavior.
- Request and response bodies are not streamed, and request URLs are rebuilt
  with a hardcoded `https://localhost` authority.
- Worker exceptions become a generic 500 response containing the exception
  message, rather than following a defined production error policy.
- The process shim has incomplete Node behavior; in particular, `nextTick`
  ordering and `process.env` do not match Node.

### 8. Certificate caching — completed

The cache now requires a completion marker and non-empty regular files, validates
cached certificate/key material, atomically replaces each file, and writes the
marker last. Private-key files and the cache directory use restrictive Unix
permissions. The unused PKCS#12 cache artifact and fixed password were removed.

### 9. iOS deployment is hardcoded to 17.0

The deployment target appears in the toolchain, build script, plist, and Rust
configuration. This is a product compatibility decision hidden in build
plumbing and blocks older devices.

### 10. `appd-cli/src/build.rs` responsibilities — completed

Build orchestration, worker compilation, target-pack handling, shared helpers,
and Apple bundle/signing code now live in separate focused modules.

### 11. Native builds depend on package-manager layout

CMake resolves Bare modules through the root hoisted `node_modules`, while
packaging uses isolated linking. Native compilation and packaging therefore
depend on different dependency layouts.

### 12. Missing target packs trigger a source build

The CLI falls back to building the Bare SDK, runtime, packer, and Rust runtime
from the source workspace. That is useful for development but should be an
explicit developer command, not an implicit product path.

### 13. Target-pack integrity is half implemented

The manifest supports SHA-256 values, but generated manifests set every hash to
`None`, and consumers do not verify artifact contents.

### 14. Vendored dependencies lack synchronization metadata

The repository contains full copies of `bare-tls`, `bare-ws`, and
`cmake-toolchains`, but does not record a machine-checkable upstream revision or
divergence manifest. Future upgrades will be difficult to audit.

### 15. Generated output leaks into the working tree

Nested `tools/bare-pack/node_modules` and `target` directories are not covered
by the root ignore rules. Packing cleanup is also best-effort, so failed builds
can leave stale symlinks and files behind.

### 16. Addon discovery is implicit

The CLI scans every dependency for `addon: true`, derives framework names, and
special-cases `bare-tls` separately. An explicit target-pack addon manifest
would make the native artifact contract deterministic.

### 17. CI misses some riskiest paths

Pull-request CI does not build the native Bare SDK or run a real bundled app.
There is no macOS end-to-end test, physical iOS smoke test, or WebSocket
latency regression benchmark.

## Recommended order

1. Decide and document the supported Cloudflare contract; fail explicitly for
   everything else.
2. Decide which non-`fetch` handlers and additional Cloudflare services are in
   scope.
3. Make target-pack integrity and iOS deployment compatibility explicit
   product decisions.
4. Make native packaging hermetic and close the remaining CI coverage gaps.

The Bare architecture remains viable. These are boundary and contract issues,
not evidence that the core technology choice is wrong.
