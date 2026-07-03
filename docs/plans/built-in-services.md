# Built-In Services

appd should expose device capabilities and reusable utilities as normal workerd bindings, without requiring upstream workerd service patches.

## Core Model

The appd runtime embeds workerd with an appd-owned `kj::Network` wrapper.

Generated workerd config points internal `external` services at reserved appd addresses such as `appd:storage`, `appd:device`, or `appd:sqlite`. The appd network wrapper intercepts those addresses and returns in-process `kj::AsyncIoStream` connections instead of OS sockets.

User code does not see this transport. It sees ordinary bindings exposed through workerd `wrapped` bindings:

```js
await env.APPD_STORAGE.get("settings");
await env.APPD_DEVICE.clipboard.writeText("copied");
```

## Transport Choices

Use the smallest transport that matches the shape of the service.

| Shape | Transport | Use for |
| --- | --- | --- |
| Request/response | `external` HTTP over in-memory `kj::Network` | storage, keychain, filesystem, permissions, clipboard, notifications, SQLite |
| Raw bidirectional stream | `external` TCP plus `connect()` over in-memory `kj::Network` | audio chunks, sensor streams, logs, custom binary feeds |

## Rust Boundary

The in-process stream terminates in appd native code, which dispatches to Rust services through a small C ABI.

Rust owns the actual service implementations. The C++ workerd overlay only adapts workerd streams to the Rust dispatcher.

## Rules

- Do not expose appd native globals to user code.
- Do not add OS ports, Unix sockets, or mTLS for the internal appd service plane.
- Do not invent a protocol when Fetch already fits.
- Do use `connect()` when the service is naturally a stream.

## Non-Goals

- Replacing the WebView-to-workerd mTLS bridge.
- Forking workerd to add appd-specific service types.
- Recreating Cloudflare product bindings before there is an appd use case.
