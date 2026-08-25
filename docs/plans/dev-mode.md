# appd development mode

Status: proposal

Scope: developer tooling only. Production builds continue to execute packaged
QuickJS bytecode without Vite, a development connection, or runtime source
evaluation.

Related plans:

- [appd QuickJS runtime](appd-quickjs-runtime.md)
- [Workers and Node compatibility](appd-workers-node-compat.md)

## Objective

`appd dev <platform>` builds, installs, and launches one development app. Code
and configuration edits then reach that running app without another native
build or install.

| Edit | Result |
|---|---|
| Client module or style | Vite HMR updates the WebView in place where the framework supports it. |
| Worker module | New requests use a new Worker generation; the page reloads after the runtime accepts it. |
| Wrangler vars or supported bindings | New requests use the new configuration generation. |
| Public asset | Vite serves the new asset and reloads or invalidates the client as required. |
| Native shell, appd runtime, or native plugin | The development app rebuilds, reinstalls, and restarts. |

The initial target is a sub-second client update and a sub-second Worker
generation update for the Astro example on a local simulator or desktop.

## Runtime model

The QuickJS runtime creates one `JSRuntime` and `JSContext` per request. Each
request evaluates its own Worker modules and owns its module state, native
resources, and `/tmp`. A WebSocket keeps its request runtime alive until the
socket closes.

Development mode preserves that model. It does not keep one shared Worker
realm or patch live QuickJS module instances. Frontend updates use HMR;
backend updates replace an immutable Worker generation.

Each request clones the current generation before creating QuickJS. An update
atomically replaces the current `Arc`; existing HTTP requests and
`waitUntil()` work retain the previous generation until they finish. Worker
WebSockets from an older generation close with code `1012` so reconnecting
clients enter the current generation.

## Architecture

```text
project files
    |
    v
Vite dev server + @appd/vite
    |- client environment ---- client modules and HMR
    `- Worker environment ---- immutable DevGeneration
                |                         |
                `------ pinned HTTPS ----'
                                          v
                                  development app
                                  |- appd HTTPS gateway
WebView <---- stable appd origin ---------|- dev asset/HMR proxy
                                  `- QuickJS runtime per request
                                       `- request-local ModuleRunner
```

`appd dev` owns the session, native app lifecycle, and platform tooling. A
small `@appd/vite` package owns Vite integration. The target runtime owns the
development connection, generation swap, request execution, and WebView
proxy.

`@appd/vite` is configured in the project's Vite or framework configuration.
appd framework integrations and templates add it; `appd dev` fails if the
plugin does not join the session.

### Vite environments

One Vite server owns:

- the normal `client` environment; and
- an appd Worker environment, normally the framework's `ssr` environment for
  a full-stack application.

The Worker environment uses the Vite Environment API for resolution,
transforms, watching, its module graph, and source maps. The appd plugin
receives the runtime's build profile from the selected target pack. That
profile contains the same export conditions, builtin registry, compatibility
module aliases, externals, and non-JavaScript loaders used by `appd build`.

The CLI supplies normalized Wrangler bindings and asset configuration. The
framework integration supplies its development Worker entry and SSR
environment. The Astro integration executes that environment only in the
device QuickJS runtime; the host Vite process never dispatches application
requests.

The Worker environment must not externalize ordinary packages or host file
URLs. Runtime-owned native modules are the only external modules, and their
names must exist in the runtime builtin registry. An unsupported import is a
transform error before a generation is published.

The Vite version and ModuleRunner protocol are pinned per appd release. The
plugin rejects an unsupported Vite version instead of relying on compatible
internal behaviour.

### Development generations

The appd plugin materializes the Worker environment into one immutable
generation:

```text
DevGeneration
  id
  Worker entry URL
  transformed ModuleRunner payloads
  source maps
  normalized Worker environment
  read-only /bundle files
  builtin registry version
  Vite ModuleRunner protocol version
```

A generation contains UTF-8 transformed source, never QuickJS bytecode or
serialized QuickJS objects. Only bytecode shipped by the selected target pack
uses QuickJS's trusted bytecode loader.

The plugin warms the Worker entry and walks the environment module graph,
including statically known dynamic imports. It obtains full `fetchModule`
payloads without evaluating application code. Publication succeeds only when
every reachable module transforms and every external import validates.

The host sends each generation as one complete compressed payload. The app
validates size limits, module names, protocol versions, and the builtin
registry version before acknowledging it.

The runtime stores development state as a standard-library lock around an
immutable value:

```text
DevelopmentWorker {
  current: RwLock<Arc<DevGeneration>>
}
```

The write lock is held only while replacing the `Arc`. No request can observe
a partially updated graph or configuration. Previous generations are freed
when their final request or WebSocket releases them.

### QuickJS ModuleRunner

The development target pack contains a precompiled, pinned
`vite/module-runner` and a small appd bootstrap. Distribution target packs do
not contain either.

Every request:

1. clones the current `DevGeneration`;
2. creates the normal request-owned QuickJS runtime, context, host bindings,
   VFS, and compatibility globals;
