# appd development mode

Status: active. The core development loop is implemented; this document is a
delta plan and intentionally records only the work that remains.

## 1. Select Apple signing assets automatically

Physical iOS development currently needs both
`APPD_IOS_SIGNING_IDENTITY` and `APPD_IOS_PROVISIONING_PROFILE`. Without them,
the Apple entrypoint can only ad-hoc sign, which is not installable on a
physical device.

The CLI should resolve signing assets before it invokes the iOS target-pack
entrypoint:

1. Keep the two environment variables as explicit overrides. When both are
   present, use them exactly as supplied.
2. On macOS, enumerate signing identities in the login keychain and retain
   only identities with a certificate and usable private key.
3. Search the Xcode-managed and legacy provisioning-profile directories for
   `.mobileprovision` files. Decode each CMS-wrapped profile and read its
   application identifier, team identifier, expiry, platform, device list,
   and developer certificates.
4. Match a profile and identity against the generated bundle identifier, the
   selected physical device UDID, certificate/team, and profile validity.
5. Reuse a previously selected exact match. If several valid matches remain,
   ask the developer to choose one and persist the choice for the project,
   bundle identifier, and device. Never choose an arbitrary identity.
6. If no match exists, print an actionable error explaining that a development
   certificate and device-registered profile must be created in Xcode or the
   Apple Developer tooling. Do not attempt account login, device registration,
   or profile creation from appd.
7. Pass the resolved identity and profile to the existing Apple entrypoint,
   which remains responsible for embedding the profile and running `codesign`.

The cache must store stable identifiers (device UDID, team, certificate
fingerprint, and profile metadata), not a temporary profile path alone. Expired
or deleted assets must trigger a fresh match.

### Rust crate note

No Rust crate provides this complete flow, so appd must own the matching,
selection, and persistence policy.

- [`security-framework`](https://docs.rs/security-framework/latest/security_framework/)
  is the appropriate macOS integration for Keychain queries. Its identity
  search can return `SecIdentity` values, from which appd can inspect the
  certificate and private-key availability.
- The same crate exposes CMS decoding. Use it to unwrap provisioning profiles;
  [`plist`](https://docs.rs/plist/latest/plist/) can decode the resulting
  property list, and the workspace's existing
  [`x509-parser`](https://docs.rs/x509-parser/latest/x509_parser/) dependency
  can inspect certificate fields and fingerprints.
- [`mprovision`](https://docs.rs/mprovision/latest/mprovision/) may help with
  profile discovery or filtering, but it is a small helper rather than an
  end-to-end signing solution. Add it only if fixture testing shows that it
  removes meaningful code.
- [`apple-codesign`](https://docs.rs/apple-codesign/latest/apple_codesign/)
  is useful for parsing and producing Apple signatures, but it deliberately
  does not obtain private keys from the macOS Keychain, so it does not replace
  `security-framework` for this feature.
- A prompt crate such as
  [`dialoguer`](https://docs.rs/dialoguer/latest/dialoguer/) is optional; a
  small CLI prompt is sufficient if adding the dependency is not worthwhile.

Tests must use fixture profiles and certificates, cover exact-match reuse,
ambiguous selection, expiry, wrong-device profiles, and missing private keys,
and avoid requiring a real Apple account in CI.

## 2. Bridge backend plugin calls during development

Frontend plugin calls already use the device WebView bridge. Backend plugin
calls remain outstanding because development Worker code executes in the host
Cloudflare-compatible runtime:

```text
host Worker binding/service
    -> host-side appd bridge
    -> development relay
    -> device runtime
    -> native plugin implementation
```

Implement the host binding/service and the device endpoint as one narrow,
authenticated contract. It must:

- identify the app, device, and development session;
- carry JSON values and binary payloads;
- support the plugin's required streaming or long-running calls;
- propagate cancellation, timeouts, disconnects, and native errors; and
- return the same unsupported-capability error used by production appd.

The browser must never receive the backend capability credential. Backend
plugin JavaScript must remain valid Cloudflare Worker code; QuickJS-only code
must fail in the host development runtime rather than creating a second
device-side module-execution path.

## 3. Rebuild for native-input changes

The initial development build, installation, and launch are implemented. The
remaining lifecycle work is to watch native shell and native-plugin inputs
while the framework process continues running.

When such an input changes, appd should rebuild the same target pack,
reinstall/relaunch the selected target, and preserve the existing framework
server, relay, and target selection. JavaScript, styles, public assets, and
supported Worker edits must continue using the framework's HMR/reload path
without a native rebuild.

## 4. End-to-end coverage

Add representative tests and manual checks for the implemented path and the
remaining work above:

- Astro with the Cloudflare integration, including style/module HMR, SSR/API
  requests, and browser WebSockets through the device WebView;
- plain Vite plus the Cloudflare Worker environment;
- the agreed vinext/Next Cloudflare setup;
- host targets on macOS, Windows, and Linux where a target pack exists;
- iOS Simulator, physical iOS, Android emulator, and supported physical
  Android targets;
- unsupported Wrangler bindings producing one readable warning block while
  development continues;
- framework restart or temporary host loss recovering on the existing session;
- backend native plugin calls once the bridge is available; and
- native-input rebuild/reinstall without restarting the framework process.

The tests must continue to assert the important split: appd dev uses the
framework's host Cloudflare runtime and normal HMR machinery, while appd
preview/build exercise the packaged QuickJS runtime.

## References

- [Cloudflare Vite plugin](https://developers.cloudflare.com/workers/vite-plugin/)
- [Cloudflare local development](https://developers.cloudflare.com/workers/local-development/)
- [Astro Cloudflare adapter](https://docs.astro.build/en/guides/integrations-guide/cloudflare/)
- [Cloudflare Next.js guide](https://developers.cloudflare.com/workers/framework-guides/web-apps/nextjs/)
- [Apple TN3125: Inside Code Signing: Provisioning Profiles](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)

