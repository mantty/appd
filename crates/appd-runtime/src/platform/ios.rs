//! iOS `UIKit` + `WKWebView` runtime shell.

use std::cell::OnceCell;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;

#[cfg(feature = "workerd-ffi")]
use dispatch2::run_on_main;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{ClassType, DefinedClass, MainThreadOnly, class, define_class, msg_send};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString,
    NSURLAuthenticationChallenge,
};
#[cfg(feature = "workerd-ffi")]
use objc2_foundation::{NSURL, NSURLRequest};
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIApplicationLaunchOptionsKey, UIScreen,
    UIViewController, UIWindow,
};

use super::apple::{
    AuthenticationCompletionHandler, SharedCertificates, bundle_work_dir,
    handle_authentication_challenge as handle_apple_authentication_challenge,
};

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

static CERTIFICATES: OnceLock<SharedCertificates> = OnceLock::new();
static WEBVIEW: AtomicPtr<AnyObject> = AtomicPtr::new(ptr::null_mut());
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
        let Some(view) = view_controller.view() else {
            eprintln!("appd failed to create a UIViewController root view");
            std::process::exit(1);
        };

        let navigation_delegate = NavigationDelegate::new(mtm, shared_certificates());
        let webview = create_webview(frame, &navigation_delegate);
        unsafe {
            let _: () = msg_send![&*view, addSubview: &*webview];
        }
        WEBVIEW.store(Retained::as_ptr(&webview).cast_mut(), Ordering::Release);

        window.setRootViewController(Some(&view_controller));
        window.makeKeyAndVisible();

        if self
            .ivars()
            .navigation_delegate
            .set(navigation_delegate)
            .is_err()
            || self.ivars().webview.set(webview).is_err()
            || self.ivars().view_controller.set(view_controller).is_err()
            || self.ivars().window.set(window).is_err()
        {
            eprintln!("appd iOS shell was initialized more than once");
            std::process::exit(1);
        }

        let certificates = shared_certificates();
        let work_dir = bundle_work_dir();
        let _ = thread::Builder::new()
            .name("appd-workerd-init".to_owned())
            .spawn(move || {
                start_runtime(&work_dir, &certificates);
            });
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
) -> Retained<AnyObject> {
    // `objc2-web-kit` does not expose the same typed iOS `WKWebView` surface
    // used by the macOS shell, so keep raw Objective-C calls isolated here.
    let configuration: Retained<AnyObject> =
        unsafe { msg_send![class!(WKWebViewConfiguration), new] };
    let data_store: Retained<AnyObject> =
        unsafe { msg_send![class!(WKWebsiteDataStore), nonPersistentDataStore] };
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
    }
    webview
}

#[cfg(feature = "workerd-ffi")]
fn start_runtime(work_dir: &Path, certificates: &SharedCertificates) {
    match crate::host::start_workerd_bridge(work_dir) {
        Ok(runtime) => {
            if let Ok(mut guard) = certificates.write() {
                *guard = Some(runtime.certificates.clone());
            }
            navigate_to_localhost(runtime.port);
            let _ = runtime.join();
        }
        Err(error) => {
            eprintln!("appd runtime startup failed: {error:#}");
        }
    }
}

#[cfg(not(feature = "workerd-ffi"))]
fn start_runtime(_: &Path, _: &SharedCertificates) {
    eprintln!("appd runtime was built without the workerd-ffi feature");
}

#[cfg(feature = "workerd-ffi")]
fn navigate_to_localhost(port: u16) {
    run_on_main(move |_| {
        let webview = WEBVIEW.load(Ordering::Acquire);
        if webview.is_null() {
            return;
        }
        let url_string = NSString::from_str(&crate::frontend_url(port, false));
        let Some(url) = NSURL::URLWithString(&url_string) else {
            eprintln!("appd failed to construct frontend URL");
            return;
        };
        let request = NSURLRequest::requestWithURL(&url);
        unsafe {
            let _: *mut AnyObject = msg_send![&*webview, loadRequest: &*request];
        }
    });
}

fn shared_certificates() -> SharedCertificates {
    Arc::clone(CERTIFICATES.get_or_init(|| Arc::new(RwLock::new(None))))
}
