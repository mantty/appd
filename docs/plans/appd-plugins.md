# appd plugins

Status: proposal

Third of three, after `appd-library.md` and `appd-api-testing.md`.

## Purpose

Give app code on-device capabilities — location, Bluetooth, NFC, camera —
through plugins that also work on web deployments.

## Model

A plugin is frontend or backend, never both.

- Frontend plugins run in the page: the browser on web, the WebView on native.
- Backend plugins run in the worker: Cloudflare on web, Bare on native.

appd does not move data between tiers. An app that collects something on one
side and needs it on the other sends a request.

## Writing one

One TypeScript entrypoint defines the plugin.

```ts
export default class Geolocation extends BackendPlugin<Impl> {
  static id = "geolocation";
  current() { return this.call("current"); }
}
```

- The base class, `FrontendPlugin` or `BackendPlugin`, sets the kind.
- `id` is what app code imports.
- The class's public methods are the API, typed for consumers.
- `Impl` names what each platform implements. Its values are primitives, plain
  objects, arrays, and buffers.
- `call` and `subscribe` reach the platform implementation.

Validation, caching, and fallbacks go in the entrypoint, written once.

## Platform implementations

```text
<package>/
  index.ts
  web/ macos/ ios/ windows/ android/ linux/
```

A platform is supported if its directory exists. An unrecognised directory is
a build error. A missing platform still builds and type-checks; its calls
throw `PluginNotImplemented`.

`web/` is an ES module. A native directory holds a build manifest naming a
build command, an artifact, and a registration entry point. appd runs the
command and links the result, and never reads the sources, so any language
works.

## Resolution

`appd:plugin/<id>` resolves in whichever build produces the bundle.

| Bundle | Built by |
|---|---|
| frontend, any target | the app's Vite build |
| worker, Cloudflare | the app's Vite build |
| worker, native | appd's esbuild step |

Frontend bundles get frontend plugins, worker bundles get backend plugins, and
the wrong pairing fails the build.

## Calls

| Target | Route |
|---|---|
| web | direct call into `web/` |
| native backend | the bare bridge, over the C ABI |
| native frontend | WebView IPC, origin-gated |

## Phases

A. The model and the web path: `@appd/plugin`, plugin declaration in appd
   config, resolution in both builds, web implementations of both kinds.
B. Native code the worker can call: the bridge, native build orchestration,
   and registration packages in Swift and Kotlin. macOS and iOS, then Android.
C. Native code the page can call: WebView IPC in each shell.
D. Capabilities that act with no request in flight, such as geofence wakes:
   worker dispatch without a socket, on the library's lifecycle events.

## Deferred

- Generating Info.plist entries, Android manifest permissions, and
  entitlements from plugin metadata.
- How a plugin declares the API version it needs.
- Service workers on web, needed for push and background sync.