3. creates a request-local Vite `ModuleRunner` with HMR disabled;
4. imports the generation's Worker entry through an in-memory transport; and
5. dispatches `fetch` using the generation's environment.

The in-memory transport implements Vite's `fetchModule` RPC by looking up the
serialized payload in the pinned generation. Worker JavaScript never opens a
development socket and a request never fetches modules from the host.

An appd evaluator uses `AsyncFunction` for transformed modules and resolves
external modules only through QuickJS's runtime-owned builtin loader. It adds
stable module URLs and inline source maps so QuickJS errors can be mapped by
the host Vite environment.

Development requests parse their transformed modules in their request-owned
QuickJS runtime. The runtime-boundary proof measures that cost before the
architecture is accepted.

### Generation commit

A Worker edit follows one ordered commit:

1. Vite invalidates and transforms the affected Worker graph.
2. The appd plugin creates a complete candidate generation.
3. The development app validates and atomically installs it.
4. The app acknowledges the generation ID.
5. Vite sends one client full-reload event.

The acknowledgement prevents the WebView from reloading before the new
Worker is ready. A client-only edit remains on Vite's normal HMR path and does
not create a Worker generation unless the changed module is also in the
Worker graph.

A transform or configuration error leaves the last accepted generation
running. The plugin reports the error in the terminal and through Vite's
browser overlay. On initial startup, the shell waits for the first valid
generation before loading the app origin.

## Stable WebView origin

The WebView always loads `https://<appname>.appd.local/`. Cookies, storage,
service workers, client certificates, origin checks, and application
WebSockets therefore use the same origin in development and production.

The development gateway adds two internal routes:

- a development asset provider for Vite client modules and public files; and
- a reserved WebSocket route for Vite HMR.

The asset provider asks the client environment to handle asset requests and
falls through to normal appd asset and Worker routing when Vite does not
handle one. `env.ASSETS.fetch()` uses the same provider. The host must not run
the application's Worker or SSR handler as an asset fallback.

The HMR WebSocket terminates at the appd gateway and is proxied to Vite. Vite
is configured through its current `server.ws` options so the injected client
uses the appd host, `wss`, port `443`, and the reserved path. The browser never
connects directly to a host or LAN address.

The development asset and HMR branches are absent from distribution builds.

## Development session and security

`appd dev` creates an ephemeral session before starting Vite:

- a random 256-bit token;
- an HTTPS certificate and its SHA-256 fingerprint;
- a protocol version and appd release identifier; and
- the device-reachable host and port.

The CLI passes the session descriptor to `@appd/vite`, starts the project's
normal development command, then packages the endpoint, token, and pinned
certificate fingerprint into the development app. This build-time bootstrap
is sufficient because code edits reuse the same installed app and session.

The development app uses the pinned HTTPS service for generation updates,
asset proxy requests, HMR proxying, and runtime diagnostics. The gateway adds
the token to host requests; browser JavaScript cannot read it. The host
service validates the token before returning source, transformed modules, or
HMR traffic.

The service binds to loopback unless the selected device requires LAN access.
LAN binding requires an explicit address, retains certificate pinning and
token authentication, and configures an exact Vite allowed-host list. It
never enables unrestricted hosts or CORS.

Only a development runtime accepts the session descriptor or evaluates Vite
payloads. A release runtime has no development protocol, outbound connection,
asset proxy, ModuleRunner, or downloaded-code path.

## Platform launch

The user command mirrors `appd build` platform selection:

```text
appd dev macos
appd dev windows
appd dev ios-simulator
appd dev ios
appd dev android
```

Target-pack entrypoints own platform-specific build, install, launch, and
port-forwarding commands. The Rust CLI continues to prepare common inputs and
does not acquire Xcode, Android, or Windows implementation details.

Desktop and iOS Simulator sessions use host loopback. Android uses `adb
reverse` when available. A physical iOS device and any device without reverse
port forwarding use the explicit LAN endpoint. Physical-device support is
part of the protocol and security tests.

Target packs contain separate development runtime artifacts. Native plugin or
shell changes invalidate the installed app and invoke the entrypoint's native
rebuild path. JavaScript and supported configuration changes do not invoke a
target-pack build entrypoint.

## Configuration and lifecycle

The Vite watcher includes the resolved Wrangler configuration, supported dev
vars, Vite configuration, and files imported by either configuration.

- A vars or binding change publishes a generation containing code and its
  normalized environment together.
- A resolution or Vite-plugin configuration change restarts the host Vite
  server, then publishes a new generation through the existing app session.
- An app name, bundle identifier, native permission, or native plugin change
  rebuilds and reinstalls the development app.
- Host disconnection keeps the last Worker generation available, marks the
  asset/HMR provider unavailable, and reconnects with a bounded retry loop.
- App suspend closes development proxy connections with the gateway's other
  connections. Resume reconnects before the WebView reloads.

The runtime forwards Worker console output and uncaught errors to the appd
terminal with generation and request IDs. Source-mapped Worker errors use the
same Vite overlay channel as transform errors. Error payloads contain no
Worker environment secrets.

## Production parity

`appd build` remains the only production build path. It bundles the Worker,
compiles split ESM modules to trusted QuickJS bytecode, packages assets and
the normalized environment, and runs without Vite.

