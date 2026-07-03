# appd Astro Example

Astro SSR app using the Cloudflare adapter. It exercises server rendering, one
prerendered route, asset serving, navigation, and a WebSocket endpoint.

From this directory, build the web app with:

```sh
pnpm install --frozen-lockfile
pnpm run build
```

Package it with appd when you have a target pack:

```sh
appd build --platforms=macos
```

Current spike status: release target packs are not available yet, so the
installed CLI cannot package this example without workspace-local runtime
artifacts.
