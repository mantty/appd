# appd

**appd** is a Cloudflare Worker compatible runtime for building cross-platform apps.

Create Android, iOS, macOS, Windows, and native web applications from a single codebase using full-stack JS frameworks.

> appd is in early alpha. Expect breaking changes.

## Install

On macOS, Linux, or WSL:

```sh
curl -fsSL https://raw.githubusercontent.com/mantty/appd/main/scripts/install.sh | bash
```

On Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/mantty/appd/main/scripts/install.ps1 | iex
```

The installer downloads the newest published release, including pre-releases,
and installs the CLI and every target pack under `~/.local`. It prints the PATH
change when `~/.local/bin` is not already available.

appd doesn't add any external dependencies, but you will need the toolchain for any platforms you wish to build for:

| Platform | Requirements |
| --- | --- |
| macOS and iOS | macOS with Xcode; physical iOS devices must be registered for development in Xcode |
| Android | Android SDK 35, Java 17, Gradle, and Bash |
| Windows | 64-bit Windows |

## Build an application

appd's aim is to take any Cloudflare worker hosted site and turn it into an app.

We do not (currently) support all Cloudflare bindings to additional services they offer, but by and large a basic website written for Cloudflare workers is all you need to build an app.

Your project must have a `package.json` build script and a Wrangler config with
at least `name` and `main`. appd runs the build with pnpm, Yarn, or npm based on
the project's lockfile, then writes the native bundle under `build/<platform>`.

```sh
appd build macos --project ./my-app
```

Use `--config` to point to a specific Wrangler config file

```sh
appd build macos --config dist/server/wrangler.json
```

Platforms are `android`, `ios`, `ios-simulator`, `macos`, and `windows`.
Multiple platforms can be comma-separated, for example `macos,android`.

## Development

appd supports dev mode with HMR via wrangler, proxying to a device for native capabilities.
Dev mode is significantly less performant than a real build, but provides an excellent local dev loop.

List available local devices, simulators, and emulators:

```sh
appd devices
```

Pass a device selector and the framework's development command to `appd dev`:

```sh
appd dev macos --project ./my-app -- pnpm dev
```

By default appd expects your server to available on `http://localhost:5173` (vite's default port). Use `--server` when the
framework uses another port.

## Example

[The Astro example](examples/astro) exercises server rendering, static assets,
navigation, WebSockets, and the native geolocation plugin.

```sh
pnpm --dir examples/astro install --frozen-lockfile
appd dev macos --project examples/astro -- pnpm dev
```

To produce a native bundle instead:

```sh
pnpm --dir examples/astro run build
appd build macos \
  --project examples/astro \
  --config examples/astro/dist/server/wrangler.json \
  --skip-web-build
```

## Contributing

### Requirements

- Rust 1.96 through `rustup`; the repository selects it with
  `rust-toolchain.toml`.
- Node.js 22 and pnpm 9.9.
- CMake, Clang and libclang, Perl, and the native C/C++ toolchain for your host.

Target-pack work additionally requires:

| Target | Requirements |
| --- | --- |
| Apple | macOS with Xcode |
| Android | Android SDK 35, NDK `29.0.14206865`, Java 17, Gradle, and Bash |
| Windows | 64-bit Windows with Visual Studio 2022 C++ Build Tools and NASM |

Install the JavaScript dependencies once after cloning:

```sh
pnpm --dir plugins install --frozen-lockfile
pnpm --dir tools/esbuild-hosts install --frozen-lockfile
pnpm --dir examples/astro install --frozen-lockfile
```

### Build the CLI

```sh
cargo build --release -p appd-cli
```

The executable is written to `target/release/appd` (`appd.exe` on Windows).

### Build a target pack

Install the Rust target, then build the runtime, native shell, and runtime tools
for one target:

```sh
rustup target add aarch64-apple-darwin
cargo run -p xtask -- target-pack --target macos-arm64
```

Target packs are written to `target/appd-target-packs/<target>`. Run
`cargo run -p appd-cli -- targets` to list supported targets.

### Package the plugins

```sh
pnpm --dir plugins --filter '@appd/*' exec pnpm pack --pack-destination "$PWD/artifacts"
```

### Build the example against local sources

After building a target pack:

```sh
pnpm --dir examples/astro run build
cargo run -p appd-cli -- build macos \
  --project examples/astro \
  --config examples/astro/dist/server/wrangler.json \
  --target-pack target/appd-target-packs/macos-arm64 \
  --skip-web-build
```

Use the local CLI in development mode with:

```sh
cargo run -p appd-cli -- dev macos \
  --project examples/astro \
  --target-pack target/appd-target-packs/macos-arm64 \
  -- pnpm dev
```

### Checks

Run the common checks before submitting a change:

```sh
cargo fmt --all --check
cargo test -p appd --features native
cargo test -p appd-cli --lib --bin appd --test cli --test target_pack
cargo test -p xtask
pnpm --dir plugins lint:ts
pnpm --dir plugins test:ts
```

Platform-specific lint and build-test commands are kept in
[the Checks workflow](.github/workflows/test.yaml).