`appd preview <platform>` builds that production package and runs it in a
development-signed shell without ModuleRunner or asset proxying. Differential
fixtures execute the same requests through `dev` and `preview`; the results,
errors, headers, request isolation, VFS lifetime, and supported bindings must
match after normalizing development source locations.

The runtime-owned builtin registry and Worker build profile are shared by the
production packer and `@appd/vite`. Vite does not maintain a second Node or
Workers compatibility table.

## Implementation sequence

### 1. Prove the runtime boundary

1. Run the pinned `vite/module-runner` in `rquickjs` with the appd evaluator.
2. Import a transformed multi-module Worker, top-level await, a dynamic
   import, a compatibility JavaScript module, and native `node:fs`.
3. Materialize a complete Vite Worker graph without evaluating application
   code.
4. Run the Astro SSR environment in device QuickJS without dispatching it in
   the host Vite process.
5. Execute concurrent requests in independent QuickJS runtimes from one
   generation and verify module globals and `/tmp` remain request-local.
6. Measure import, evaluation, request, and generation-publication time with
   the Astro Worker graph.

Stop if the ModuleRunner cannot preserve request isolation, import native
modules, produce usable source maps, or meet the development memory envelope.

### 2. Add runtime generations

1. Add the development-only generation format and strict decoder limits.
2. Add the packaged/development Worker mode to runtime startup.
3. Pin one generation per request and atomically swap accepted generations.
4. Drain old HTTP work and close old-generation Worker WebSockets with `1012`.
5. Cover normal completion, errors, cancellation, suspend, resume, shutdown,
   and a generation arriving during each state.

### 3. Add the Vite provider

1. Create `@appd/vite` with the client and Worker environments.
2. Consume the target pack's Worker build profile and builtin registry.
3. Materialize, compress, publish, and acknowledge complete generations.
4. Watch Worker configuration and publish code and environment atomically.
5. Keep the last accepted generation on transform failure and forward errors
   with source maps.

### 4. Add same-origin client development

1. Add the development asset provider and `env.ASSETS.fetch()` integration.
2. Proxy Vite HTTP responses and WebSocket frames through the appd gateway.
3. Configure the Vite client and HMR WebSocket for the stable appd origin.
4. Reload the client only after a Worker generation acknowledgement.
5. Verify cookies, storage, navigation, client HMR, Worker WebSockets, and
   external fetches through the normal WebView.

### 5. Add CLI and platform launch

1. Add `appd dev`, session creation, child-process supervision, and clean
   shutdown.
2. Add development artifacts and build metadata to target packs.
3. Implement build/install/launch for macOS and iOS Simulator.
4. Add Android emulator/device, physical iOS, and Windows launch paths.
5. Add `appd preview` over the packaged QuickJS Worker path.

## Verification

- Unit tests reject wrong tokens, certificate pins, protocol versions,
  registry versions, duplicate modules, invalid module names, oversized
  generations, and truncated payloads.
- Concurrency tests update a generation during module import, handler
  execution, response streaming, `waitUntil()`, and a WebSocket session. Each
  execution observes exactly one generation.
- Failure tests keep the last accepted generation after syntax, transform,
  configuration, transport, and runtime errors.
- Release-binary tests verify that development session parsing, ModuleRunner,
  and proxy routes are not linked into distribution artifacts.
- End-to-end tests edit client, Worker, shared, asset, and configuration files
  in the Astro example and observe the running WebView without reinstalling.
- Platform tests cover macOS, both iOS Simulator architectures, physical iOS,
  Android arm64, and Windows x64 with the device transport each target uses.
- Differential tests compare development execution with the production
  QuickJS bytecode package for the Workers compatibility fixtures.
- Performance tests record cold start, client HMR, Worker publication,
  request evaluation, payload size, retained generations, and memory before
  enforcing the sub-second targets.

## Acceptance criteria

- A client edit updates the running Astro app through Vite HMR without an app
  reinstall.
- A Worker edit reaches the next request only after one complete generation
  is accepted, then reloads the page once.
- A failed edit leaves the last valid application running and displays a
  source-mapped error in the terminal and WebView.
- Concurrent requests never share QuickJS globals, module caches, native
  resources, environment objects, or `/tmp` across generations.
- The WebView remains on the stable appd HTTPS origin and never connects
  directly to Vite.
- Distribution builds contain no Vite runtime, development endpoint,
  development credential, or remote source-evaluation path.
- The Astro example passes the same application fixtures in `appd dev` and
  `appd preview` on every supported target.

## References

- [Vite Environment API for runtimes](https://vite.dev/guide/api-environment-runtimes)
- [Vite environment instances](https://vite.dev/guide/api-environment-instances)
- [Vite server and WebSocket options](https://vite.dev/config/server-options)
- [Cloudflare Vite environments](https://developers.cloudflare.com/workers/vite-plugin/reference/vite-environments/)
- [QuickJS-NG developer guide](https://quickjs-ng.github.io/quickjs/developer-guide/intro/)
- [`rquickjs` documentation](https://docs.rs/rquickjs/latest/rquickjs/)
