# appd runtime API and plugin plan

Status: proposal

## Purpose

Developers need on-device capabilities — location, Bluetooth, NFC, camera —
from apps that also deploy to the web. appd provides them through plugins.

A plugin needs somewhere to register its native code. That place is the same
public API each platform's shell uses to run appd. So appd first becomes a
library with a public API, and plugins follow.

## appd as a library

Today appd owns the application: it starts the process, creates the window,
and calls platform code. This inverts.

The shell is the application. It starts the process, creates the window and
WebView, handles OS lifecycle callbacks and permission prompts, and calls
appd.

appd is a library. It runs the JS runtime, gateway, certificates, worker
dispatch, and the plugin bridge. It exposes one public API over a C ABI and
owns no event loop.

Anything a shell can do, an existing native app can do, so embedding appd in
another application works by the same route.

### Shell languages

| Platform | Shell | Plugins register through |
|---|---|---|
| macOS, iOS | Rust with `objc2` | a Swift package over the C ABI |
| Android | Kotlin | the Kotlin shell library |
| Windows, Linux | Rust | the Rust crate |

Every shell inverts. Only Android changes language, because its platform calls
are JNI descriptor strings that no compiler checks. Apple shells stay Rust
because `objc2` is already checked, and plugin authors get Swift from a
package rather than a shell rewrite.

### API tiers

The stable surface is lifecycle, configuration, worker dispatch, bridge
registration, events, and hooks. Everything else is internal.

Plugins use a subset. A plugin cannot start, stop, or reconfigure the runtime,
and cannot change certificate handling, origin routing, or client-certificate
verification.

## Plugins

A plugin is frontend or backend, never both.

- Frontend plugins run in the page: the browser on web, the WebView on native.
- Backend plugins run in the worker: Cloudflare on web, Bare on native.

appd does not connect the tiers. An app that collects something on one tier
and needs it on the other sends it in a request.

### Writing one

One TypeScript entrypoint defines the whole plugin. There is no manifest and
no separate types file.

```ts
export default class Geolocation extends BackendPlugin<Impl> {
  static id = "geolocation";
  current() { return this.call("current"); }
}
```

- The base class, `FrontendPlugin` or `BackendPlugin`, sets the kind.
- `id` is what app code imports.
- The class's public methods are the API, typed for consumers.
- `Impl` names the methods and streams each platform implements. Its values
  are primitives, plain objects, arrays, and buffers.
- `call` and `subscribe` reach the platform implementation.

Shared logic — validation, caching, author-chosen fallbacks — goes in the
entrypoint and is written once.

### Platform implementations

```text
<package>/
  index.ts
  web/ macos/ ios/ windows/ android/ linux/
```

A platform is supported if its directory exists. A misspelled directory is a
build error. A missing platform still builds and type-checks; calls throw
`PluginNotImplemented` naming the plugin and platform.

`web/` is an ES module. A native directory holds a build manifest naming a
build command, an artifact, and a registration entry point, plus whatever
sources that command needs. appd runs the command with target metadata in the
environment and links the result. It never reads the sources, so any language
works.

### How calls reach the implementation

An appd Vite plugin resolves `appd:plugin/<id>`. Frontend bundles get frontend
plugins and worker bundles get backend plugins; the wrong pairing fails the
build. Resolution is Vite-based; other bundlers are out of scope.

| Target | Route |
|---|---|
| web | direct call into `web/` |
| native backend | C ABI bridge, no encoding, buffers zero-copy |
| native frontend | WebView IPC, origin-gated, payloads encoded |
| unimplemented | throws `PluginNotImplemented` |

## Rules

- App code imports `appd:plugin/<id>` and contains no platform checks.
- A plugin's metadata, API, and types live only in its entrypoint.
- Frontend plugins cannot be imported by worker code, and backend plugins
  cannot be imported by page code.
- The shell owns the process, window, and OS lifecycle; appd owns no event
  loop.
- Plugins cannot reconfigure the runtime or its security behaviour.
- appd builds native plugin code only through the declared command and
  outputs.
- Every API addition ships with something that uses it.

## Deliverables

In order. macOS and iOS lead. Plugins come last, including the web-only phase,
so the plugin contract is set against a finished API and the web path matches
every other platform.

**0. Surface artifacts.** So an API cannot grow without someone noticing.

Generate and commit an API listing for every public surface, and fail CI when
code and listing disagree. `cargo public-api` covers the crates and API
Extractor covers the TypeScript packages; the C ABI header, Swift baseline,
and Kotlin dump join as those surfaces appear.

Done when a public change without its regenerated listing fails CI.

**1. appd becomes a library.** So shells drive appd, giving plugins an API to
register against.

Define the API — start, stop, suspend, resume, configuration, worker
dispatch — over the C ABI, and move the entry point and event loop into each
shell. macOS and iOS first, then Windows and Linux. Android inverts straight
into a Kotlin shell, replacing its JNI descriptor calls in the same pass.

Done when every shell runs appd through the public API and appd users see no
change.

**2. Lifecycle hooks.** So shell and plugin code can run at defined points in
startup and lifecycle.

Hook points for before start, gateway ready, suspend, resume, and shutdown,
with defined ordering, threading, and failure behaviour.

Done when hooks are registerable through the public API on every platform,
with that behaviour tested.

**3. Plugins.** Four phases, each shipping independently.

*A — the model and the web path.* `@appd/plugin`, plugin declaration in appd
config, Vite resolution and tier enforcement, and web implementations of both
kinds. Done when a web-only plugin works on web deployments and throws
`PluginNotImplemented` on native targets.

*B — native code the worker can call.* The bridge channel, native build
orchestration, and the Swift and Kotlin registration packages. macOS and iOS,
then Android. Done when a native handler round-trips a buffer without copying
or encoding, and a plugin written in a language appd has never seen builds
from its declared command.

*C — native code the page can call.* Origin-gated WebView IPC in each shell.
Done when one app imports a frontend plugin and runs on web, macOS, iOS, and
Android with no platform-conditional code.

*D — capabilities that act with no request in flight,* such as geofence wakes
and Bluetooth reconnects. Worker dispatch without a socket, built on the hooks
from deliverable 2, plus an example plugin covering every case and CLI
scaffolding. Done when an OS-woken plugin delivers an event into worker code.

## Deferred

- Generating Info.plist entries, Android manifest permissions, and
  entitlements from plugin metadata.
- How a plugin declares the runtime API version it needs, and what a mismatch
  does.
- Service workers on web, needed for web push and background sync.
- Plugins owning a WebView or window.
- Reconciling `appd-webview-proxy.md`, which assumes Rust platform adapters
  own WebView configuration.
