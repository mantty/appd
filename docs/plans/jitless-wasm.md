# Jitless WebAssembly

iOS forbids JIT compilation, so iOS-family builds run V8 with `--jitless`. Jitless WebAssembly is supported via V8's own wasm interpreter, DrumBrake.

## How it's wired

1. **Build time**: `--@v8//:v8_enable_drumbrake=true` on every Bazel invocation (`BAZEL_COMMON_ARGS` in `workerd/scripts/appd_workerd.py`). This compiles the interpreter in and makes `--wasm-jitless` a settable flag; without it the flag is compiled read-only false. The interpreter is dormant unless the flag is passed, costs nothing measurable in the final binary, and keeps macOS and iOS Simulator builds identical Bazel configurations, so simulator SDKs repackage the shared compile.
2. **Runtime**: `v8Flags = ["--jitless", "--wasm-jitless"]`, emitted by `crates/appd-runtime/src/workerd_config.rs` whenever `jitless` is requested. Both flags are required: V8 only auto-implies `wasm_jitless` on tvOS, and under `--jitless` alone the `WebAssembly` global is absent, which fails an assert in workerd's `Worker::setupContext()` and kills the server.
3. **Wasm delivery**: `.wasm` files in the worker directory are embedded into `config.capnp` as `wasm = embed` modules (`scan_modules()` in `workerd_config.rs`), compiled once at worker load; imports receive a `WebAssembly.Module`. Bundlers externalize `.wasm` imports into sibling files whose relative paths resolve against the generated module names.

The iOS Simulator doesn't enforce the no-JIT constraint, but `jitless` stays on for all iOS-family builds so the Simulator previews real-device behavior.

## Constraints

- **`WebAssembly.compile()`/`new WebAssembly.Module(bytes)` fails at request time** with `CompileError: Wasm code generation disallowed by embedder` on every platform: workerd's embedder policy permits wasm code generation only at worker load. Ship wasm as a module import.
- **Never install a `WebAssembly` stub via `v8::Extension`.** V8 runs extensions before it installs WebAssembly itself (`InstallExtensions` precedes `InstallSpecialObjects` in `bootstrapper.cc`), and the real installation is an add-only property write -- a stub silently replaces the real API in every context, on every platform.
- `wasm_jitless` is marked experimental by V8 (`DEFINE_EXPERIMENTAL_FEATURE`).

## Dead ends (don't re-investigate)

- No workerd compatibility flag or Autogate key controls the jitless/WebAssembly behavior.
- Disabling WebAssembly at the V8 build level doesn't help: `setupContext()` asserts on the global's *presence*, so a wasm-less build fails the same assert.
- A guard patch to upstream `worker.c++` works but patches upstream source. DrumBrake supersedes it.
