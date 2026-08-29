# appd

Native applications powered by an appd-owned QuickJS-NG runtime. The runtime
supports Apple Silicon and Intel macOS, physical arm64 iOS devices, arm64 and
x64 iOS Simulator builds, arm64 Android, and x64 Windows. Every target uses the
same JavaScript runtime and compatibility layer.

## Structure

- `appd/` owns the runtime library. Its modules are organised under
  the root-level application, mTLS, packaging, and platform modules, plus the
  focused compatibility and QuickJS modules.
- `appd/src/quickjs.rs`, `appd/src/dispatcher.rs`, `appd/src/gateway.rs`, and
  `appd/src/transport.rs` own the QuickJS-NG engine integration, request
  dispatch, and gateway transport.
- `appd/src/server.rs` owns application startup, lifecycle, and packaged
  Worker loading.
- `appd/src/certificates.rs`, `appd/src/cert_generation.rs`, and
  `appd/src/cert_validation.rs` own local mTLS
  certificate material and trust decisions.
- `appd/src/packaging.rs`, `appd/src/env_vars.rs`,
  `appd/src/asset_manifest.rs`,
  and `appd/src/wrangler_config.rs` own the packaged Worker contract.
- `appd/src/fs/` owns the virtual filesystem and native Node filesystem
  bindings.
- `appd/src/streams/`, `appd/src/network/`, `appd/src/events/`, and
  `appd/src/globals/` own capability implementations shared by Web, Node, and
  Worker compatibility surfaces.
- `appd/src/builtins/` owns synthetic builtin entrypoints and registration.
- `appd-cli/` builds web projects and packages native application
  bundles.
- `plugins/` is an independent pnpm workspace containing the frontend plugin
  bridge and local plugin packages. Plugin Swift and Kotlin sources compile
  with the application shell.
- Each directory under `examples/` is an independent app package and links
  local plugins by their published package names.
- `tools/xtask/` contains maintainer-only target-pack generation. Target packs
  are CI/build artifacts, not a user-facing CLI operation.
- `appd/src/android_jni.rs` and `appd/src/apple_ffi.rs` own the target-specific
  runtime bridges.
- `platforms/apple/source/` contains the Swift application shell and Apple ABI
  headers. `platforms/apple/build/entrypoint` owns the Apple bundle build.
- `platforms/android/source/` contains the Kotlin application shell compiled
  by `platforms/android/build/entrypoint`.
- `platforms/windows/source/` contains the Rust application shell, and
  `platforms/windows/build/entrypoint.ps1` owns Windows app assembly around
  the precompiled shell.
The native shell is the application: it owns the process entry point, window,
`WebView`, and operating-system lifecycle, and drives the appd runtime through
a narrow native API. The appd package owns the shared runtime and its
target-specific runtime bridges; each platform directory owns its shell and
build entrypoint. Target packs contain the compiled appd artifact, native shell
sources where the platform compiles them during app assembly, or a precompiled
shell executable for Windows, plus a `build/entrypoint` (or
`build/entrypoint.ps1` for Windows) for one target. `appd build`
prepares the Worker package and native plugin inputs, then invokes that
entrypoint as `build INPUT_DIRECTORY OUTPUT_PATH`. The CLI does not generate
platform projects; each entrypoint owns its platform toolchain, bundle layout,
signing, and project files. The QuickJS runtime owns HTTPS gateway handling,
HTTP routing, Worker execution, static assets, and request-scoped storage.
Client-certificate verification remains required for the loopback gateway.

The entrypoint input directory contains the prepared app under `app/`, the
selected runtime and native shell artifacts under `runtime/` and
`native-shell/`, one-value build metadata under `metadata/`, and normalized
native plugin data under `plugins/`.

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

On macOS, `appd dev` automatically finds valid signing identities and
provisioning profiles for a physical iOS device. If more than one match is
available, it asks which identity/profile to use and remembers the choice for
that project and device. Set `APPD_IOS_SIGNING_IDENTITY` and
`APPD_IOS_PROVISIONING_PROFILE` together to override the selection (for
example, in CI).

Windows target-pack builds require the Visual Studio C++ toolchain, CMake 4,
NASM, Perl, Node.js, and Rust. The Windows CI and weekly release jobs configure
MSVC and verify those prerequisites before building.

WebAssembly is deliberately unsupported so every target has the same initial
feature set.
