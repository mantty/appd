# appd Astro Example

Astro SSR app using the Cloudflare adapter. It exercises server rendering, one
prerendered route, asset serving, navigation, and a WebSocket endpoint.

From this directory, build the web app with:

```sh
pnpm install --frozen-lockfile
pnpm run build
```

Build a target pack from the appd workspace, then package it:

```sh
cargo run -p xtask -- target-pack --target macos-arm64
target_pack_dir=../../target/appd-target-packs \
  cargo run -p cli -- build macos --project . --config dist/server/wrangler.json
```

The example intentionally uses no WebAssembly. It covers server rendering,
static assets, API routes, navigation, and WebSockets through the QuickJS
runtime.
