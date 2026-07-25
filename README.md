# appd

Native applications powered by [Bare](https://github.com/holepunchto/bare).
The initial runtime supports Apple Silicon and Intel macOS, physical arm64 iOS
devices, arm64 and x64 iOS Simulator builds, and arm64 Android. Every target
uses the same JavaScript runtime and compatibility layer.

## Structure

- `bare/` pins and builds BareKit, links the required Bare addons, and packages
  a native SDK.
- `crates/appd-bare/` exposes the appd-owned C ABI as a safe Rust runtime.
- `runtime/` owns the JavaScript server, Cloudflare/Node compatibility, assets,
  and WebSockets.
- `crates/appd-runtime/` owns certificates, lifecycle, and native platform
  shells.
- `crates/appd-cli/` builds web projects and packages native application
  bundles.
- `crates/appd-target-pack/` defines the versioned CLI/runtime artifact
  contract.

`vendor/` contains the small appd-owned seams around Bare ecosystem packages.
`bare-tls` provides the gateway TLS implementation and covers its
client-certificate capability with addon tests; `bare-ws` preserves
text-versus-binary message types for the Workerd-compatible WebSocket API; and
`cmake-toolchains` carries the iOS 17 deployment target. These are file
dependencies pinned in `pnpm-lock.yaml`, not modifications to an installed
dependency tree.

The native shell is intentionally narrow. Bare owns HTTPS gateway handling,
HTTP routing, worker execution, static assets, and WebSockets. Rust owns OS
lifecycle and WebView integration. Client-certificate verification remains
required for the current loopback gateway.

## Commands

```sh
cargo fmt --all --check
pnpm lint:ts
pnpm test:ts
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p appd-runtime --features bare-test-stubs -- -D warnings
cargo clippy -p appd-runtime --target aarch64-apple-ios --features bare-test-stubs -- -D warnings
cargo clippy -p appd-runtime --target aarch64-apple-ios-sim --features bare-test-stubs -- -D warnings
cargo clippy -p appd-runtime --target x86_64-apple-ios --features bare-test-stubs -- -D warnings
cargo clippy -p appd-runtime --target x86_64-apple-darwin --features bare-test-stubs -- -D warnings
cargo clippy -p appd-runtime --target aarch64-linux-android --features bare-test-stubs -- -D warnings
cargo test --workspace
python3 -m unittest discover -s bare/tests
```

Build a local Bare SDK and the JavaScript runtime with:

```sh
pnpm install
pnpm build:runtime
python3 bare/scripts/build-sdk.py --target macos-arm64
```

The following command builds the example and creates a native app:

```sh
appd pack build --target macos-arm64
appd build macos --project examples/astro \
  --target-pack target/appd-target-packs/macos-arm64/target-pack.json \
  --config examples/astro/dist/server/wrangler.json
```

Physical iOS signing uses `APPD_IOS_SIGNING_IDENTITY` and
`APPD_IOS_PROVISIONING_PROFILE`.

WebAssembly is deliberately unsupported so every target has the same initial
feature set.
