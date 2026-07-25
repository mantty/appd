# Bare SDK

This directory is the native boundary between appd and upstream BareKit. The
upstream source is pinned by tag, commit, and archive SHA-256 in
`upstream.toml`; it is downloaded into `target/bare/downloads` and extracted
under `target/bare/src`.

`CMakeLists.txt` links the addons required by the appd JavaScript runtime and
builds `appd_bare`, a small C API implemented in `src/appd_bare.c`. The API
starts one BareKit worklet, passes its immutable runtime configuration, and
forwards suspend, resume, and terminate lifecycle events.

Build an SDK archive for a supported target:

```sh
python3 bare/scripts/build-sdk.py --target macos-arm64
python3 bare/scripts/build-sdk.py --target macos-x64
python3 bare/scripts/build-sdk.py --target ios-arm64
python3 bare/scripts/build-sdk.py --target ios-simulator-arm64
python3 bare/scripts/build-sdk.py --target ios-simulator-x64
```

Each output contains the public header, native link inputs, link arguments,
and an `sdk-manifest.json` consumed by `crates/appd-bare/build.rs`.

Set `SCCACHE` to an sccache executable to cache C and C++ compilation. The
weekly GitHub workflow configures sccache against the existing R2 build-cache
bucket and publishes both SDK archives as a GitHub release.
