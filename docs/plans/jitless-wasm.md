# Jitless WebAssembly

iOS forbids JIT compilation, so iOS-family builds run V8 with `--jitless`. WebAssembly still works there: V8 ships a wasm interpreter (DrumBrake, built for exactly this scenario -- its own source comment cites tvOS, where JIT is unavailable), and appd enables it for those targets. No patches, no shims, no custom workerd code.

## How it's wired

1. **Build time**: `--@v8//:v8_enable_drumbrake=true` on the Bazel invocation for iOS-family targets only (`DEFAULT_BAZEL_ARGS_BY_TARGET` in `workerd/scripts/appd_workerd.py`). This compiles the interpreter in and makes `--wasm-jitless` a real, settable flag; without it the flag is compiled as read-only false. macOS/Linux/Windows builds don't carry it -- they have JIT.
2. **Runtime**: `v8Flags = ["--jitless", "--wasm-jitless"]`, emitted by `crates/appd-runtime/src/workerd_config.rs` whenever `jitless` is requested. Both flags are load-bearing: V8 only auto-implies `wasm_jitless` from `jitless` on tvOS, and under `--jitless` alone V8 omits the `WebAssembly` global entirely, which kills the whole workerd server -- upstream's `Worker::setupContext()` asserts the global exists for every context, before any user code runs.
3. **Wasm delivery**: `.wasm` files in the worker directory are embedded into `config.capnp` as `wasm = embed` modules (`scan_modules()` in `workerd_config.rs`). workerd compiles them once at worker load -- the only point its embedder policy permits wasm code generation, matching production Cloudflare Workers -- and imports receive a ready `WebAssembly.Module`. This is the same contract wrangler deploys with: bundlers (e.g. `@cloudflare/vite-plugin`) externalize `.wasm` imports into sibling files with relative import paths, which resolve against the module names appd generates.

The iOS Simulator doesn't technically enforce the no-JIT constraint (it runs on the macOS kernel), but `jitless` stays on for all iOS-family builds so the Simulator remains a faithful preview of real-device behavior.

## Constraints worth knowing

- **`WebAssembly.compile()`/`new WebAssembly.Module(bytes)` from arbitrary bytes fails at request time** with `CompileError: Wasm code generation disallowed by embedder`, on every platform, JIT or not. This is workerd's own embedder policy, identical to production Workers. Ship wasm as a module import, never as runtime-compiled bytes.
- **Never install a `WebAssembly` stub via `v8::Extension`.** V8 runs extensions before it installs WebAssembly itself (`InstallExtensions` precedes `InstallSpecialObjects` in `bootstrapper.cc`), and the real installation is an add-only property write -- a stub always wins the race and permanently replaces the real API in every context, on every platform, including ones with full JIT. appd previously carried exactly such a stub and it silently broke all WebAssembly everywhere; it was removed once DrumBrake made the real API available under jitless.
- `wasm_jitless` is marked experimental by V8 (`DEFINE_EXPERIMENTAL_FEATURE`).

## Verified (2026-07-05)

- macOS (JIT path): example app's `.wasm` module import compiles at load and executes per request.
- iOS Simulator (DrumBrake path, `--jitless --wasm-jitless`, retagged build, real booted device): same worker, same module, real interpreted execution with correct results.
- Both-flag requirement: with the interpreter compiled in but only `--jitless` passed, the server dies at context setup on the missing-`WebAssembly` assert.

## Dead ends (don't re-investigate)

- No workerd compatibility flag or Autogate key controls the jitless/WebAssembly behavior.
- Disabling WebAssembly at the V8 build level doesn't help: `setupContext()` asserts on the global's *presence*, so a wasm-less build fails the same way plain jitless does.
- A guard patch to upstream `worker.c++` works but patches upstream source; the stub-extension approach breaks real wasm (above). DrumBrake supersedes both.
