# appd

Native applications powered by [Bare](https://github.com/holepunchto/bare).
The initial runtime supports Apple Silicon and Intel macOS, physical arm64 iOS
devices, arm64 and x64 iOS Simulator builds, arm64 Android, and x64 Windows.
Every target uses the same JavaScript runtime and compatibility layer.

## Structure

- `bare/` pins upstream BareKit prebuilds, compiles appd's TLS add-on, and
  links the runtime add-ons for each target.
- `crates/appd-bare/` owns BareKit worklets through its native C API.
- `bare/runtime/` owns the JavaScript server, Cloudflare/Node compatibility,
  assets, and WebSockets.
- `crates/appd-runtime/` is the runtime library: certificates, lifecycle, and
  events. A shell drives it; it owns no application object or event loop.
- `crates/appd-shell-apple/` provides the precompiled Apple runtime library and
  the Swift application shell compiled by `appd build`.
- `crates/appd-shell-android/` provides the precompiled Android runtime library
  and the Kotlin application shell compiled by `appd build`.
- `crates/appd-shell-windows/` provides the precompiled Windows runtime
  executable.
- `crates/appd-bundle/` owns the on-disk app contract: layout, Wrangler
  config, and the asset manifest. The CLI writes it and the runtime reads it.
- `crates/appd-cli/` builds web projects and packages native application
  bundles.
- `crates/appd-target-pack/` defines the versioned CLI/runtime artifact
  contract.
- `plugins/` contains the frontend plugin bridge and plugin packages. Plugin
  Swift and Kotlin sources compile with the application shell.

`bare/vendor/` contains the small appd-owned seams around Bare ecosystem packages.
`bare-tls` provides the gateway TLS implementation and covers its
client-certificate capability with addon tests; `bare-ws` preserves
text-versus-binary message types for the Workerd-compatible WebSocket API; and
`cmake-toolchains` carries the iOS 17 deployment target. These are file
dependencies pinned in `pnpm-lock.yaml`, not modifications to an installed
dependency tree.

The native shell is the application: it owns the process entry point, window,
`WebView`, and operating-system lifecycle, and drives a precompiled appd
runtime library through a narrow native API. Target packs contain that library
and the shell sources for one target. `appd build` compiles the shell and links
the library; it never compiles Rust. Bare owns HTTPS gateway handling, HTTP
routing, worker execution, static assets, and WebSockets. Client-certificate
verification remains required for the loopback gateway.

Bare reports its port over BareKit's IPC channel once the listener binds, so
startup never polls.

## Commands

```sh
cargo fmt --all --check
xcrun swift-format lint --strict crates/appd-shell-apple/native/*.swift plugins/*/apple/*.swift
pnpm lint:ts
pnpm test:ts
pnpm test:bare
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p appd-runtime --all-targets --features test-stubs -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target aarch64-apple-ios -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target aarch64-apple-ios-sim -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target x86_64-apple-ios -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target x86_64-apple-darwin -- -D warnings
cargo clippy -p appd-shell-android --all-targets --features test-stubs --target aarch64-linux-android -- -D warnings
cargo test --workspace
python3 -m unittest discover -s bare/tests
```

Build a local Bare SDK and the JavaScript runtime with:

```sh
pnpm install
pnpm build:runtime
appd pack build --target macos-arm64
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

Windows target-pack builds require the Visual Studio C++ toolchain, CMake 4,
NASM, Perl, Node.js, and Rust. The Windows CI and weekly release jobs configure
MSVC and verify those prerequisites before building.

WebAssembly is deliberately unsupported so every target has the same initial
feature set.
