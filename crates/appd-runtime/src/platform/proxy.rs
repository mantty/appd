//! `WebKit` proxy configuration shared by Apple shells.

use std::ffi::{CString, c_char, c_void};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

type NwEndpointRef = *mut c_void;
type NwProxyConfigRef = *mut c_void;

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_endpoint_create_host(host: *const c_char, port: *const c_char) -> NwEndpointRef;
    fn nw_proxy_config_create_http_connect(
        endpoint: NwEndpointRef,
        tls_options: *mut c_void,
    ) -> NwProxyConfigRef;
    fn nw_proxy_config_add_match_domain(config: NwProxyConfigRef, domain: *const c_char);
    fn nw_proxy_config_set_failover_allowed(config: NwProxyConfigRef, allowed: bool);
    fn nw_release(object: *mut c_void);
}

/// Configure an HTTP CONNECT proxy for the app origin on a `WebKit` data store.
pub(super) fn configure_webview_proxy(data_store: &AnyObject, port: u16, app_host: &str) {
    let Ok(port) = CString::new(port.to_string()) else {
        return;
    };
    let Ok(host) = CString::new(app_host) else {
        return;
    };
    let Ok(proxy_host) = CString::new("127.0.0.1") else {
        return;
    };
    let endpoint = unsafe { nw_endpoint_create_host(proxy_host.as_ptr(), port.as_ptr()) };
    if endpoint.is_null() {
        return;
    }
    let proxy = unsafe { nw_proxy_config_create_http_connect(endpoint, std::ptr::null_mut()) };
    if proxy.is_null() {
        unsafe { nw_release(endpoint.cast()) };
        return;
    }
    unsafe {
        nw_proxy_config_set_failover_allowed(proxy, false);
        nw_proxy_config_add_match_domain(proxy, host.as_ptr());
        let array: Retained<AnyObject> =
            msg_send![class!(NSArray), arrayWithObject: proxy.cast::<AnyObject>()];
        let _: () = msg_send![data_store, setProxyConfigurations: &*array];
        nw_release(proxy.cast());
        nw_release(endpoint.cast());
    }
}
