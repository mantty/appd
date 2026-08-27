# Request-scoped Bare realms and VFS

Status: superseded by [appd QuickJS runtime](appd-quickjs-runtime.md)

## Objective

Execute every Worker request in a fresh JavaScript Realm, with one
pre-warmed Realm ready for the next request. Give that Realm a Rust-backed
filesystem that is destroyed with it.

This work precedes the remaining Workers and Node compatibility work. The
Node compatibility plan is written against this lifecycle rather than trying
to recover request identity from a shared Realm.

## Constraints

- Consume upstream Bare and `bare-realm`; do not patch, fork, or add private
  hooks to Bare.
- Keep the add-on appd-owned and written in Rust using Bare's supported
  add-on interfaces.
- Do not expose host filesystem paths or native Realm handles to Worker code.
- Do not share mutable JavaScript objects, globals, module state, or VFS data
  between requests.
- Keep direct `bare-*` imports an explicit escape hatch. They remain outside
  the Workers compatibility contract and are not rejected by appd.
- Preserve streaming request, response, `waitUntil()`, and WebSocket
  behaviour; Realm destruction must wait for the associated work to finish.

## Upstream boundary

`bare-realm` is the upstream Realm primitive. Its public API creates a Realm,
evaluates code in it, and destroys it. The appd add-on may compose that API
with a request bridge and VFS, but must not depend on Bare engine internals.

The first implementation phase is a capability test against the pinned
upstream version. It must prove that the add-on can:

1. create and destroy multiple Realms;
2. install the appd bootstrap in each Realm;
3. invoke a request handler repeatedly without cross-Realm object leakage;
4. bind native operations to the calling Realm; and
5. share immutable bundle storage where the upstream API permits it.

If a capability requires private Bare symbols or an upstream patch, stop and
redesign the bridge. Do not work around the boundary by forking Bare.

## Architecture

Keep the implementation to five responsibilities:

### Bundle image

`BundleImage` is one immutable, reference-counted representation of the
packaged Worker bundle and read-only assets. The runtime loads it once.

Each Realm receives a read-only reference to this image. Sharing the compiled
engine representation is an optimisation, not something the public
`bare-realm` API currently promises. The add-on must measure whether the
upstream engine shares compiled code; otherwise it must at least share the
source/assets and report the per-Realm compiled-memory cost. It must not fake
sharing by passing mutable objects between Realms.

### Realm manager

The Rust `RealmManager` owns the bundle image and a bounded set of Realm
leases. A lease is one request execution unit and is never returned to the
pool after use.

Normal lifecycle:

```text
pre-warmed -> checked out -> running -> draining -> destroyed
                         \-> cancelled -> destroyed
```

The manager maintains an idle reserve of one fully bootstrapped Realm. It
starts with two prepared Realms before serving requests, so checking one out
never consumes the reserve. It creates replacements as leases are consumed.
Requests may create additional Realms up to the configured bound; if capacity
is exhausted they wait or receive a clear overload error. There is no fallback
shared Worker Realm.

Realm creation failure is observable. Startup fails if the initial Realm
cannot be prepared; a replacement failure is surfaced through runtime health
and the next acquisition, not silently hidden.

### Request bridge

The host passes a small request envelope into the Realm rather than a
cross-Realm `Request` object:

- method, URL, headers, and a body stream handle;
- cancellation and client-disconnect signals; and
- response status, headers, body stream, and upgrade information on return.

The bridge owns bounded queues and backpressure. It must not buffer an entire
request or response merely to cross the Realm boundary. A response Realm is
kept alive until its body is consumed or cancelled and all `waitUntil()` work
settles.

An HTTP upgrade keeps the same execution unit alive until the WebSocket closes.
The associated VFS is released at that point, not immediately after the 101
response.

### Realm bootstrap

The bootstrap is the same appd runtime entrypoint currently used for a Worker,
but it is evaluated inside each Realm. It installs globals, the configured
environment, the Worker entrypoint, request context, and the Realm-bound VFS
module before loading the application bundle.

