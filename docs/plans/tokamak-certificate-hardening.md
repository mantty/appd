# tokamak certificate hardening

Status: draft

## Objective

Keep tokamak's HTTPS origin and mutual TLS boundary without persisting private
certificate material in ordinary application storage. Windows retains only the
short-lived current-user client credential required by WebView2.

The app origin remains `https://<appname>.tokamak.local/`. Certificate renewal
does not change the origin, website data, cookies, CORS behaviour, or service
worker scope.

## Required invariants

- Bare accepts only TLS connections for `<appname>.tokamak.local:443`.
- Bare requests a client certificate and rejects absent, expired, and
  foreign-CA certificates.
- The listener binds only `127.0.0.1` on an operating-system-assigned port.
- A new process starts with a complete certificate set before its WebView
  navigates.
- No CA private key, server private key, or client private key is written to
  ordinary application storage.
- Certificate rotation is atomic from the perspective of a new connection.
- Failure to generate, install, replace, or remove platform credentials fails
  safely and is visible to the application host.

## Common certificate model

`SessionCertificates` owns an in-memory CA, server identity, and client
identity for one app process.

- Generate a new certificate set at process startup.
- Keep the CA for the process lifetime.
- Create server and client leaves from that CA.
- Pass the CA and server identity to Bare as in-memory data, not filesystem
  paths.
- Give every TLS connection one immutable snapshot of the current CA and
  server identity.
- Give platform adapters the current client identity through a narrow
  certificate-provider interface.

Leaves rotate before expiry. A connection that finds expired material blocks
while the replacement is created and installed. A connection that finds
renewal-due material continues with valid material while replacement happens
asynchronously.

The existing CA validates both old and replacement client leaves, so client
credential replacement does not interrupt a live session.

## Bare boundary

`RuntimeConfig` carries certificate bytes or PEM strings. It does not carry
certificate paths.

The Bare TLS adapter reads the current certificate snapshot when it creates a
TLS socket. It does not read certificate files. The runtime provides an atomic
certificate replacement operation to the adapter.

The TLS options retain `requestCert: true`, `rejectUnauthorized: true`, and
the session CA for every tokamak connection.

## Platform adapters

### macOS and iOS

Create the server-trust and client-identity objects from the current
in-memory certificate set. Do not write credentials to the Keychain or an
application support directory.

Replace the client identity before a new client leaf is required. Cancel a
challenge when the identity is unavailable or has already failed.

### Android

Pass the current client certificate and private key from the Rust runtime to
the WebView adapter only for the TLS challenge. Do not cache certificate
material in application files or the system credential store.

Replace the adapter identity before a new client leaf is required. Reject a
challenge when the identity is unavailable or has already failed.

### Windows

WebView2 selects a client identity from its mutually trusted certificate
collection; it does not accept an arbitrary certificate and private-key byte
pair. The current-user certificate store is therefore required by the current
WebView2 transport.

- Do not write tokamak certificate material to the Windows local application-data directory.
- Import only the short-lived session client identity required by WebView2.
- Remove the certificate and its persisted key container during normal
  teardown.
- On startup, remove stale credentials that belong to the same app identity.
- Use an immutable application identifier, not the display name, for all
  Windows credential and WebView-profile namespaces.
- Surface credential installation, replacement, and cleanup failures through
  host-visible diagnostics.

A current-user Windows certificate store is not a per-application security
boundary. A hostile process under the same user may be able to use a client
credential in that store. Windows does not meet a hostile-same-user mTLS
threat model until tokamak uses an operating-system application boundary or a
transport that does not require WebView2 to select a current-user identity.

## Current risks to remove

- The common runtime writes the CA and client private keys to its certificate
  cache.
- Windows gives that cache a name-derived local application-data location.
- Windows imports the client identity into `MY` with a persistent PFX import.
- Windows removes the certificate context on normal shutdown but does not
  explicitly remove its key container.
- Windows diagnostic events currently go to stderr from a GUI application.
- The Windows mTLS integration test runs only on macOS.

## Implementation sequence

1. Add `SessionCertificates` and remove certificate paths from the common
   runtime configuration.
2. Change the Bare TLS adapter to consume and atomically replace in-memory
   certificate snapshots.
3. Migrate the Apple adapters and remove their certificate-cache dependency.
4. Migrate the Android adapter and remove its certificate-cache dependency.
5. Add Windows credential lifecycle management with a unique app identifier,
   persistent-key cleanup, stale-credential cleanup, and host-visible errors.
6. Remove the common certificate-cache files and replace its rotation thread
   with connection-triggered in-memory renewal.
7. Add native integration coverage on every supported platform.

## Acceptance criteria

- Launching an app creates no certificate files in its ordinary state
  directory.
- Restarting an app changes its local CA and leaves while preserving
  `location.origin` and persistent WebView website data.
- A valid current client certificate completes a request through the actual
  packaged app path.
- No client certificate and a certificate from a different CA both fail the
  TLS handshake through the actual packaged app path on every platform.
- The listener is reachable only on `127.0.0.1`.
- Existing connections remain valid during rotation. New connections use the
  replacement material without a failed request or WebSocket reconnect.
- A failed certificate replacement leaves the last valid session material in
  service and reports the failure.
- Windows leaves no certificate or key container after normal teardown and
  removes stale tokamak credentials after an unclean exit.
- Windows stale-credential cleanup does not remove credentials belonging to a
  different app identity.
- Windows credential installation, replacement, and cleanup errors reach a
  GUI or host-visible diagnostic rather than stderr alone.
- Windows release criteria explicitly state whether same-user hostile-process
  isolation is required and, if it is, do not ship until its platform boundary
  exists.
