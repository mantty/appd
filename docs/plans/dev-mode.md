# appd development mode

Status: proposal

Scope: developer tooling only. Production builds continue to execute packaged
QuickJS bytecode without Vite, a development connection, or runtime source
evaluation.

Related plans:

- appd QuickJS runtime (appd-quickjs-runtime.md)
- Workers and Node compatibility (appd-workers-node-compat.md)
- appd plugins (appd-plugins.md)

## Objective

appd dev <target> builds, installs, and launches one development app while
running the developer's existing Cloudflare-compatible framework command on the
host. The running app uses the framework's normal development server, Worker
runtime, HMR, overlays, source maps, and debugging tools.

The target is a single selector. It may be an appd-managed alias such as `ios`
or `android`, the native identifier of an existing simulator, emulator, or
physical device, or the host platform name (`macos`, `windows`, or `linux`). Dev
mode does not accept a separate platform argument and device flag; the selected
target determines the platform and native target pack.

The command may be supplied explicitly so appd does not need to know which
framework is being used:

~~~
appd dev ios -- astro dev
appd dev android -- vite dev
appd dev <simulator-or-device-id> -- vinext dev
~~~

The project may also define the command and host development-server endpoint in
appd project configuration. appd dev is a supervisor and device adapter; it
does not replace the framework CLI.

The host server endpoint defaults to `http://localhost:5173`. Commands using a
different port pass it before the command separator, such as
`appd dev macos --server http://localhost:4321 -- astro dev`. A physical iOS
target additionally requires `--host-address` for the host address reachable
from the device.

| Edit | Result |
|---|---|
| Client module or style | The framework's normal Vite HMR updates the WebView through the appd gateway. |
| Worker or SSR module | The host framework server reloads its Worker environment; subsequent requests use the new code. The framework decides whether a client reload is needed. |
| Wrangler vars or binding configuration | appd validates the configuration before startup. A supported change restarts or reloads the host dev server; one prominent warning block lists unsupported bindings while the session continues. |
| Public asset | The framework dev server serves the new asset and performs its normal invalidation or reload. |
| Native shell, appd runtime, or native plugin | The development app rebuilds, reinstalls, and restarts. |

The target is the same responsive client experience provided by the framework
when it is run locally in a browser, including near-instant style and client
module updates.

## Runtime model

Development backend execution deliberately happens in Cloudflare's local
workerd runtime through the framework's Cloudflare/Vite setup. This is the
normative development environment for Worker-compatible code.

The appd QuickJS runtime is not on the development request path. It remains the
production runtime and is exercised by appd preview and compatibility tests.
QuickJS-only globals, appd-only backend internals, and other code that is not
valid in workerd should fail during development rather than being silently
accepted and discovered later on another platform.

This makes development particularly useful for applications that target
Cloudflare as well as appd: the backend is running in the same class of runtime
and using the same framework integration as the Cloudflare deployment.

## Architecture

~~~
project files
    |
    v
developer's normal framework command on the host
(astro dev, vite dev, vinext dev, ...)
    |
    +-- Vite client environment ----- browser modules and HMR
    |
    +-- Cloudflare Worker environment
          |
          +-- local workerd / Miniflare
                |
                +-- appd capability binding (when used)
                       |
                       authenticated host-to-device tunnel
                       |
device appd gateway ---------------- WebView
    |                                  |
    |                                  +-- frontend plugin bridge -> native plugin
    +-- HTTP, SSR, API, asset, and HMR proxy -> host dev server

device native plugins
    +-- backend capability requests from the host Worker
~~~

The host framework server is the complete development application server. It
handles frontend assets, SSR, API routes, Worker requests, and HMR. The device
does not receive transformed module payloads and does not run a development
ModuleRunner.

### Responsibilities

appd dev owns:

- project and target-pack lifecycle;
- Wrangler configuration discovery and appd binding validation;
- the authenticated development session and host/device tunnel;
- the device gateway and stable WebView origin;
- native plugin dispatch and platform launch tooling; and
- child-process supervision, signal forwarding, and clean shutdown.

The developer's framework command owns:

- Vite and framework configuration;
- module resolution and transformation;
- the client and Worker module graphs;
- HMR, browser overlays, source maps, and framework debugging; and
- execution of the backend in local workerd.

Appd does not add Astro-, Next-, React-, or other framework-specific request
handlers. A single optional appd Cloudflare/Vite adapter may be used to expose
the native capability binding to the host Worker; it is not a framework
adapter.

### Framework command wrapper

The appd supervisor starts the developer's command as a normal child process:

- inherit the terminal and preserve stdout/stderr;
- forward interrupt, terminate, and platform-specific shutdown signals;
- preserve the framework's exit status and debugger support;
- provide the appd session descriptor and device bridge endpoint through the
  environment or a temporary session file; and
- connect the device gateway to the configured host development-server URL.

Appd must not parse framework log output to implement HMR or backend execution.
The host URL and port are explicit configuration or are reported through a
small generic readiness contract. The framework process remains the process
developers interact with.

## Target discovery and selection

Dev mode accepts one target selector, not a platform plus a separate device
option:

~~~
appd dev macos
appd dev ios
appd dev android
appd dev <simulator-or-device-id>
~~~

The selector is resolved against an inventory assembled by the platform
adapters. It may identify:

- the host platform name (`macos`, `windows`, or `linux`), selecting the local
  desktop target;
- an appd-managed virtual-target alias such as `ios` or `android`;
- an existing iOS Simulator UUID, Android AVD name, Android ADB serial, or
  physical-device identifier; or
- another concrete identifier returned by a supported platform adapter.

The platform is metadata on the resolved target. It is never repeated in the
command, and native identifiers are not required to carry a platform prefix.
If an identifier matches more than one target, appd reports the ambiguity and
prints the matching rows; it never chooses at random.

`appd devices` lists both concrete targets and provisionable virtual-target
aliases in the same table. The output keeps the user-facing columns to an ID,
the target type, and a short status. The default status vocabulary is
deliberately small:

~~~
ID                    Type                              Status
macos                 macOS desktop                     available
ios                   managed iOS Simulator              available
android               managed Android emulator            available
00008110-...          physical iPhone                    blocked: trust device
A1B2C3D4-...          iPhone 15 / iOS Simulator          available
Pixel_8_API_35        Android emulator (AVD)              available
emulator-5554         Android emulator                   available
~~~

`available` includes targets that are already running, can be booted, or can
be provisioned automatically. A `blocked` row includes one short actionable
reason, such as a missing runtime or system image, an unsupported host, an
unauthorized physical device, or an incompatible architecture. Low-level
transitions such as `booting` are progress messages from `appd dev`, not
additional user-facing states.

The default inventory omits valid but never-booted iOS Simulator records, which
are commonly created automatically for every installed Xcode runtime. A
simulator is listed when it is available and either currently booted or has a
non-empty `lastBootedAt` value. The managed `ios` and `android` aliases remain
visible so a fresh target can be provisioned without selecting an arbitrary
existing profile.

### Managed virtual targets

`ios` and `android` are requests for an appd-managed default
target, not claims that a particular instance is already running. The adapter
does the following when the alias is selected:

1. Verify that the host toolchain and a compatible simulator runtime or Android
   system image are installed.
2. Reuse the project’s persisted managed target when it still exists.
3. Create a deterministic default profile when no managed target exists, then
   persist the native identifier returned by the platform tooling.
4. Boot the profile if it is stopped, wait for readiness, and return its
   concrete running identifier to the session.

Managed profiles are local development resources. They persist after appd exits
and are reused by later sessions; appd stops the app, not the simulator or
emulator, unless the user explicitly asks it to clean up. If a managed profile
is deleted outside appd, the next alias invocation recreates it. The project
mapping is local state and is not required to be committed to the repository.

Creating a profile must not silently download a large platform runtime or
system image. If the required image is absent, the alias is `blocked` with an
instruction to install it. The profile defaults (device model, runtime, and
ABI) must be deterministic, and a project may override them in appd
configuration when it needs a specific device shape.

Selecting a concrete identifier uses that existing target exactly. Appd may
boot a stopped simulator or emulator, but it does not replace, rename, or
delete the user’s profile. Physical devices are discovered and connected, not
provisioned.

The target resolver and adapters own host capability checks, profile creation,
boot, installation, launch, and forwarding. The Rust supervisor only consumes
the resolved target record and keeps the development session bound to that
target. A disconnect never silently switches the session to another target.

## Wrangler configuration and binding validation

The appd repository already resolves and parses Wrangler JSON, JSONC, and TOML
files in `appd/src/wrangler_config.rs`. Development mode reuses
`resolve_wrangler_config_path()` and `load_wrangler_config()` rather than
adding another config parser.

The current normalized WranglerConfig intentionally retains only the fields
needed by packaging (main, assets, vars, and rules). Extend that model with the
binding declarations needed for development validation. Each declaration
records its binding kind and name; the validator compares it with the appd
development support profile.

Startup validation is ordered as follows:

