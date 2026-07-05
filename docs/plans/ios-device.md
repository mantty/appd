# iOS Device

`aarch64-apple-ios` (real devices) has no workerd SDK target. It cannot be built without upstream patches; every blocker below fails Bazel analysis before a single compile, and none is reachable from command-line flags or overlay files.

## Blockers

1. **V8's generator tools build in the target configuration.** workerd's `patches/v8/0005-Speed-up-V8-bazel-build-by-always-using-target-cfg.patch` hardcodes `cfg = "target"` for torque/mksnapshot/builtins generators as a load-time `.bzl` value -- invisible to flags, selects, and transitions. Under an iOS platform those tools become iOS-device executables, which no build host can run: macOS SIGKILLs platform-2 binaries at exec (AMFI policy, ad-hoc signing included; verified empirically). Fixing this means changing the patch to `"exec"` (or a flag-driven transition) and paying the double-compile of the tools' dep graph that the patch exists to avoid.
2. **V8's Bazel port has no iOS support.** `bazel/config` defines no `is_ios`; source selects cover linux/android/macos/windows only and fail analysis under `os:ios`. GN supports iOS; the Bazel port doesn't -- this is new porting work, not configuration.
3. **No Rust toolchain for the triple.** `build/deps/rust.MODULE.bazel` registers darwin/linux/windows triples only, and the SDK link graph includes ~500 rlibs. Toolchain registration is MODULE-level upstream; `--extra_toolchains` can't add unregistered rules_rust toolchains.
4. **Dependency BUILD files lack iOS branches.** zlib's `ARMV8_OS_*` select has no `os:ios` arm (verified); perfetto/sqlite3/tcmalloc/workerd sources are untested behind it.

## Verified non-blockers

- apple_support's cc toolchain resolves cleanly for `ios_arm64` (`cc-compiler-ios_arm64`, host-executed).
- `--platforms=@@apple_support+//platforms:ios_arm64` works from the command line (canonical repo name required; `apple_support` is a transitive dep).
- capnp codegen is exec-config.

## Path if prioritized

In dependency order: (1) patch V8's tool cfg to exec, (2) port iOS to V8's Bazel build (`is_ios`, posix/darwin source mapping, `V8_TARGET_OS_IOS`), (3) add `aarch64-apple-ios` to `RUST_TARGET_TRIPLES`, (4) iterate per-dep iOS select branches. Steps 1-2 are the substance; both are upstream patches, so this is a porting project requiring a maintained patch mechanism -- a deliberate policy change from the no-patches rule.
