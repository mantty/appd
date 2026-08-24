# appd as a library

Status: implemented for macOS, iOS, Android, and Windows. Linux remains a
future platform.

First of three: this, then `appd-api-testing.md`, then `appd-plugins.md`.

## Purpose

appd used to be the application: it created the application object, window,
WebView, and event loop. That inverted. A per-platform shell is the
application and drives appd through a public API.

appd is pre-release. Backwards compatibility is not a goal.

## Rust structure

| Directory | Owns |
|---|---|
| `appd/src/worker_package` | the on-disk Worker contract and source preparation |
| `appd/src/quickjs` | QuickJS embedding and the gateway |
| `appd/src/runtime` | certificates, lifecycle, and runtime startup |
| `appd/src/vfs` | the Workers virtual filesystem |
| `appd/src/node_fs` | native `node:fs` bindings |
| `appd/src/platform` | target-specific runtime bridges |
| `cli` | user-facing native app packaging |
| `platforms/apple/source` | Swift shell, C ABI header, and module map |
| `platforms/apple/build` | Apple target-pack recipe and app build entrypoint |
| `platforms/android/source` | Kotlin shell sources |
| `platforms/android/build` | Android target-pack recipe and app build entrypoint |
| `platforms/windows/source` | Rust Windows application shell |
| `platforms/windows/build` | Windows target-pack recipe and app build entrypoint |
| `tools/xtask` | maintainer-only target-pack generation |
| `target_pack_format` | target-pack manifest format and target metadata |

`appd/src/worker_package` owns the Worker layout and resolves every path within it,
so neither `cli` nor the runtime names a file. `cli` writes the contract and
the runtime reads it.

The `appd` package contains the shared runtime and its Apple C and Android JNI
bridges. Every platform shell remains under `platforms/`, regardless of its
implementation language. The CLI stages app-specific inputs and invokes the
uniform target-pack entrypoint; it does not own platform project generation or
signing. Components inside `appd` depend on each other through narrow
interfaces.

## Certificates

- Generates the CA, server certificate, and client certificate.
- Caches them in the app state directory.
- Renews them before expiry, on a background thread.
- Decides TLS auth challenges: which identity to present, and whether a server
  certificate is trusted. The shell wires those decisions into the platform
  callback.

## Runtime

- Starts and stops QuickJS, and owns the runtime handle.
- Builds the runtime config from paths resolved by `appd/src/worker_package` and the
  certificates.
- Loads app code, already prepared by `cli`.
- Suspends and resumes when the shell reports an OS lifecycle change.
- Emits lifecycle events — starting, listening, suspended, resumed, failed —
  for shells and, later, plugins.
- Learns the gateway's port when its listener binds. Startup does not poll.

## Shell

The shell owns the process entry point, application object, UI event loop,
window, WebView, and WebView proxy configuration. It starts, stops, suspends,
and resumes appd, and answers platform callbacks using the certificate
component.

macOS and iOS use the same Swift shell and C runtime ABI. Android uses a Kotlin
shell and JNI runtime ABI. Windows uses the Rust shell under
`platforms/windows`. All target packs have the same boundary: a compiled appd
artifact, native shell sources where the platform compiles the shell during app
assembly, or a precompiled shell executable, and `build/entrypoint`.
Developers need the native platform toolchain to build and sign an app; the
Windows target pack additionally contains the precompiled Rust shell.

## Done when

Every shell runs appd through the public API, the library creates no
application object or event loop, and every app build links a precompiled
runtime library without invoking Cargo.
