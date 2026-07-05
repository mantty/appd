# appd workerd

This directory builds the static `workerd` SDK used by `appd-runtime` without
vendoring the upstream `cloudflare/workerd` source tree.

## Flow

1. `scripts/fetch-upstream.py` downloads the pinned upstream release declared in
   `upstream.toml` into `target/workerd/src/<tag>` and verifies the archive
   checksum.
2. `scripts/apply-overlay.py` copies the appd C ABI overlay (`overlay/appd/embed/`)
   into that source tree, then widens visibility on the upstream
   `src/workerd/server` targets `//appd/embed:appd-workerd` depends on
   (via `buildozer`, auto-installed by `go install`).
3. `scripts/build-sdk.py` builds `//appd/embed:appd-workerd` with Bazel using
   the `link_inputs_aspect` aspect (`overlay/appd/embed/link_inputs.bzl`),
   reads the resulting `.params` file, and packages the static link inputs
   plus `appd_workerd.h` into an SDK directory under
   `target/workerd/sdk/<cargo-target-triple>`.

The overlay exposes:

- `appd_workerd_serve(config_path, working_dir, listener_fd)`
- `appd_workerd_wait_ready()`

Rust binds a loopback listener on a random port, transfers that listener to
workerd once, and then lets upstream workerd accept connections directly. The
workerd config still owns TLS and mTLS policy, so the platform webviews keep the
same security model as the previous runtime.

## Example

```sh
python3 -m pip install -r workerd/requirements.txt
python3 workerd/scripts/build-sdk.py \
  --target aarch64-apple-darwin
```

Python 3.11+ uses stdlib `tomllib`; older Python versions use `tomli` from
`requirements.txt`.

The packaged SDK contains `sdk-manifest.json`, `include/appd_workerd.h`, and a
`lib/` directory with the static link inputs in linker order.

The supported desktop SDK targets are:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`

The older app-facing aliases (`macos-arm64`, `macos-x64`, `windows-x64`, and
`linux-x64`) still parse, but packaged SDKs use Cargo triples so
`appd-runtime` can find them without an environment override.

## Build Cache

Local disk and repository caches are enabled by default under
`target/workerd/cache`. To disable all cache flags:

```sh
python3 workerd/scripts/build-sdk.py \
  --target aarch64-apple-darwin \
  --cache off
```

The shared R2-backed cache is used automatically and gives dramatically
faster builds — install `bazel-remote` and export:

```sh
export APPD_BAZEL_S3_ACCESS_KEY_ID="<r2-access-key-id>"
export APPD_BAZEL_S3_SECRET_ACCESS_KEY="<r2-secret-access-key>"
```

Then run the build normally, with no extra flags:

```sh
python3 workerd/scripts/build-sdk.py \
  --target aarch64-apple-darwin
```

If both `APPD_BAZEL_S3_*` credentials are present and `--cache` is omitted,
the script defaults to `r2-read-write` and spawns a local `bazel-remote`
process backed directly by R2 over its S3 API. If they're *not* set and
`--cache` is also omitted, the build aborts with an explanatory error rather
than silently falling back to a slower cache — pass `--cache local` (or
`--cache off`) explicitly if you want to build without the shared cache on
purpose. `--cache r2-read` uses existing remote cache entries but does not
upload local results.

`bazel-remote`'s own on-disk cache under `target/workerd/cache/bazel-remote`
persists across builds (it's a plain directory, not cleaned between runs), so
a second build on the same machine is faster still — everything already
fetched is served from local disk instead of R2.
