//! iOS `UIKit` + `WKWebView` runtime shell.

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadOnly, class, define_class, msg_send};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString, NSURL,
    NSURLAuthenticationChallenge, NSURLRequest,
};
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIApplicationLaunchOptionsKey, UIScreen,
    UIViewController, UIWindow,
};
use std::cell::OnceCell;
use std::sync::{Arc, OnceLock, RwLock};

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

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

static CERTIFICATES: OnceLock<SharedCertificates> = OnceLock::new();
const UI_VIEW_AUTORESIZING_FLEXIBLE_WIDTH: usize = 1 << 1;
const UI_VIEW_AUTORESIZING_FLEXIBLE_HEIGHT: usize = 1 << 4;

#[derive(Debug)]
struct RuntimeDelegateIvars {
    window: OnceCell<Retained<UIWindow>>,
    view_controller: OnceCell<Retained<UIViewController>>,
    webview: OnceCell<Retained<AnyObject>>,
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

    impl RuntimeDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(RuntimeDelegateIvars {
                window: OnceCell::new(),
                view_controller: OnceCell::new(),
                webview: OnceCell::new(),
                navigation_delegate: OnceCell::new(),
            });
            // SAFETY: `NSObject -init` has the expected signature for this class.
            unsafe { msg_send![super(this), init] }
        }
    }

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for RuntimeDelegate {}

    // SAFETY: `UIApplicationDelegate` has no safety requirements.
    unsafe impl UIApplicationDelegate for RuntimeDelegate {
        #[unsafe(method(application:didFinishLaunchingWithOptions:))]
        unsafe fn application_did_finish_launching_with_options(
            &self,
            _: &UIApplication,
            _: Option<&NSDictionary<UIApplicationLaunchOptionsKey, AnyObject>>,
        ) -> bool {
            self.finish_launching();
            true
        }

        #[unsafe(method(applicationDidEnterBackground:))]
        fn application_did_enter_background(&self, _: &UIApplication) {
            suspend_runtime();
        }

        #[unsafe(method(applicationWillEnterForeground:))]
        fn application_will_enter_foreground(&self, _: &UIApplication) {
            resume_runtime();
        }
    }
);

