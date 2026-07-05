# Workers Compatibility

Gaps between authoring for appd and authoring for deployed Cloudflare Workers. Goal: a stock Workers project runs under appd unchanged.

## Config and bindings

- [ ] `vars` -> plain text bindings on `env`
- [ ] Cache API service in the generated config (`caches.default`; astro's image endpoint already expects it)
- [ ] KV namespaces (local persistence)
- [ ] R2 buckets (local persistence)
- [ ] D1 databases (local persistence)
- [ ] Durable Objects
- [ ] Queues (producers + consumers)
- [ ] Secrets (`.dev.vars` or platform keychain)
- [ ] Cron triggers -> invoke `scheduled()`

## Modules and build

- [ ] `rules` from wrangler config (custom globs/types) instead of extension-only discovery
- [ ] Text/data/json module kinds (workerd supports them; scan only handles `.mjs`/`.wasm`)
- [ ] Sibling `.js` modules (currently only `.mjs` is discovered; main is exempt)
- [ ] CommonJS modules

## Assets

- [ ] Honor `.assetsignore` (exclude matches; never serve the file itself -- currently served)
- [ ] `_headers` / `_redirects`
- [ ] `run_worker_first`

## Tooling

- [ ] `appd dev`: live-reload dev loop against the native runtime (today: rebuild the app)
- [ ] Surface unsupported wrangler config keys as warnings instead of silently ignoring them

## Known divergences accepted for now

- appd consumes built artifacts, not source; no bundling. Projects point `--config` at their build output's resolved wrangler config.
- Local serving uses mTLS on an OS-assigned port; invisible to the WebView, visible to direct HTTP clients.
- `request.cf` carries defaults, not real geo data.
