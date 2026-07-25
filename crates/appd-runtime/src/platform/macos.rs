//! macOS `AppKit` + `WKWebView` runtime shell.

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSError, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSURL, NSURLAuthenticationChallenge, NSURLRequest, ns_string,
};
use objc2_web_kit::{
    WKNavigation, WKNavigationDelegate, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use std::cell::OnceCell;
use std::sync::{Arc, RwLock};

use super::apple::{
    AuthenticationCompletionHandler, SharedCertificates, app_host,
    handle_authentication_challenge as handle_apple_authentication_challenge,
};
#[cfg(feature = "bare-runtime")]
use super::apple::{
    bundle_state_dir, bundle_work_dir, record_startup_error, set_runtime_suspended, start_runtime,
    store_runtime,
};
use super::proxy::configure_webview_proxy;

#[derive(Debug)]
struct RuntimeDelegateIvars {
    certificates: SharedCertificates,
    window: OnceCell<Retained<NSWindow>>,
    webview: OnceCell<Retained<WKWebView>>,
    navigation_delegate: OnceCell<Retained<NavigationDelegate>>,
}

define_class!(
    // SAFETY:
    // - The superclass `NSObject` does not have subclassing requirements.
    // - `RuntimeDelegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    #[derive(Debug)]
    #[thread_kind = MainThreadOnly]
    #[ivars = RuntimeDelegateIvars]
    struct RuntimeDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for RuntimeDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSApplicationDelegate for RuntimeDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, notification: &NSNotification) {
            self.finish_launching(notification);
        }

        #[unsafe(method(applicationDidResignActive:))]
        fn application_did_resign_active(&self, _: &NSNotification) {
            #[cfg(feature = "bare-runtime")]
            set_runtime_suspended(true);
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn application_did_become_active(&self, _: &NSNotification) {
            #[cfg(feature = "bare-runtime")]
            set_runtime_suspended(false);
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _: &NSApplication) -> bool {
            true
        }
    }

    // SAFETY: `NSWindowDelegate` has no safety requirements.
    unsafe impl NSWindowDelegate for RuntimeDelegate {}
);

