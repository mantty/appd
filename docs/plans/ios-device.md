# iOS Device

`aarch64-apple-ios` builds V8 via its own native GN/Ninja flow instead of Bazel, then imports the result into workerd's Bazel build through the `@workerd-v8` seam Cloudflare already provides for this purpose. Bazel never attempts to compile V8 for this target.

## Why GN instead of porting Bazel

V8's Bazel port has no iOS support (no `is_ios` config_setting, dependency selects like zlib's `ARMV8_OS_*` have no iOS branch) and forces its generator tools (torque, mksnapshot) into target configuration for a build-speed optimization that assumes exec == target -- under a real device platform this would mean those tools are compiled as iOS binaries, which no build host can execute (macOS's AMFI refuses to run platform-2 Mach-O outside a provisioned device or the Simulator). V8's native GN build has none of this: it has real, actively-maintained `is_ios`/`target_os == "ios"` support (used to ship Chromium's own iOS browser-engine work) and a dedicated `v8_snapshot_toolchain` mechanism (`gni/snapshot_toolchain.gni`) that correctly builds host-executable tools while cross-compiling for a different target.

## Building V8

Fetch via `depot_tools`/`gclient` with `target_os = ['ios']`, check out the exact tag `build/deps/v8.MODULE.bazel` pins, apply all of Cloudflare's `patches/v8/*.patch` wholesale via `git apply -p1` (Bazel-specific ones, e.g. the tool-cfg patch, touch files GN never reads -- harmless no-ops). Build `v8_monolith` with GN/Ninja using:

```
target_os = "ios"
target_cpu = "arm64"
target_environment = "device"
ios_enable_code_signing = false
is_component_build = false
is_debug = false
use_custom_libcxx = false
v8_monolithic = true
v8_monolithic_for_shared_library = true
v8_expose_public_symbols = true
v8_use_external_startup_data = false
v8_enable_pointer_compression = false
cppgc_enable_caged_heap = false
v8_enable_sandbox = false
v8_enable_i18n_support = true
icu_use_data_file = false
v8_enable_webassembly = true
v8_enable_lite_mode = false
use_rtti = true
rust_sysroot_absolute = "<path to the rustup toolchain matching appd's rust-toolchain.toml>"
rustc_version = "<output of `rustc -V`>"
rust_force_head_revision = true
removed_rust_stdlib_libs = ["adler"]
added_rust_stdlib_libs = ["adler2"]
```

Non-obvious ones:

- `use_rtti = true` -- V8's GN build defaults to `-fno-rtti`; workerd's `jsg::Serializer`/`Deserializer` need typeinfo for `v8::ValueSerializer::Delegate`/`ValueDeserializer::Delegate`.
- `rust_sysroot_absolute`/`rustc_version` -- without this, GN builds V8's Rust crates (Temporal, ICU4X) with Chromium's own bundled custom rustc, which disagrees with appd-runtime's rustup toolchain on the unstable global-allocator shim ABI (`__rust_alloc` etc.), failing the final cargo link. Pointing GN at the same toolchain appd-runtime uses fixes it at the root; `rust.gni`'s own comment names V8 as likely compatible with a stable toolchain. `rust_force_head_revision` bypasses a version-consistency assertion that only expects Chromium's own pinned revision; the `adler`/`adler2` swap accounts for the crate rename between rustc versions.
- `RUSTC_BOOTSTRAP=1` must be set on the build invocation -- V8's Rust build config unconditionally passes internal `-Z` diagnostic/reproducibility flags to every rustc call, assuming a nightly compiler.
- Even with the toolchain aligned, `mksnapshot` failed to link on `__rust_no_alloc_shim_is_unstable_v2`: the hand-written `#[linkage = "weak"]` marker symbols in `build/rust/allocator/lib.rs` compile with local, not external, linkage under rustc 1.96 -- a real, currently-unreliable rustc mechanism (the upstream fix, rust-lang/rust#134522, was closed days before this was built). Fixed with a GN `action` (`build/rust/allocator/fix_shim_linkage.py`) that runs `llvm-objcopy --globalize-symbol` on the already-compiled object to promote the existing symbols to external linkage -- a symbol-table edit, not new code.
- `cppgc_enable_caged_heap = false` -- with it on (the default whenever pointer compression is off), `cppgc::InitializeProcess()` reserves a 4GB virtual address range per cage, retried up to 4 times on failure; real-device VM limits make every attempt fail, and V8 aborts with an OOM fatal error before a single request is served. V8's own iOS guide has always paired `v8_enable_pointer_compression = false` with this flag for the same reason.
- `icu_use_data_file = false` -- defaults to `true` on this GN-built target (unlike every other appd platform's Bazel-built V8, which statically links ICU by default). With it on, `V8System::init()`'s unconditional `InitializeICUDefaultLocation(nullptr)` crashes on a null-pointer `strlen` inside `RelativePath`.
- `v8_enable_webassembly = true`, `v8_enable_lite_mode = false` -- GN defaults iOS device builds to lite mode, which asserts wasm cannot be enabled without DrumBrake (`gni/v8.gni`). DrumBrake needs pointer compression on, which reintroduces the caged-heap OOM above, so device gets a full (non-lite) build with wasm compiled in but no execution backend. See WebAssembly below.

## WebAssembly

Wasm compiles into V8 here, but nothing can run it: DrumBrake (V8's jitless wasm interpreter) requires pointer compression, which this target can't afford, and there is no JIT under `--jitless`. Two consequences, both handled the same way Cloudflare's own embedder policy already handles refused wasm compilation -- a real, catchable `WebAssembly.CompileError` rather than a crash or a `ReferenceError`:

- V8 does not install the `WebAssembly` global at all under `--jitless --wasm-jitless` without DrumBrake. `appd_workerd.cpp` registers a V8 extension that installs a JS-level stub in its place, gated at compile time to the real device target only (`TARGET_OS_IOS && !TARGET_OS_SIMULATOR`) so it can never race V8's own install on a platform where the real global exists -- application code that references `WebAssembly` directly, including framework internals like Astro's own `WebAssembly.compile()` call bundled unconditionally into every SSR worker, gets a graceful rejection instead of failing to start.
- workerd's own `wasm = embed` module mechanism compiles embedded `.wasm` files eagerly at worker load, independent of the JS-level global entirely; V8's wasm code allocator has no valid path under this configuration and hits a hard `V8_Fatal`. `ConfigOptions::wasm_available` (`workerd_config.rs`) skips embedding `.wasm` files in `config.capnp` on this target for exactly this reason, which means any app-level wasm feature (e.g. the astro example's word-picker) is unavailable on real iOS devices; Simulator and macOS are unaffected and still ship it.

## Wiring into workerd's Bazel build

`workerd/overlay/build/workerd-v8/{BUILD,MODULE.bazel}` replaces the stock ICU-only passthrough at `@workerd-v8//:v8` (a pure overlay file addition, not a patch). A `config_setting` on `@platforms//os:ios` + `@apple_support//constraints:device` (disambiguating device from Simulator, which also matches `os:ios`) selects between a `cc_import` of the GN-built `libv8_monolith.a` (device) and the existing passthrough (every other platform, untouched). The `cc_import` also gathers V8's Rust crates (`.rlib`s renamed to `.a`, since `cc_import`/`cc_library` reject the `.rlib` extension) since they aren't in the monolith archive.

Two new appd-owned patches (applied the same way as Cloudflare's own, via a new `apply_patches()` in `appd_workerd.py`): add `aarch64-apple-ios` to `RUST_TARGET_TRIPLES` in workerd's own `rust.MODULE.bazel` (its internal Rust crates -- lolhtml, workerd-cxx -- still need a real toolchain for this genuine device platform, unlike the Simulator's retagged-macOS-build shortcut), and an `os:ios` branch for zlib's ARM-CRC32 select. Two more skip workerd's own `gen-compile-cache` tool (same target-cfg exec problem as V8's generators, narrower blast radius) by shipping empty compile-cache files -- V8 treats an absent cache as "compile normally," costing only a startup-time optimization.

Device and Simulator both force `--jitless`/`--wasm-jitless` at runtime (see `jitless-wasm.md`) and an explicit deployment-target linker flag (`-Wl,-platform_version,...`, in `crates/appd-runtime/build.rs`) matching the `MinimumOSVersion` app bundles declare -- rustc's own per-target defaults (10.0 device, 14.0 Simulator) don't match it otherwise.

## Verified

Real SDK build (`aarch64-apple-ios`, `otool` confirms arm64/platform 2/minos matching the app bundle), `appd-runtime` links and exports the expected symbols, the astro example builds a correctly-tagged `.app` with `v8Flags = ["--jitless", "--wasm-jitless"]` and no wasm module embedded. Installed and launched on a real, physical iPhone (ad-hoc codesigned, Personal Team provisioning) via `devicectl`: the app loads its page for real over the on-device WKWebView, no crash, no startup error. macOS and iOS Simulator confirmed unaffected -- Simulator re-verified by installing and running the built app in a booted device (real page load, real DrumBrake-executed wasm result). Code-signing and on-device installation require Developer Mode enabled on the physical device -- an Apple-enforced, one-time physical setting, not something any build tooling can set remotely.
