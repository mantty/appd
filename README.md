# appd

Native applications powered by an appd-owned QuickJS-NG runtime. The runtime
supports Apple Silicon and Intel macOS, physical arm64 iOS devices, arm64 and
x64 iOS Simulator builds, arm64 Android, and x64 Windows. Every target uses the
same JavaScript runtime and compatibility layer.

## Structure

- `crates/appd-quickjs/` owns QuickJS-NG embedding, the Rust gateway, request
  runtimes, and native lifecycle.
- `runtime/qjs/` owns the engine-neutral Web, Node, and Workers compatibility
  modules bundled into each Worker.
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

The native shell is the application: it owns the process entry point, window,
`WebView`, and operating-system lifecycle, and drives a precompiled appd
runtime library through a narrow native API. Target packs contain that library
and the shell sources for one target. `appd build` compiles the shell and links
the library; it never compiles Rust. The QuickJS runtime owns HTTPS gateway
handling, HTTP routing, Worker execution, static assets, and request-scoped
storage. Client-certificate verification remains required for the loopback
gateway.

## Commands

```sh
cargo fmt --all --check
xcrun swift-format lint --strict crates/appd-shell-apple/native/*.swift plugins/*/apple/*.swift
pnpm lint:ts
pnpm test:ts
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p appd-runtime --all-targets --features test-stubs -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target aarch64-apple-ios -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target aarch64-apple-ios-sim -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target x86_64-apple-ios -- -D warnings
cargo clippy -p appd-shell-apple --all-targets --features test-stubs --target x86_64-apple-darwin -- -D warnings
cargo clippy -p appd-shell-android --all-targets --features test-stubs --target aarch64-linux-android -- -D warnings
cargo test --workspace
```

Build a target pack with:

```sh
pnpm install
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
