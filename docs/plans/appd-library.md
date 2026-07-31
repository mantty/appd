# appd as a library

Status: implemented for macOS, iOS, and Android. Windows and Linux land with
their platform support.

First of three: this, then `appd-api-testing.md`, then `appd-plugins.md`.

## Purpose

appd used to be the application: it created the application object, window,
WebView, and event loop. That inverted. A per-platform shell is the
application and drives appd through a public API.

appd is pre-release. Backwards compatibility is not a goal.

## Crates

| Crate | Owns |
|---|---|
| `appd-runtime` | the library: certificates, runtime, events |
| `appd-bare` | Bare integration |
| `appd-bundle` | the on-disk app contract and source preparation |
| `appd-shell-apple` | precompiled Apple runtime ABI and Swift shell |
| `appd-shell-android` | precompiled Android runtime ABI and Kotlin shell |
| `appd-cli` | compiling native shells and packaging apps |

`appd-bundle` is new. `wrangler_config` and `assets` move there out of
`appd-runtime`. It owns the app layout and resolves every path within it, so
neither `appd-cli` nor `appd-runtime` names a file. `appd-cli` writes the
contract, `appd-runtime` reads it, and both depend on `appd-bundle`.

`appd-bare` holds Bare-specific code only. Supporting a different backend
means writing one new crate beside it, with no appd logic to move.

Platform shell crates contain two distinct parts: a precompiled Rust runtime
library and the native shell sources that `appd build` compiles and links
against it. Components inside `appd-runtime` depend on each other through
narrow interfaces.

## Certificates

- Generates the CA, server certificate, and client certificate.
- Caches them in the app state directory.
- Renews them before expiry, on a background thread.
- Decides TLS auth challenges: which identity to present, and whether a server
  certificate is trusted. The shell wires those decisions into the platform
  callback.

## Runtime

- Starts and stops Bare through `appd-bare`, and owns the runtime handle.
- Builds Bare's config from paths `appd-bundle` resolves and the certificates.
- Loads app code, already prepared by `appd-cli`.
- Suspends and resumes when the shell reports an OS lifecycle change.
- Emits lifecycle events — starting, listening, suspended, resumed, failed —
  for shells and, later, plugins.
- Learns Bare's port when its listener binds. Startup does not poll.

## Shell

The shell owns the process entry point, application object, UI event loop,
window, WebView, and WebView proxy configuration. It starts, stops, suspends,
and resumes appd, and answers platform callbacks using the certificate
component.

macOS and iOS use the same Swift shell and C runtime ABI. Android uses a Kotlin
shell and JNI runtime ABI. All target packs have the same boundary:
precompiled Rust runtime library plus native shell sources. Developers need
the native platform toolchain to build and sign an app, but never a Rust
toolchain.

## Steps

1. Extract `appd-bundle`; repoint `appd-cli` and `appd-runtime` at it.
2. Define the library API: start, stop, suspend, resume, config, events,
   certificate challenges.
3. Replace the Bare startup handshake so the port arrives when the listener
   binds.
4. Move the entry point, application object, and event loop into the shared
   Swift shell for macOS and iOS.
5. Do the same for Android in Kotlin.
6. Do the same for Windows and Linux alongside their platform support.

## Done when

Every shell runs appd through the public API, the library creates no
application object or event loop, and every app build links a precompiled
runtime library without invoking Cargo.
