//! Windows `Win32` + `WebView2` runtime shell.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, AtomicU16, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW, CreateCoreWebView2Environment,
    ICoreWebView2, ICoreWebView2_5, ICoreWebView2_14, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2ServerCertificateErrorDetectedEventArgs,
};
use webview2_com::{
    ClientCertificateRequestedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ServerCertificateErrorDetectedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{E_FAIL, E_POINTER, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi;
use windows::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_NAME_ISSUER_FLAG, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    CERT_STORE_ADD_REPLACE_EXISTING, CRYPT_INTEGER_BLOB, CRYPT_KEY_FLAGS,
    CertAddCertificateContextToStore, CertCloseStore, CertDeleteCertificateFromStore,
    CertEnumCertificatesInStore, CertGetNameStringW, CertOpenSystemStoreW, HCERTSTORE,
    PFXImportCertStore,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader;
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    KillTimer, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SW_SHOW, SetTimer, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_DESTROY, WM_SIZE, WM_TIMER, WNDCLASSW,
    WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::{HSTRING, Interface, w};

use crate::certs::CertificateBundle;
use crate::host::{PreparedRuntime, work_dir_next_to_current_exe_or_default};
use crate::platform::webview2_args;

const WM_APP_NAVIGATE: u32 = WM_APP + 1;
const NAVIGATION_TIMER_ID: usize = 1;
const NAVIGATION_RETRY_LIMIT: u32 = 50;
const WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
const LOCAL_CA_COMMON_NAME: &str = "appd local ca";
const CLIENT_COMMON_NAME: &str = "appd client";

static HWND_VALUE: AtomicIsize = AtomicIsize::new(0);
static PENDING_PORT: AtomicU16 = AtomicU16::new(0);
static NAVIGATION_RETRIES: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static WEBVIEW_STATE: RefCell<Option<WebViewState>> = const { RefCell::new(None) };
}

type PlatformResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
struct WebViewState {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
}

/// Start the Windows runtime shell.
pub fn run() -> ! {
    if let Err(error) = run_inner() {
        eprintln!("appd Windows runtime failed: {error:#}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

#[cfg(feature = "workerd-ffi")]
fn run_inner() -> PlatformResult<()> {
    let work_dir = work_dir_next_to_current_exe_or_default();
    let prepared = crate::host::prepare_workerd_bridge(&work_dir)?;
    configure_webview2_trust(&prepared.certificates)?;
    import_client_certificate(&prepared.certificates)?;

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    let hwnd = create_window()?;
    HWND_VALUE.store(hwnd.0 as isize, Ordering::Release);
    let webview_state = create_webview(hwnd, &prepared.certificates)?;
    WEBVIEW_STATE.with(|state| {
        *state.borrow_mut() = Some(webview_state);
    });
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = Gdi::UpdateWindow(hwnd);
    }

    start_runtime_thread(prepared);
    run_message_loop()?;
    Ok(())
}

#[cfg(not(feature = "workerd-ffi"))]
fn run_inner() -> PlatformResult<()> {
    eprintln!("appd runtime was built without the workerd-ffi feature");
    std::process::exit(1);
}

#[cfg(feature = "workerd-ffi")]
fn start_runtime_thread(prepared: PreparedRuntime) {
    let _ = thread::Builder::new()
        .name("appd-workerd-init".to_owned())
        .spawn(
            move || match crate::host::start_prepared_workerd_bridge(prepared) {
                Ok(runtime) => {
                    request_navigation(runtime.port);
                    let _ = runtime.join();
                }
                Err(error) => {
                    eprintln!("appd runtime startup failed: {error:#}");
                }
            },
        );
}

fn create_window() -> windows::core::Result<HWND> {
    let hmodule = unsafe { LibraryLoader::GetModuleHandleW(None)? };
    let class = w!("appd-window");
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hmodule.0),
        lpszClassName: class,
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&raw const window_class);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("appd"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1024,
            768,
            None,
            None,
            Some(windows::Win32::Foundation::HINSTANCE(hmodule.0)),
            None,
        )
    }
}

fn create_webview(hwnd: HWND, certs: &CertificateBundle) -> PlatformResult<WebViewState> {
    let environment = create_webview_environment()?;
    let controller = create_webview_controller(&environment, hwnd)?;
    resize_controller(hwnd, &controller)?;
    unsafe { controller.SetIsVisible(true)? };
    let webview = unsafe { controller.CoreWebView2()? };
    register_certificate_handlers(&webview, certs)?;
    Ok(WebViewState {
        controller,
        webview,
    })
}

