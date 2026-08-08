# Bare runtime

This directory contains appd's Bare runtime code and native module sources.
`CMakeLists.txt` follows BareKit's source-build pattern: it fetches pinned
BareKit source, uses JavaScriptCore on Apple platforms, and statically links
the runtime's native modules into BareKit. Rust calls BareKit's worklet and
IPC C APIs directly.

Build a target pack, including the static BareKit artifact:

```sh
appd pack build --target macos-arm64
```

The artifact is written to `target/bare/sdk/<target>/runtime`. Set `SCCACHE`
to an sccache executable to cache C and C++ compilation. The weekly GitHub
workflow publishes those artifacts as releases.