1. Resolve the explicit --config path or the project Wrangler file using the
   existing resolver.
2. Select the requested Cloudflare environment when the project uses
   environment-specific bindings.
3. Validate all declared binding kinds and names against appd's supported set.
4. Print one prominent warning block explaining that unsupported bindings may
   still be conditionally unused, followed by every unsupported binding's name,
   kind, and reason.
5. Continue launching the framework command after the warning block is emitted.

For example:

~~~
WARNING: this app declares bindings that appd dev does not provide.

The host development server will continue. Avoid these bindings when running
on appd, or guard their use with the appropriate platform or feature flag.

Unsupported bindings:

  - DB (d1_databases): appd development does not provide D1
  - CACHE (kv_namespaces): appd development does not provide KV

~~~

The Wrangler file is the appd development contract for bindings. Bindings
added only through framework-specific programmatic configuration are not
silently treated as supported; they must either be represented in the resolved
Wrangler configuration or appear in the same warning block when the Worker
starts. The warning is informational, because application code may intentionally
gate the binding behind a platform or feature check. This avoids requiring appd
to understand each framework's configuration language.

The appd validator must never reject ordinary Cloudflare bindings merely
because they are not native. The support profile distinguishes bindings that
the host Cloudflare runtime can provide from bindings that require a device
capability bridge or are unavailable for appd deployment.

## Backend and frontend plugins

### Frontend plugins

Frontend plugin JavaScript executes in the WebView exactly as it does in a
normal browser. Existing appd WebView bridges continue to dispatch native
calls to Swift, Kotlin, or another platform implementation on the device.
Client HMR does not change this path.

### Backend plugins

Backend JavaScript executes in host workerd. A backend native capability is
exposed as a Worker-compatible appd binding or service. The host-side binding
turns the call into an authenticated RPC over the appd tunnel; the device
dispatches it to the native plugin and returns the result.

~~~
Worker request in host workerd
    +-- appd Worker binding/service
    +-- host bridge
    +-- authenticated tunnel
    +-- device appd plugin dispatcher
    +-- Swift/Kotlin/native implementation
~~~

The application-facing plugin API remains the same between development and
production. The JavaScript facade must itself be valid Worker code. Backend
plugin JavaScript that depends on QuickJS-only behavior is intentionally not
supported in this development mode; it should fail on the host instead of
creating a second device-side JavaScript execution path.

The capability bridge must support:

- request and session authentication;
- JSON-compatible values and binary payloads where a plugin requires them;
- streaming and long-running calls where the API requires them;
- cancellation, timeout, and device disconnect errors;
- request/session identity; and
- the same unsupported-capability error used by production appd.

The browser is never given the backend capability credential. Frontend native
calls use the WebView process bridge; backend native calls use the authenticated
host/device channel.

## Stable WebView origin and proxying

The WebView always loads:

~~~
https://<app-name>.appd.local/
~~~

The device gateway proxies the entire application origin through the host
development server:

- document and navigation requests;
- frontend modules and public assets;
- SSR and API requests;
- fetch() and form submissions;
- browser WebSockets; and
- the framework's HMR WebSocket.

The gateway preserves the stable appd origin for cookies, storage, service
workers, origin checks, redirects, and forwarding headers. It forwards the
appropriate host/protocol information so the Worker does not accidentally
generate localhost URLs for the device.

The HMR client connects to the appd origin. The gateway relays the WebSocket to
the host framework server; the browser never connects directly to a host or LAN
address. Appd does not interpret HMR messages.

The host framework server remains responsible for deciding whether an edit is
handled in place, causes a client reload, or causes a Worker/SSR module reload.

## Development session and security

appd dev creates an ephemeral session before launching the framework command:

- a random session token;
- an authenticated, device-reachable transport;
- a protocol version and appd release identifier; and
- the app origin and host development-server endpoint.

The device app is built with only the session information required to connect.
The gateway authenticates every host request and WebSocket. The host bridge
authenticates every capability call and scopes it to the app and device
session.

The host service binds to loopback unless the selected device requires LAN
access. LAN access uses explicit address selection, authentication, and
certificate pinning. It never enables unrestricted hosts or unauthenticated
native capability endpoints.

If the host process exits or the tunnel disappears, the device reports the
development session as unavailable and retries with bounded backoff. It does
not execute stale downloaded Worker source on the device.

## Platform launch

The user selects one target. Platform selection is inferred from the resolved
target rather than repeated in a separate argument:

~~~
appd dev macos
appd dev ios
appd dev android
appd dev <simulator-or-device-id>
~~~

