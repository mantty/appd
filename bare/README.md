# Bare SDK

This directory builds the upstream BareKit and addon artifacts consumed by
appd. The upstream source is pinned by tag, commit, and archive SHA-256 in
`upstream.toml`; it is downloaded into `target/bare/downloads` and extracted
under `target/bare/src`.

`CMakeLists.txt` links BareKit and the addons required by the appd JavaScript
runtime. Rust calls BareKit's worklet and IPC C APIs directly.

Build an SDK archive for a supported target:

```sh
python3 bare/scripts/build-sdk.py --target macos-arm64
python3 bare/scripts/build-sdk.py --target macos-x64
python3 bare/scripts/build-sdk.py --target ios-arm64
python3 bare/scripts/build-sdk.py --target ios-simulator-arm64
python3 bare/scripts/build-sdk.py --target ios-simulator-x64
```

Each output contains native link inputs, link arguments, and an
`sdk-manifest.json` consumed by `crates/appd-bare/build.rs`.

Set `SCCACHE` to an sccache executable to cache C and C++ compilation. The
weekly GitHub workflow configures sccache against the existing R2 build-cache
bucket and publishes both SDK archives as a GitHub release.