impl RuntimeDelegate {
    fn new(mtm: MainThreadMarker, certificates: SharedCertificates) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RuntimeDelegateIvars {
            certificates,
            window: OnceCell::new(),
            webview: OnceCell::new(),
            navigation_delegate: OnceCell::new(),
        });
        // SAFETY: `NSObject -init` has the expected signature for this class.
        unsafe { msg_send![super(this), init] }
    }

    fn finish_launching(&self, notification: &NSNotification) {
        let mtm = self.mtm();
        let app = notification
            .object()
            .and_then(|object| object.downcast::<NSApplication>().ok())
            .unwrap_or_else(|| NSApplication::sharedApplication(mtm));

        let window = create_window(mtm);
        if window.contentView().is_none() {
            eprintln!("appd failed to create an NSWindow content view");
            std::process::exit(1);
        }
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.center();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        if self.ivars().window.set(window).is_err() {
            eprintln!("appd macOS shell was initialized more than once");
            std::process::exit(1);
        }

        #[cfg(feature = "bare-runtime")]
        self.start_runtime(mtm);
        #[cfg(not(feature = "bare-runtime"))]
        self.load_webview(mtm, 0, "app.appd.local");
    }

    #[cfg(feature = "bare-runtime")]
    fn start_runtime(&self, mtm: MainThreadMarker) {
        let host = match app_host() {
            Ok(host) => host,
            Err(error) => {
                record_startup_error(&bundle_state_dir(), &error);
                self.show_startup_error(mtm);
                return;
            }
        };
        let Some(delegate) = (unsafe { Retained::retain(std::ptr::from_ref(self).cast_mut()) })
        else {
            self.show_startup_error(mtm);
            return;
        };
        let delegate = MainThreadBound::new(delegate, mtm);
        let packaged_dir = bundle_work_dir();
        let state_dir = bundle_state_dir();
        let certificates = Arc::clone(&self.ivars().certificates);
        DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
            DispatchQoS::UserInitiated,
        ))
        .exec_async(move || {
            let runtime = start_runtime(&packaged_dir, &state_dir, &host, &certificates);
            DispatchQueue::main().exec_async(move || {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                delegate.get(mtm).finish_runtime(mtm, host, runtime);
            });
        });
    }

    #[cfg(feature = "bare-runtime")]
    fn finish_runtime(
        &self,
        mtm: MainThreadMarker,
        host: String,
        runtime: crate::RuntimeResult<crate::host::StartedRuntime>,
    ) {
        match runtime {
            Ok(runtime) => {
                self.load_webview(mtm, runtime.port, &host);
                store_runtime(runtime);
                drop(host);
            }
            Err(_) => self.show_startup_error(mtm),
        }
    }

    fn load_webview(&self, mtm: MainThreadMarker, proxy_port: u16, host: &str) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        let Some(content_view) = window.contentView() else {
            return;
        };
        let nav_delegate =
            NavigationDelegate::new(mtm, Arc::clone(&self.ivars().certificates), host.to_owned());
        let webview = create_webview(mtm, content_view.bounds(), &nav_delegate, proxy_port, host);
        content_view.addSubview(&webview);
        if self.ivars().navigation_delegate.set(nav_delegate).is_err()
            || self.ivars().webview.set(webview).is_err()
        {
            eprintln!("appd macOS shell was initialized more than once");
            std::process::exit(1);
        }
    }

    fn show_startup_error(&self, mtm: MainThreadMarker) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        let Some(content_view) = window.contentView() else {
            return;
        };
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                content_view.bounds(),
                &configuration,
            )
        };
        unsafe {
            webview.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            webview.loadHTMLString_baseURL(
                &NSString::from_str(
                    "<h1>App failed to start</h1><p>Check startup-error.log for details.</p>",
                ),
                None,
            );
        }
        content_view.addSubview(&webview);
        if self.ivars().webview.set(webview).is_err() {
            eprintln!("appd macOS shell was initialized more than once");
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
struct NavigationDelegateIvars {
    certificates: SharedCertificates,
    host: String,
}

define_class!(
    // SAFETY:
    // - The superclass `NSObject` does not have subclassing requirements.
    // - `NavigationDelegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    #[derive(Debug)]
    #[thread_kind = MainThreadOnly]
    #[ivars = NavigationDelegateIvars]
    struct NavigationDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for NavigationDelegate {}

    // SAFETY: `WKNavigationDelegate` has no safety requirements.
    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        unsafe fn web_view_did_finish_navigation(&self, _: &WKWebView, _: Option<&WKNavigation>) {}

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        unsafe fn web_view_did_fail_provisional_navigation(
            &self,
            _: &WKWebView,
            _: Option<&WKNavigation>,
            error: &NSError,
        ) {
            eprintln!("appd WebView navigation failed: {error:?}");
        }

        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        unsafe fn web_view_did_receive_authentication_challenge(
            &self,
            _: &WKWebView,
            challenge: &NSURLAuthenticationChallenge,
            completion_handler: &AuthenticationCompletionHandler,
        ) {
            handle_apple_authentication_challenge(
                challenge,
                completion_handler,
                &self.ivars().certificates,
                &self.ivars().host,
            );
        }
    }
);

impl NavigationDelegate {
    fn new(
        mtm: MainThreadMarker,
        certificates: SharedCertificates,
        host: String,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationDelegateIvars { certificates, host });
        // SAFETY: `NSObject -init` has the expected signature for this class.
        unsafe { msg_send![super(this), init] }
    }
}

/// Start the macOS runtime shell.
pub fn run() -> ! {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("appd runtime must start on the main thread");
        std::process::exit(1);
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let certificates = Arc::new(RwLock::new(None));
    let delegate = RuntimeDelegate::new(mtm, certificates);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    std::process::exit(0);
}

fn create_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    // SAFETY: The window is retained by `RuntimeDelegateIvars` for its lifetime.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1024.0, 768.0)),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(ns_string!("appd"));
    // SAFETY: The delegate owns the window, so AppKit must not release it when closed.
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

fn create_webview(
    mtm: MainThreadMarker,
    frame: NSRect,
    nav_delegate: &NavigationDelegate,
    proxy_port: u16,
    host: &str,
) -> Retained<WKWebView> {
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    let data_store = unsafe { WKWebsiteDataStore::defaultDataStore(mtm) };
    let data_store_ptr = Retained::as_ptr(&data_store).cast();
    configure_webview_proxy(unsafe { &*data_store_ptr }, proxy_port, host);
    unsafe { configuration.setWebsiteDataStore(&data_store) };
    let webview = unsafe {
        WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
    };
    unsafe {
        webview.setNavigationDelegate(Some(ProtocolObject::from_ref(nav_delegate)));
        webview.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
    }
    let url_string = NSString::from_str(&crate::frontend_url(host));
    let Some(url) = NSURL::URLWithString(&url_string) else {
        eprintln!("appd failed to construct frontend URL");
        return webview;
    };
    let request = NSURLRequest::requestWithURL(&url);
    unsafe {
        let _ = webview.loadRequest(&request);
    }
    webview
}