The host platform name (`macos`, `windows`, or `linux`) selects the local desktop
target. `ios` and `android` select the project’s managed virtual targets,
creating them when the required host runtime or system image is already
installed. A concrete identifier selects an existing simulator, emulator, or
physical device.

Platform adapters expose a common inventory and lifecycle contract: enumerate
capabilities and target profiles, resolve a selector, create or boot a managed
profile when requested, install and launch the native bundle, and establish
port forwarding. Target-pack entrypoints own the platform-specific commands;
the Rust CLI owns common session and project logic and does not acquire Xcode,
Android, or Windows implementation details.

The adapter checks host and target compatibility before building or installing.
Unsupported host/toolchain combinations appear as a `blocked` target with a
short reason. A physical iOS device and any device without reverse port
forwarding use an explicit authenticated LAN endpoint.

Desktop and iOS Simulator sessions use host loopback. Android uses adb reverse
when available.

Native shell or plugin changes invalidate the installed app and invoke the
entrypoint's native rebuild path. JavaScript, styles, assets, and supported
Worker configuration changes do not invoke a target-pack build.

## Configuration and lifecycle

The appd supervisor watches the resolved Wrangler configuration and the
framework process reports its own Vite/framework configuration changes.

- A supported code or asset edit is handled by the host framework server.
- A supported vars or binding change restarts or reloads that host server using
  its normal mechanism.
- An unsupported binding declaration or change re-emits one warning block with
  the current unsupported-binding list; the session remains usable for code
  paths that do not use those bindings.
- A change to the app name, bundle identifier, native permission, native
  plugin, or shell requires a native rebuild and reinstall.
- Host disconnect keeps the native app installed but marks the WebView's
  development origin unavailable until the session reconnects.
- Desktop focus changes leave the appd runtime and proxy running. Mobile
  backgrounding leaves the process to the operating system; on foreground the
  shell probes the gateway and, if its port changed, rebinds the proxy and
  reloads the WebView.

Worker console output and uncaught errors remain in the framework's normal host
terminal and debugger. Appd forwards connection and native-plugin errors with
request/session IDs, without exposing Worker secrets to the browser.

## Production parity

appd build remains the production build path. It bundles the Worker, compiles
split ESM modules to trusted QuickJS bytecode, packages assets and the
normalized environment, and runs without Vite or a development connection.

appd preview <platform> runs that packaged QuickJS application in a
development-signed shell. It is not an HMR server; it is the final appd-runtime
compatibility check.

Development and preview deliberately exercise different runtime implementations:

- appd dev: framework tooling and host workerd, optimized for Cloudflare
  compatibility and fast feedback;
- appd preview: appd's packaged QuickJS runtime, optimized for deployment
  parity.

Compatibility fixtures should run through both paths. QuickJS-specific
behavior must not be used as an implicit development API.

## Implementation sequence

### 1. Extend Wrangler loading and validation

1. Extend WranglerConfig with the binding declarations needed by appd's
   support profile.
2. Preserve the existing JSON, JSONC, TOML, explicit-path, and parent-search
   behavior.
3. Add environment selection for the same Wrangler environment used by the
   host framework command.
4. Add one prominent warning block explaining unsupported bindings and listing
   each binding's name, kind, and reason.
5. Test that the warning block is emitted before the framework child process
   starts and that the child still launches.

### 2. Add the generic development supervisor

1. Add `appd dev <target>` and `appd devices` with one target-selector
   contract.
2. Add platform-adapter discovery, host capability checks, and concrete target
   resolution.
3. Add managed simulator/emulator creation, persistence, reuse, and boot.
4. Create the session descriptor and secure host/device transport.
5. Launch the developer's command with inherited terminal behavior.
6. Forward signals, child exit status, and clean shutdown.
7. Add readiness and reconnect handling without parsing framework log output.

### 3. Add the device gateway

1. Serve the stable appd HTTPS origin.
2. Proxy host HTTP and WebSocket traffic, including HMR.
3. Preserve origin, cookie, redirect, and forwarding-header behavior.
4. Add device-to-host port forwarding for desktop, simulators, emulators, and
   physical devices.
5. Verify that the WebView never connects directly to the host endpoint.

### 4. Add the capability bridge

1. Define the Worker-compatible appd binding/service contract.
2. Provide the host-side bridge for the Cloudflare/Vite Worker environment.
3. Route authenticated calls to the device native plugin dispatcher.
4. Add streaming, cancellation, timeout, and disconnect behavior.
5. Keep frontend plugin calls on the existing WebView process bridge.

### 5. Validate representative frameworks

