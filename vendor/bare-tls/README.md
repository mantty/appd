# bare-tls

Transport Layer Security (TLS) streams for JavaScript, built on BoringSSL. Provides both a low-level `Socket` class that wraps any duplex stream with TLS and higher-level `createServer()` and `connect()` functions for TLS over TCP, similar to `node:tls`. Mozilla root certificates are bundled for out-of-the-box certificate verification.

```
npm i bare-tls
```

## Usage

```js
const tls = require('bare-tls')
const fs = require('bare-fs')

const server = tls.createServer(
  {
    cert: fs.readFileSync('cert.pem'),
    key: fs.readFileSync('key.pem')
  },
  (socket) => {
    socket.on('data', (data) => socket.end('pong')).on('close', () => server.close())
  }
)

server.listen(8443)

const client = tls.connect({ port: 8443, host: 'localhost' })

client.on('data', (data) => console.log(data)).end('ping')
```

## API

#### `const socket = new tls.Socket(stream[, options])`

Wraps an existing duplex `stream` with TLS. The underlying `stream` handles transport; the TLS socket handles encryption and decryption.

Options include:

```js
options = {
  isServer: false,
  cert: null,
  key: null,
  host: null,
  rejectUnauthorized: true,
  ca: null,
  alpnProtocols: null,
  eagerOpen: true,
  allowHalfOpen: true,
  readBufferSize: 65536
}
```

`isServer` controls whether the socket acts as a TLS server or client. If `true`, `cert` and `key` must be provided.

`cert` and `key` are `Buffer`s containing PEM-encoded certificate and private key data, respectively.

`host` enables hostname verification against the server certificate. For DNS names it is also sent as the SNI (Server Name Indication) extension; for IP literals it is matched against the certificate's IP SANs and SNI is suppressed per RFC 6066. Required for client sockets unless `rejectUnauthorized` is `false`.

`rejectUnauthorized` controls whether the client rejects connections when certificate verification fails. Defaults to `true`.

`ca` is a `Buffer` containing one or more PEM-encoded CA certificates. When provided, only these CAs are used for verification instead of the bundled Mozilla root certificates.

`alpnProtocols` is an array of ALPN protocol name strings, ordered by preference.

#### `socket.socket`

The underlying duplex stream.

#### `socket.encrypted`

Always `true`.

#### `socket.alpnProtocol`

The negotiated ALPN protocol as a string, or `null` if no protocol was negotiated.

#### `event: 'connect'`

Emitted when the TLS handshake completes.

#### `const server = tls.createServer([options][, onconnection])`

Creates a TLS server that listens for TCP connections and wraps them with TLS. Incoming connections are emitted as `'connection'` events with a `tls.Socket` instance. Options are the same as `tls.Socket`, plus any options supported by <https://github.com/holepunchto/bare-net>.

#### `server.listen(...args)`

Start listening for connections. Arguments are passed through to the underlying TCP server.

#### `server.close([onclose])`

Stop listening for connections.

#### `server.address()`

Returns the bound address of the server.

#### `server.listening`

Whether or not the server is listening.

#### `server.ref()`

Ref the server.

#### `server.unref()`

Unref the server.

#### `event: 'listening'`

Emitted when the server starts listening.

#### `event: 'connection'`

Emitted when a new TLS connection is established.

#### `event: 'close'`

Emitted when the server closes.

#### `event: 'error'`

Emitted on server error.

#### `const socket = tls.connect(options[, onconnect])`

Creates a TCP connection and wraps it with TLS. `options` are passed to both the underlying TCP socket and `tls.Socket`. At minimum, `port` must be specified. `host` is used for both the TCP connection target and TLS hostname verification, and defaults to `'localhost'`.

#### `const socket = tls.connect(port[, host][, onconnect])`

Shorthand for `tls.connect({ port, host })`.

## License

Apache-2.0
