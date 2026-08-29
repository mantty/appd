# appd

Cross-platform apps written using web native frameworks and semantics. Build apps that run on Android, iOS, MacOS, Windows, Linux, and the web (via Cloudflare workers).

>! This project is in a very early alpha state. Here be dragons.

## Commands

```sh
cargo fmt --all --check
xcrun swift-format lint --strict platforms/apple/source/*.swift plugins/*/apple/*.swift
pnpm --dir plugins lint:ts
pnpm --dir plugins test:ts
node --test appd/tests/quickjs_runtime/runtime.test.mjs
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p appd --all-targets --features test-stubs -- -D warnings
cargo clippy -p appd --lib --features test-stubs --target aarch64-apple-ios -- -D warnings
cargo clippy -p appd --lib --features test-stubs --target aarch64-apple-ios-sim -- -D warnings
cargo clippy -p appd --lib --features test-stubs --target x86_64-apple-ios -- -D warnings
cargo clippy -p appd --lib --features test-stubs --target x86_64-apple-darwin -- -D warnings
cargo clippy -p appd --lib --features test-stubs --target aarch64-linux-android -- -D warnings
cargo test -p appd --features native --test packaged_worker
cargo test --workspace
```

Build a target pack with:

```sh
pnpm --dir plugins install --frozen-lockfile
cargo run -p xtask -- target-pack --target macos-arm64
```

The following command builds the example and creates a native app:

```sh
cargo run -p xtask -- target-pack --target macos-arm64
appd build macos --project examples/astro \
  --target-pack target/appd-target-packs/macos-arm64 \
  --config examples/astro/dist/server/wrangler.json
```

Run a development app by passing the framework command after `--`. The
framework's local HTTP endpoint is supplied with `--server` when it does not
use the default Vite port:

```sh
appd dev macos --project examples/astro \
  --target-pack target/appd-target-packs/macos-arm64 \
  --config examples/astro/wrangler.json \
  --server http://localhost:4321 -- pnpm dev
```
