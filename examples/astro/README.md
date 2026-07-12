# appd Astro Example

Astro SSR app using the Cloudflare adapter. It exercises server rendering, one
prerendered route, asset serving, navigation, and a WebSocket endpoint.

From this directory, build the web app with:

```sh
pnpm install --frozen-lockfile
pnpm run build
```

Package it from the appd workspace:

```sh
cargo run -p appd-cli -- build macos --project . --config dist/server/wrangler.json
```

The example intentionally uses no WebAssembly. It covers server rendering,
static assets, API routes, navigation, and WebSockets through the Bare runtime.