1. Run the Astro Cloudflare setup through appd dev without replacing
   astro dev.
2. Run a plain Vite + Cloudflare Worker application.
3. Run the agreed Next/Cloudflare setup through its existing dev command.
4. Verify that no framework-specific appd request handler is required.
5. Add framework-specific work only when a generic Cloudflare/Vite contract is
   insufficient and document the reason.

## Verification

- Wrangler parser tests cover JSON, JSONC, TOML, explicit paths, parent search,
  selected environments, and every supported/unsupported binding category.
- Unsupported bindings produce one deterministic warning block with a list of
  every unsupported binding, while the child process still launches.
- Device discovery tests list managed aliases, installed simulator/emulator
  profiles, running instances, and physical devices with an ID, type, and
  `available` or `blocked` status.
- Target-selection tests resolve aliases and concrete identifiers without a
  separate platform argument, reject ambiguous identifiers, and never select a
  target at random.
- Managed-target tests create a missing compatible simulator/emulator profile,
  persist and reuse it, boot it when stopped, and report a short blocked reason
  when the required host runtime or system image is absent.
- Supervisor tests cover inherited output, signals, exit status, readiness,
  reconnect, cancellation, and shutdown.
- HTTP proxy tests cover navigation, assets, API/SSR requests, headers,
  cookies, redirects, streaming, and application WebSockets.
- HMR tests edit styles, client modules, shared modules, and public assets in a
  device WebView and observe the framework's normal update behavior.
- Worker tests verify that backend code executes in host workerd, uses the
  latest host module graph, and never executes in device QuickJS during dev.
- Capability tests verify frontend calls through the WebView bridge and
  backend calls through the authenticated host/device bridge.
- Security tests reject invalid sessions, expired credentials, wrong origins,
  unauthorized capabilities, and unpinned LAN connections.
- Native changes rebuild and reinstall; JavaScript and supported configuration
  changes do not.
- Preview tests run the packaged QuickJS application and cover appd-specific
  runtime compatibility separately from host development.
- Platform tests cover macOS, iOS Simulator, physical iOS, Android emulator,
  physical Android where supported, and Windows WebView2.

## Acceptance criteria

- appd dev launches the developer's normal framework command without
  framework-specific appd request handlers.
- `appd dev <target>` accepts a managed target alias or concrete device
  identifier; the target determines the platform and native target pack.
- `appd devices` lists target IDs and types with only `available` or
  `blocked: reason` as the default status vocabulary.
- Selecting a managed virtual-target alias provisions and boots a missing or
  stopped compatible target without choosing randomly among the developer's
  other profiles.
- A client style or module edit updates the device WebView through the
  framework's normal HMR path without reinstalling the app.
- A Worker/SSR edit is handled by the host Cloudflare development runtime and
  reaches subsequent device requests without a device-side generation or
  ModuleRunner.
- Unsupported Wrangler bindings produce one prominent warning block with a
  specific list while development continues.
- Backend native plugin calls reach the device through a scoped authenticated
  binding/RPC channel.
- The WebView remains on the stable appd HTTPS origin and never connects
  directly to the host development server.
- QuickJS-only backend code fails in dev rather than being evaluated on the
  device.
- Distribution builds contain no Vite runtime, development endpoint,
  development credential, or downloaded-code path.
- appd preview remains available for validating the packaged QuickJS runtime.

## References

- Cloudflare Vite plugin:
  https://developers.cloudflare.com/workers/vite-plugin/
- Cloudflare local development:
  https://developers.cloudflare.com/workers/local-development/
- Cloudflare Vite plugin API:
  https://developers.cloudflare.com/workers/vite-plugin/reference/api/
- Cloudflare Vite programmatic configuration:
  https://developers.cloudflare.com/workers/vite-plugin/reference/programmatic-configuration/
- Cloudflare Vite environments:
  https://developers.cloudflare.com/workers/vite-plugin/reference/vite-environments/
- Cloudflare service bindings:
  https://developers.cloudflare.com/workers/runtime-apis/bindings/service-bindings/
- Astro Cloudflare adapter:
  https://docs.astro.build/en/guides/integrations-guide/cloudflare/
- Cloudflare Next.js guide:
  https://developers.cloudflare.com/workers/framework-guides/web-apps/nextjs/
- Vite plugin API:
  https://vite.dev/guide/api-plugin.html
- QuickJS-NG developer guide:
  https://quickjs-ng.github.io/quickjs/developer-guide/intro/
- rquickjs documentation:
  https://docs.rs/rquickjs/latest/rquickjs/