Top-level application code therefore runs once per request. Mutable top-level
state is intentionally not shared between requests; this is the required
trade-off for request isolation and must be covered by compatibility tests.

### Realm VFS

Each lease owns one Rust `RealmVfs` instance. The VFS is bound to the lease,
not discovered through a process-global current-request variable.

- `/bundle` is a read-only view backed by the shared `BundleImage`.
- `/tmp` and all writable state are private to the lease.
- The initial implementation is memory-backed and bounded; it does not write
  worker-controlled paths to the host filesystem.
- Sync and async `node:fs` operations use the same VFS object.
- Dropping or explicitly destroying the lease deletes the VFS and releases
  all buffers, handles, and pending operations.

Direct `bare-fs` access remains outside the Worker contract. The appd
`node:fs` implementation is the path that receives request-scoped VFS
semantics.

## Add-on boundary

The appd add-on should expose only host-side lifecycle operations to the
runtime shell:

- create the manager from a bundle image;
- acquire a Realm lease;
- dispatch, cancel, and drain a lease; and
- destroy the manager.

Realm code must not receive the manager, another Realm, or a raw native
pointer. The bootstrap installs a Realm-local filesystem binding carrying that
lease's VFS handle; asynchronous operations retain the same handle rather than
looking up a process-global current request.

The add-on owns Realm and VFS cleanup. Rust `Drop` paths are safety nets, not
the normal request lifecycle; explicit cancellation and destruction must be
idempotent and tested. Any VFS value visible inside a Realm is an opaque,
lease-bound capability, never a raw native pointer.

## Implementation sequence

1. Pin the upstream `bare-realm` version and build a minimal Rust add-on
   capability test on every supported target.
2. Add `BundleImage` and prove one immutable bundle can be referenced by
   multiple Realm leases without mutable cross-Realm objects.
3. Implement the Realm manager, one-reserve prewarm policy, bounded admission,
   cancellation, and explicit lifecycle errors.
4. Move the appd bootstrap and Worker dispatch into a Realm and implement the
   request/response/stream bridge.
5. Implement the Rust-backed Realm VFS and the appd `node:fs` adapter.
6. Integrate WebSocket lifetime and `waitUntil()` draining before destroying
   a lease.
7. Measure bundle bytes, compiled code, heap, RSS, startup latency, request
   latency, and concurrent-request memory on every platform.
8. Only after this plan passes its acceptance criteria, resume the remaining
   Node and Workers compatibility work.

## Tests

- Two Realms cannot observe each other's globals, module caches, timers,
  events, or VFS files.
- A request cannot access another lease's VFS by path, handle, stream, or
  retained JavaScript object.
- `/bundle` rejects writes; `/tmp` disappears after normal completion,
  cancellation, thrown errors, and `waitUntil()` settlement.
- Concurrent requests exercise sync and async filesystem calls without a
  process-global request marker.
- Request and response streams preserve backpressure and cancellation across
  the bridge.
- WebSocket upgrades keep the lease alive until close and then release it.
- A rejected or timed-out request always destroys its Realm and VFS.
- The manager has an idle pre-warmed Realm before accepting traffic and
  preserves that reserve while serving requests, without sharing mutable
  state.
- The same tests run on macOS arm64/x64, iOS device and simulators, Android,
  Windows, and Linux targets.

## Acceptance criteria

- Every ordinary request executes in a new Realm and receives a new VFS.
- At least one fully bootstrapped Realm is ready before normal traffic starts.
- No Bare patch, fork, or private engine hook is required.
- The appd runtime can share immutable bundle storage; any inability to share
  compiled engine state is measured and documented rather than hidden.
- Realm destruction is deterministic after response completion, cancellation,
  `waitUntil()`, or WebSocket close.
- The Astro example and compatibility fixtures pass without changing worker
  application code.
