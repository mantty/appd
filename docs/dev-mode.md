# Dev Mode: Vite-based HMR for appd

Status: proposal (not implemented)
Scope: developer experience only. Nothing in this document affects production builds or the production runtime path.

## Goal

Eliminate the edit → build → install → review loop. A developer running `appd dev` should see:

- Frontend edits applied in the running webview in under a second, without reload where possible (HMR).
- Worker (backend) edits applied to the running Bare runtime in under a second, without reinstalling the app.
- Config/binding changes applied without an app reinstall.
- Native shell / addon changes as the only case requiring a rebuild + reinstall.

Assumption: we can build and install to the iOS simulator and all other targets. The simulator shares the host network, so `localhost` on the host is reachable from the app.

## Why Vite

Vite 6's Environment API separates the dev server (module graph, transforms, watch, HMR protocol) from the runtime that executes the code. A small ModuleRunner executes modules inside an arbitrary runtime, fetching transformed modules from the dev server on demand and receiving invalidation events over a pluggable transport. Cloudflare's `@cloudflare/vite-plugin` is the production reference for exactly this shape: the runner executes inside workerd. We do the same with the runner inside Bare.

This is the industry-default approach (Astro, SvelteKit, Nuxt, SolidStart, Remix, and Cloudflare Workers all build dev mode on Vite). There is no standard to build on instead: ESM is standardised, but module graphs, transform pipelines, and HMR protocols are not — `import.meta.hot` is a Vite convention. The alternative is owning a bespoke dev server and HMR protocol (the Next.js/Turbopack path), which is a multi-year investment orthogonal to appd's goals.

## Architecture

`appd dev` orchestrates three pieces:

### 1. One Vite dev server, two environments

- `client` environment: the frontend. Standard Vite behaviour.
- `worker` environment: the backend worker. Must be configured with the same module-resolution rules as our production pipeline: the `"bare"` export condition, builtin/native module externals, and `node:*` handling identical to what wrangler/bare-pack produce.

The dev server owns watching, transforms, the module graph, and pushes HMR/invalidation events.

### 2. One dev-mode app install

The simulator/device app is built once in dev configuration. Native shell, Bare, addons, and the mTLS loopback listener are identical to production. The single difference: instead of loading the bundled worker artifact from disk, the runtime bootstraps a Vite ModuleRunner that connects out to the dev server and imports the worker entry through it. Module evaluation in the runner is `AsyncFunction`-based, so it requires nothing from the engine beyond what we already have. This bootstrap is dev-only code and is excluded from release builds (also keeps us clear of App Store downloaded-code concerns).

### 3. The dev loop per edit type

| Edit | Mechanism | Cost |
| --- | --- | --- |
| Frontend source | Vite HMR over its WebSocket; webview patches modules in place | sub-second, usually no reload |
| Worker source | Vite invalidates worker environment graph → runner re-imports entry → `server.ts` swaps the `worker` module reference it dispatches to; in-flight requests drain on the old module | sub-second |
| `wrangler.jsonc` / bindings | restart worker environment only: rebuild `env`, re-import entry | seconds, no reinstall |
| Native shell / addons | full rebuild + reinstall | minutes; by design the rare path |

## Key decision: the webview stays on the appd origin

Two options exist for serving the frontend in dev:

1. Point the webview directly at the Vite dev-server origin.
2. Keep the webview on the appd loopback origin and proxy asset/HMR traffic from the worker's asset path through to Vite (Vite supports being reverse-proxied; configure `server.hmr.clientPort`/`path` for the HMR WebSocket).

We take option 2. Rationale: cookies, origin semantics, mTLS, and the WebSocket upgrade path are then exercised continuously during development instead of only at release. The frontend cannot accidentally depend on dev-origin behaviour. Cost: proxy plumbing in the asset service, written once, reusing the existing transport interface.

## Transport separation for the runner

The ModuleRunner needs its own channel to the dev server (module fetch + invalidation events). This channel must NOT route through the appd transport (loopback/mTLS/interception). It is a separate plain connection direct to the Vite server. Reason: the dev tool must keep working while we debug the transport it would otherwise depend on. Dev-only channel; plain HTTP/WS is acceptable.

## Physical device support

Same architecture with two adjustments:

- Runner and HMR connections target the host's LAN IP instead of `localhost` (surfaced by `appd dev` via QR/flag, as convenient).
- Dev TLS relaxation for the Vite legs (or plain HTTP — they are dev-only channels).

Wire this early. Simulator-only DX lets device-specific transport bugs accumulate unseen.

## Containing the Vite dependency

The anxiety to manage is coupling surface, not tool choice. Discipline:

1. **Production never touches Vite.** Release artifacts come from the wrangler/bare-pack pipeline. appd's correctness, compat layer, and transport are fully exercisable with no dev server in existence. A Vite breaking change can inconvenience development; it cannot block shipping.
2. **Small runtime-side contract behind a seam.** What lives in the appd runtime is a ModuleRunner bootstrap + transport shim (order of a few hundred lines). Define it as a "dev provider" interface; Vite is the first implementation. We do not expect to write a second one; the seam exists to keep the dependency honest and replaceable.
3. **Pin deliberately.** The Environment API is new (experimental in Vite 6) and the project moves fast (VoidZero; Rolldown replacing Rollup). Pin Vite per appd release; upgrade on our schedule with the differential suite green.
4. **`appd preview` is the drift alarm.** Preview mode runs the real production artifact (wrangler dry-run / bare-pack output) inside the same dev harness. CI runs differential tests against preview, not dev. This catches dev/prod module-resolution skew — the failure mode that actually burns users — regardless of what the dev tooling does.

## Known risks

- **Dev/prod resolution skew** (see preview mode above). Dev serves modules individually with Vite resolution; prod is a bundled artifact with unenv injection. These pipelines will drift if untested.
- **Environment API maturity.** Track Cloudflare's plugin as the canary; they hit the sharp edges first.
- **HMR semantics for worker state.** Re-importing the worker entry resets module-level state. This matches the Workers model (state belongs in bindings/storage), but document it so developers don't chase "lost" in-memory state in dev.

## Open questions

- Exact bootstrap handshake between `appd dev` and the dev-mode app (how the app learns the dev-server address: build-time constant vs. discovery).
- Whether the worker environment can share the Cloudflare plugin's resolution config directly rather than re-deriving it.
- Error overlay: surface worker-side errors into the webview overlay the way Vite does for client errors.