fn create_webview_environment() -> PlatformResult<ICoreWebView2Environment> {
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|handler| unsafe {
            CreateCoreWebView2Environment(&handler).map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, environment| {
            error_code?;
            let environment = environment.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            if tx.send(environment).is_err() {
                return Err(windows::core::Error::from(E_FAIL));
            }
            Ok(())
        }),
    )?;
    Ok(rx.recv()?)
}

fn create_webview_controller(
    environment: &ICoreWebView2Environment,
    hwnd: HWND,
) -> PlatformResult<ICoreWebView2Controller> {
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new({
            let environment = environment.clone();
            move |handler| unsafe {
                environment
                    .CreateCoreWebView2Controller(hwnd, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }
        }),
        Box::new(move |error_code, controller| {
            error_code?;
            let controller = controller.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            if tx.send(controller).is_err() {
                return Err(windows::core::Error::from(E_FAIL));
            }
            Ok(())
        }),
    )?;
    Ok(rx.recv()?)
}

fn register_certificate_handlers(
    webview: &ICoreWebView2,
    certs: &CertificateBundle,
) -> windows::core::Result<()> {
    let webview14: ICoreWebView2_14 = webview.cast()?;
    let expected_server_cert_pem = certs.server_cert_pem.clone();
    let server_handler =
        ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };

            let mut uri = windows::core::PWSTR::null();
            unsafe { args.RequestUri(&raw mut uri)? };
            if !is_local_frontend_url(&take_pwstr(uri))
                || !server_certificate_matches(&args, &expected_server_cert_pem)?
            {
                return Ok(());
            }

            unsafe {
                args.SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW)?;
            }
            Ok(())
        }));
    let mut server_token = 0;
    unsafe {
        webview14.add_ServerCertificateErrorDetected(&server_handler, &raw mut server_token)?;
    };

    let webview5: ICoreWebView2_5 = webview.cast()?;
    let client_handler = ClientCertificateRequestedEventHandler::create(Box::new(|_, args| {
        let Some(args) = args else {
            return Ok(());
        };

        let mut host = windows::core::PWSTR::null();
        unsafe { args.Host(&raw mut host)? };
        if !is_local_frontend_host(&take_pwstr(host)) {
            return Ok(());
        }

        let certs = unsafe { args.MutuallyTrustedCertificates()? };
        let mut count = 0;
        unsafe { certs.Count(&raw mut count)? };
        if count == 0 {
            return Ok(());
        }

        let cert = unsafe { certs.GetValueAtIndex(0)? };
        unsafe {
            args.SetSelectedCertificate(&cert)?;
            args.SetHandled(true)?;
        }
        Ok(())
    }));
    let mut client_token = 0;
    unsafe { webview5.add_ClientCertificateRequested(&client_handler, &raw mut client_token)? };
    Ok(())
}

fn run_message_loop() -> windows::core::Result<()> {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&raw mut message, None, 0, 0).0 };
        if result == -1 {
            return Err(windows::core::Error::from_thread());
        }
        if result == 0 {
            return Ok(());
        }
        unsafe {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            WEBVIEW_STATE.with(|state| {
                if let Some(state) = state.borrow().as_ref() {
                    let _ = resize_controller(hwnd, &state.controller);
                }
            });
            LRESULT(0)
        }
        WM_APP_NAVIGATE => {
            navigate_or_retry(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), NAVIGATION_TIMER_ID);
                let _ = PostMessageW(Some(hwnd), WM_APP_NAVIGATE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn resize_controller(
    hwnd: HWND,
    controller: &ICoreWebView2Controller,
) -> windows::core::Result<()> {
    let mut bounds = RECT::default();
    unsafe {
        GetClientRect(hwnd, &raw mut bounds)?;
        controller.SetBounds(bounds)
    }
}

fn navigate_or_retry(hwnd: HWND) {
    let port = PENDING_PORT.load(Ordering::Acquire);
    if port == 0 {
        return;
    }

    WEBVIEW_STATE.with(|state| {
        if let Some(state) = state.borrow().as_ref() {
            let url = HSTRING::from(crate::frontend_url(port, true));
            if let Err(error) = unsafe { state.webview.Navigate(&url) } {
                eprintln!("appd WebView2 navigation failed: {error:#}");
            }
            return;
        }

        let retries = NAVIGATION_RETRIES.fetch_add(1, Ordering::AcqRel) + 1;
        if retries <= NAVIGATION_RETRY_LIMIT {
            unsafe {
                let _ = SetTimer(Some(hwnd), NAVIGATION_TIMER_ID, 200, None);
            }
        } else if retries == NAVIGATION_RETRY_LIMIT + 1 {
            eprintln!("appd WebView2 failed to initialize before navigation");
        }
    });
}

#[cfg(feature = "workerd-ffi")]
fn request_navigation(port: u16) {
    PENDING_PORT.store(port, Ordering::Release);
    let raw = HWND_VALUE.load(Ordering::Acquire);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw as *mut _)),
                WM_APP_NAVIGATE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