impl RuntimeDelegate {
    fn finish_launching(&self) {
        let mtm = self.mtm();
        #[allow(deprecated)]
        let screen = UIScreen::mainScreen(mtm);
        let frame = screen.bounds();
        #[allow(deprecated)]
        let window = UIWindow::initWithFrame(UIWindow::alloc(mtm), frame);
        let view_controller = UIViewController::new(mtm);
        if view_controller.view().is_none() {
            eprintln!("appd failed to create a UIViewController root view");
            std::process::exit(1);
        }

        window.setRootViewController(Some(&view_controller));
        window.makeKeyAndVisible();

        if self.ivars().view_controller.set(view_controller).is_err()
            || self.ivars().window.set(window).is_err()
        {
            eprintln!("appd iOS shell was initialized more than once");
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
                self.show_startup_error();
                return;
            }
        };
        let Some(delegate) = (unsafe { Retained::retain(std::ptr::from_ref(self).cast_mut()) })
        else {
            self.show_startup_error();
            return;
        };
        let delegate = MainThreadBound::new(delegate, mtm);
        let packaged_dir = bundle_work_dir();
        let state_dir = bundle_state_dir();
        let certificates = shared_certificates();
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
            Err(_) => self.show_startup_error(),
        }
    }

    fn load_webview(&self, mtm: MainThreadMarker, proxy_port: u16, host: &str) {
        let Some(view_controller) = self.ivars().view_controller.get() else {
            return;
        };
        let Some(view) = view_controller.view() else {
            return;
        };
        let navigation_delegate =
            NavigationDelegate::new(mtm, shared_certificates(), host.to_owned());
        let webview = create_webview(view.bounds(), &navigation_delegate, proxy_port, host);
        unsafe {
            let _: () = msg_send![&*view, addSubview: &*webview];
        }
        if self
            .ivars()
            .navigation_delegate
            .set(navigation_delegate)
            .is_err()
            || self.ivars().webview.set(webview).is_err()
        {
            eprintln!("appd iOS shell was initialized more than once");
            std::process::exit(1);
        }
    }

    fn show_startup_error(&self) {
        let Some(view_controller) = self.ivars().view_controller.get() else {
            return;
        };
        let Some(view) = view_controller.view() else {
            return;
        };
        let configuration: Retained<AnyObject> =
            unsafe { msg_send![class!(WKWebViewConfiguration), new] };
        let webview: Retained<AnyObject> = unsafe {
            msg_send![
                msg_send![class!(WKWebView), alloc],
                initWithFrame: view.bounds(),
                configuration: &*configuration
            ]
        };
        unsafe {
            let _: () = msg_send![&*webview, setAutoresizingMask: UI_VIEW_AUTORESIZING_FLEXIBLE_WIDTH | UI_VIEW_AUTORESIZING_FLEXIBLE_HEIGHT];
            let _: () = msg_send![&*webview, loadHTMLString: &*NSString::from_str("<h1>App failed to start</h1><p>Restart the app to try again.</p>"), baseURL: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*view, addSubview: &*webview];
        }
        if self.ivars().webview.set(webview).is_err() {
            eprintln!("appd iOS shell was initialized more than once");
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

    impl NavigationDelegate {
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        unsafe fn web_view_did_fail_provisional_navigation(
            &self,
            _: &AnyObject,
            _: *mut AnyObject,
            error: &NSError,
        ) {
            eprintln!("appd WebView navigation failed: {error:?}");
        }

        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        unsafe fn web_view_did_receive_authentication_challenge(
            &self,
            _: &AnyObject,
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

/// Start the iOS runtime shell.
pub fn run() -> ! {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("appd runtime must start on the main thread");
        std::process::exit(1);
    };
    let _ = shared_certificates();
    let delegate_class = NSString::from_class(RuntimeDelegate::class());
    UIApplication::main(None, Some(&delegate_class), mtm);
}

fn create_webview(
    frame: objc2_core_foundation::CGRect,
    nav_delegate: &NavigationDelegate,
    proxy_port: u16,
    host: &str,
) -> Retained<AnyObject> {
    // `objc2-web-kit` does not expose the same typed iOS `WKWebView` surface
    // used by the macOS shell, so keep raw Objective-C calls isolated here.
    let configuration: Retained<AnyObject> =
        unsafe { msg_send![class!(WKWebViewConfiguration), new] };
    let data_store: Retained<AnyObject> =
        unsafe { msg_send![class!(WKWebsiteDataStore), defaultDataStore] };
    configure_webview_proxy(&data_store, proxy_port, host);
    unsafe {
        let _: () = msg_send![&*configuration, setWebsiteDataStore: &*data_store];
    }

    let autoresizing_mask =
        UI_VIEW_AUTORESIZING_FLEXIBLE_WIDTH | UI_VIEW_AUTORESIZING_FLEXIBLE_HEIGHT;
    let webview: Retained<AnyObject> = unsafe {
        msg_send![
            msg_send![class!(WKWebView), alloc],
            initWithFrame: frame,
            configuration: &*configuration
        ]
    };
    unsafe {
        let _: () = msg_send![&*webview, setNavigationDelegate: nav_delegate];
        let _: () = msg_send![&*webview, setAutoresizingMask: autoresizing_mask];
        let url_string = NSString::from_str(&crate::frontend_url(host));
        let Some(url) = NSURL::URLWithString(&url_string) else {
            eprintln!("appd failed to construct frontend URL");
            return webview;
        };
        let request = NSURLRequest::requestWithURL(&url);
        let _: *mut AnyObject = msg_send![&*webview, loadRequest: &*request];
    }
    webview
}

#[cfg(feature = "bare-runtime")]
fn suspend_runtime() {
    set_suspended(true);
}

#[cfg(not(feature = "bare-runtime"))]
fn suspend_runtime() {}

#[cfg(feature = "bare-runtime")]
fn resume_runtime() {
    set_suspended(false);
}

#[cfg(not(feature = "bare-runtime"))]
fn resume_runtime() {}

#[cfg(feature = "bare-runtime")]
fn set_suspended(suspended: bool) {
    set_runtime_suspended(suspended);
}

fn shared_certificates() -> SharedCertificates {
    Arc::clone(CERTIFICATES.get_or_init(|| Arc::new(RwLock::new(None))))
}
