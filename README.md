# appd

Unified Rust workspace for the appd spike.

The workspace keeps product ownership together while still producing separate
build artifacts:

- `appd-cli`: host-side developer tooling
- `appd-runtime`: target-native runtime contracts and listener handoff
- `appd-target-pack`: shared metadata contract between CLI and runtime packs

`appd.workerd` remains separate unless the workerd fork needs changes. This
workspace consumes workerd through a narrow optional C ABI boundary.

## Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Platform-specific runtime code should also be checked in CI on native runners.
Useful compile gates are:

```sh
cargo clippy -p appd-runtime --target aarch64-apple-ios --all-features -- -D warnings
cargo clippy -p appd-runtime --target x86_64-pc-windows-msvc --all-features -- -D warnings
```

Run the Windows gate on a Windows runner or with a real Windows OpenSSL
toolchain.