fn configure_webview2_trust(certs: &CertificateBundle) -> PlatformResult<()> {
    let pin = certs.server_spki_sha256_base64()?;
    let existing_args = std::env::var(WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS).ok();
    let args = webview2_args::append_spki_pin(existing_args.as_deref(), &pin);
    unsafe {
        std::env::set_var(WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, args);
    }
    Ok(())
}

fn import_client_certificate(certs: &CertificateBundle) -> PlatformResult<()> {
    let cb_data = u32::try_from(certs.client_p12_der.len())?;
    let mut blob = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: certs.client_p12_der.as_ptr().cast_mut(),
    };
    let password = HSTRING::from(certs.p12_password.as_str());
    let temp_store = unsafe { PFXImportCertStore(&raw mut blob, &password, CRYPT_KEY_FLAGS(0))? };
    let personal_store = unsafe { CertOpenSystemStoreW(None, w!("MY"))? };

    let result = (|| {
        delete_existing_appd_certificates(personal_store)?;
        copy_certificates_to_store(temp_store, personal_store)
    })();
    unsafe {
        let _ = CertCloseStore(Some(temp_store), 0);
        let _ = CertCloseStore(Some(personal_store), 0);
    }
    let copied = result?;
    if copied == 0 {
        return Err("client certificate P12 did not contain any certificates".into());
    }
    Ok(())
}

fn copy_certificates_to_store(
    source: HCERTSTORE,
    destination: HCERTSTORE,
) -> windows::core::Result<u32> {
    let mut count = 0;
    let mut context: Option<*const CERT_CONTEXT> = None;
    loop {
        let next = unsafe { CertEnumCertificatesInStore(source, context) };
        if next.is_null() {
            break;
        }
        unsafe {
            CertAddCertificateContextToStore(
                Some(destination),
                next,
                CERT_STORE_ADD_REPLACE_EXISTING,
                None,
            )?;
        }
        count += 1;
        context = Some(next);
    }
    Ok(count)
}

fn delete_existing_appd_certificates(store: HCERTSTORE) -> windows::core::Result<()> {
    loop {
        if !delete_next_appd_certificate(store)? {
            return Ok(());
        }
    }
}

fn delete_next_appd_certificate(store: HCERTSTORE) -> windows::core::Result<bool> {
    let mut context: Option<*const CERT_CONTEXT> = None;
    loop {
        let next = unsafe { CertEnumCertificatesInStore(store, context) };
        if next.is_null() {
            return Ok(false);
        }
        if is_appd_certificate(next) {
            unsafe { CertDeleteCertificateFromStore(next)? };
            return Ok(true);
        }
        context = Some(next);
    }
}

fn is_appd_certificate(context: *const CERT_CONTEXT) -> bool {
    let subject = certificate_name(context, 0);
    let issuer = certificate_name(context, CERT_NAME_ISSUER_FLAG);
    issuer == LOCAL_CA_COMMON_NAME
        && (subject == CLIENT_COMMON_NAME || subject == LOCAL_CA_COMMON_NAME)
}

fn certificate_name(context: *const CERT_CONTEXT, flags: u32) -> String {
    let length =
        unsafe { CertGetNameStringW(context, CERT_NAME_SIMPLE_DISPLAY_TYPE, flags, None, None) };
    if length <= 1 {
        return String::new();
    }

    let mut buffer = vec![0u16; length as usize];
    let written = unsafe {
        CertGetNameStringW(
            context,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            flags,
            None,
            Some(buffer.as_mut_slice()),
        )
    };
    if written <= 1 {
        return String::new();
    }

    String::from_utf16_lossy(&buffer[..written.saturating_sub(1) as usize])
}

fn server_certificate_matches(
    args: &ICoreWebView2ServerCertificateErrorDetectedEventArgs,
    expected_pem: &str,
) -> windows::core::Result<bool> {
    let certificate = unsafe { args.ServerCertificate()? };
    let mut pem = windows::core::PWSTR::null();
    unsafe { certificate.ToPemEncoding(&raw mut pem)? };
    Ok(normalize_pem(&take_pwstr(pem)) == normalize_pem(expected_pem))
}

fn normalize_pem(pem: &str) -> String {
    pem.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_local_frontend_url(uri: &str) -> bool {
    uri.starts_with("https://localhost:") || uri.starts_with("https://127.0.0.1:")
}

fn is_local_frontend_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1"
}
