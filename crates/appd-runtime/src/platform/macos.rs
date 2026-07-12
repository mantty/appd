//! macOS `AppKit` + `WKWebView` runtime shell.

use std::cell::OnceCell;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, RwLock};
#[cfg(feature = "bare-runtime")]
use std::sync::{Mutex, OnceLock};
use std::thread;

#[cfg(feature = "bare-runtime")]
use dispatch2::run_on_main;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSError, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSURLAuthenticationChallenge, ns_string,
};
#[cfg(feature = "bare-runtime")]
use objc2_foundation::{NSString, NSURL, NSURLRequest};
use objc2_web_kit::{
    WKNavigation, WKNavigationDelegate, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use super::apple::{
    AuthenticationCompletionHandler, SharedCertificates, bundle_state_dir, bundle_work_dir,
    clear_startup_error, handle_authentication_challenge as handle_apple_authentication_challenge,
    record_startup_error,
};

static WEBVIEW: AtomicPtr<WKWebView> = AtomicPtr::new(ptr::null_mut());
#[cfg(feature = "bare-runtime")]
static RUNTIME: OnceLock<Mutex<Option<crate::host::StartedRuntime>>> = OnceLock::new();

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
        let Some(content_view) = window.contentView() else {
            eprintln!("appd failed to create an NSWindow content view");
            std::process::exit(1);
        };
        let frame = content_view.bounds();
        let nav_delegate = NavigationDelegate::new(mtm, Arc::clone(&self.ivars().certificates));
        let webview = create_webview(mtm, frame, &nav_delegate);

        content_view.addSubview(&webview);
        WEBVIEW.store(Retained::as_ptr(&webview).cast_mut(), Ordering::Release);

        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        window.center();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        if self.ivars().navigation_delegate.set(nav_delegate).is_err()
            || self.ivars().webview.set(webview).is_err()
            || self.ivars().window.set(window).is_err()
        {
            eprintln!("appd macOS shell was initialized more than once");
            std::process::exit(1);
        }

        let certificates = Arc::clone(&self.ivars().certificates);
        let packaged_dir = bundle_work_dir();
        let state_dir = bundle_state_dir();
        let result = thread::Builder::new()
            .name("appd-bare-init".to_owned())
            .spawn(move || {
                start_runtime(&packaged_dir, &state_dir, &certificates);
            });
        if let Err(error) = result {
            eprintln!("appd failed to start the runtime thread: {error}");
        }
    }
}

#[derive(Debug)]
struct NavigationDelegateIvars {
    certificates: SharedCertificates,
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
            );
        }
    }
);

impl NavigationDelegate {
    fn new(mtm: MainThreadMarker, certificates: SharedCertificates) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationDelegateIvars { certificates });
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
) -> Retained<WKWebView> {
    let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
    let data_store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
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
    webview
}

#[cfg(feature = "bare-runtime")]
fn start_runtime(packaged_dir: &Path, state_dir: &Path, certificates: &SharedCertificates) {
    match crate::host::start_bare_runtime(packaged_dir, state_dir) {
        Ok(runtime) => {
            clear_startup_error(state_dir);
            if let Ok(mut guard) = certificates.write() {
                *guard = Some(runtime.certificates.clone());
            }
            navigate_to_localhost(runtime.port);
            let state = RUNTIME.get_or_init(|| Mutex::new(None));
            if let Ok(mut guard) = state.lock() {
                *guard = Some(runtime);
            }
        }
        Err(error) => {
            record_startup_error(state_dir, &error);
            eprintln!("appd runtime startup failed: {error:#}");
        }
    }
}

#[cfg(not(feature = "bare-runtime"))]
fn start_runtime(_: &Path, _: &Path, _: &SharedCertificates) {
    eprintln!("appd runtime was built without the bare-runtime feature");
}

#[cfg(feature = "bare-runtime")]
fn navigate_to_localhost(port: u16) {
    run_on_main(move |_| {
        let webview = WEBVIEW.load(Ordering::Acquire);
        if webview.is_null() {
            return;
        }
        let url_string = NSString::from_str(&crate::frontend_url(port));
        let url = NSURL::URLWithString(&url_string);
        let Some(url) = url else {
            eprintln!("appd failed to construct frontend URL");
            return;
        };
        let request = NSURLRequest::requestWithURL(&url);
        // SAFETY: `WEBVIEW` is written only with a live `WKWebView` retained by
        // `RuntimeDelegateIvars`, and all access here runs on the main queue.
        unsafe {
            let _ = (&*webview).loadRequest(&request);
        }
    });
}
