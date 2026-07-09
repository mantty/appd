# iOS Backgrounding

`platform/ios.rs` currently implements only `application:didFinishLaunchingWithOptions:`. iOS gets no other signal from us, so its default behavior applies: backgrounding freezes every thread in the process, including the one running workerd's event loop, mid-request if necessary. iOS may later kill a frozen process outright to reclaim memory, with no further notice. Neither case is handled gracefully today.

## Approach

Drain on background, cold-start on foreground. `workerd::server::Server::run()` takes a `kj::Promise<void> drainWhen` parameter (default `kj::NEVER_DONE`, i.e. never drain). Fulfilling it stops workerd from accepting new connections while letting in-flight requests finish, then lets `run()`'s promise resolve. This is the same mechanism Cloudflare's own edge uses to recycle isolates, not new machinery built for this purpose.

Foregrounding after a drain starts a fresh workerd instance rather than resuming the previous one. Workers are built around fast cold starts as a core property, so a clean restart is cheaper to get right than preserving live state across an OS-level freeze, and matches how the isolate lifecycle already works everywhere else this runtime runs.

## Wiring

1. `platform/ios.rs` implements `applicationDidEnterBackground:` and `applicationWillEnterForeground:` on `RuntimeDelegate`.
2. `appd_workerd.cpp`'s `doServe()` constructs an explicit promise/fulfiller pair instead of relying on `run()`'s default, and a new `appd_workerd_drain()` C ABI entry point exposes the fulfiller.
3. The workerd event loop runs on its own thread (`appd-workerd-init`); `applicationDidEnterBackground:` fires on the main thread. Triggering the fulfiller across that boundary goes through KJ's own cross-thread executor, not a raw call into the other thread's promise machinery.
4. On `applicationWillEnterForeground:`, the host starts a new runtime the same way `finish_launching()` does today, including fresh certificate generation and a fresh `navigate_to_localhost` call once the new instance is ready.
