//! Android shell built on Tao and WRY.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use jni::objects::{GlobalRef, JClass, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

use crate::certs::{CertificateBundle, SharedCertificateBundle};
use crate::host::{StartedRuntime, start_bare_runtime};

struct AndroidRuntime {
    certificates: SharedCertificateBundle,
    runtime: StartedRuntime,
}

struct AndroidContext {
    activity: GlobalRef,
    vm: JavaVM,
}

static RUNTIME: OnceLock<Mutex<Option<AndroidRuntime>>> = OnceLock::new();
static CONTEXT: OnceLock<AndroidContext> = OnceLock::new();
static EVENT_PROXY: OnceLock<EventLoopProxy<()>> = OnceLock::new();

/// Register the WRY JNI entry points and keep the Android process alive.
pub fn run() {
    tao::android_binding!(com_appd, runtime, Rust, setup_android, start_app, tao);
    wry::android_binding!(com_appd, runtime, wry);
    loop {
        std::thread::park();
    }
}

unsafe fn setup_android(
    package: &str,
    env: JNIEnv,
    looper: &wry::prelude::ndk::looper::ThreadLooper,
    activity: GlobalRef,
) {
    let vm = env
        .get_java_vm()
        .unwrap_or_else(|error| panic!("appd Android JNI startup failed: {error}"));
    let context = AndroidContext {
        activity: activity.clone(),
        vm,
    };
    unsafe { wry::android_setup(package, env, looper, activity) };
    let _ = CONTEXT.set(context);
}

fn start_app() {
    let _ = CONTEXT.wait();
    let event_loop = EventLoopBuilder::<()>::with_user_event().build();
    let _ = EVENT_PROXY.set(event_loop.create_proxy());
    let mut app = None;
    let mut loaded = false;
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        if app.is_none() {
            app = Some(create_webview(target));
        }
        match event {
            Event::Suspended => suspend_runtime(),
            Event::Resumed => resume_runtime(),
            Event::UserEvent(()) if !loaded => {
                if let Some((_, webview)) = app.as_ref()
                    && let Some(url) = frontend_url()
                    && webview.load_url(&url).is_ok()
                {
                    loaded = true;
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    })
}

fn create_webview(
    target: &tao::event_loop::EventLoopWindowTarget<()>,
) -> (tao::window::Window, wry::WebView) {
    let window = WindowBuilder::new()
        .build(target)
        .unwrap_or_else(|error| panic!("appd Android window startup failed: {error}"));
    let webview = WebViewBuilder::new()
        .with_html(loading_page())
        .build(&window)
        .unwrap_or_else(|error| panic!("appd Android WebView construction failed: {error}"));
    if let Err(error) = start_runtime() {
        eprintln!("appd Android runtime startup failed: {error}");
        let _ = webview.load_html("<h1>App failed to start</h1>");
    }
    (window, webview)
}

fn start_runtime() -> Result<(), String> {
    let context = CONTEXT
        .get()
        .ok_or_else(|| "Android runtime context is unavailable".to_owned())?;
    let mut env = context
        .vm
        .attach_current_thread_as_daemon()
        .map_err(|error| error.to_string())?;
    start_runtime_with_env(&mut env, context.activity.as_obj())
}

fn start_runtime_with_env<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject<'local>,
) -> Result<(), String> {
    let host = app_host(env, activity)?;
    let root = runtime_root(env, activity)?;
    let certificates = Arc::new(RwLock::new(None));
    let runtime = start_bare_runtime(
        root.join("app"),
        root.join("state"),
        &host,
        certificates.clone(),
    )
    .map_err(|error| error.to_string())?;
    let port = runtime.port;
    configure_proxy(env, activity, &host, port)?;
    let state = RUNTIME.get_or_init(|| Mutex::new(None));
    let mut state = state
        .lock()
        .map_err(|_| "Android runtime state is unavailable".to_owned())?;
    *state = Some(AndroidRuntime {
        certificates,
        runtime,
    });
    Ok(())
}

fn app_host(env: &mut JNIEnv, activity: &JObject) -> Result<String, String> {
    let package_manager = env
        .call_method(
            activity,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let package = env
        .call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let info = env
        .call_method(
            &package_manager,
            "getApplicationInfo",
            "(Ljava/lang/String;I)Landroid/content/pm/ApplicationInfo;",
            &[(&package).into(), 128.into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let metadata = env
        .get_field(&info, "metaData", "Landroid/os/Bundle;")
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let key = env
        .new_string("appd.host")
        .map_err(|error| error.to_string())?;
    let host = env
        .call_method(
            &metadata,
            "getString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[(&key).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    if host.is_null() {
        return Err("appd.host is required".to_owned());
    }
    java_string(env, host)
}

fn runtime_root(env: &mut JNIEnv, activity: &JObject) -> Result<PathBuf, String> {
    let files = env
        .call_method(activity, "getFilesDir", "()Ljava/io/File;", &[])
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let path = env
        .call_method(&files, "getPath", "()Ljava/lang/String;", &[])
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let root = PathBuf::from(java_string(env, path)?).join("appd");
    let app = root.join("app");
    if app.exists() {
        fs::remove_dir_all(&app).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    copy_assets(env, activity, "app", &app)?;
    Ok(root)
}

fn copy_assets(
    env: &mut JNIEnv,
    activity: &JObject,
    source: &str,
    destination: &Path,
) -> Result<(), String> {
    let assets = env
        .call_method(
            activity,
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    copy_asset_tree(env, &assets, source, destination)
}

fn copy_asset_tree(
    env: &mut JNIEnv,
    assets: &JObject,
    source: &str,
    destination: &Path,
) -> Result<(), String> {
    let source_java = env.new_string(source).map_err(|error| error.to_string())?;
    let entries = env
        .call_method(
            assets,
            "list",
            "(Ljava/lang/String;)[Ljava/lang/String;",
            &[(&source_java).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let entries = JObjectArray::from(entries);
    let count = env
        .get_array_length(&entries)
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return copy_asset_file(env, assets, source, destination);
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for index in 0..count {
        let entry = env
            .get_object_array_element(&entries, index)
            .map_err(|error| error.to_string())?;
        let entry = java_string(env, entry)?;
        copy_asset_tree(
            env,
            assets,
            &format!("{source}/{entry}"),
            &destination.join(entry),
        )?;
    }
    Ok(())
}

fn copy_asset_file(
    env: &mut JNIEnv,
    assets: &JObject,
    source: &str,
    destination: &Path,
) -> Result<(), String> {
    let source_java = env.new_string(source).map_err(|error| error.to_string())?;
    let stream = env
        .call_method(
            assets,
            "open",
            "(Ljava/lang/String;)Ljava/io/InputStream;",
            &[(&source_java).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let parent = destination
        .parent()
        .ok_or_else(|| "asset destination is missing a parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut output = fs::File::create(destination).map_err(|error| error.to_string())?;
    let buffer = env
        .new_byte_array(8_192)
        .map_err(|error| error.to_string())?;
    loop {
        let count = env
            .call_method(&stream, "read", "([B)I", &[(&buffer).into()])
            .map_err(|error| error.to_string())?
            .i()
            .map_err(|error| error.to_string())?;
        if count < 0 {
            break;
        }
        let count = usize::try_from(count).map_err(|error| error.to_string())?;
        let bytes = env
            .convert_byte_array(&buffer)
            .map_err(|error| error.to_string())?;
        output
            .write_all(&bytes[..count])
            .map_err(|error| error.to_string())?;
    }
    let _ = env.call_method(&stream, "close", "()V", &[]);
    Ok(())
}

fn configure_proxy<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject<'local>,
    host: &str,
    port: u16,
) -> Result<(), String> {
    if !proxy_feature_supported(env, activity, "PROXY_OVERRIDE")?
        || !proxy_feature_supported(env, activity, "PROXY_OVERRIDE_REVERSE_BYPASS")?
    {
        return Err("this WebView does not support appd's secure origin".to_owned());
    }
    let builder_class = app_class(env, activity, "androidx.webkit.ProxyConfig$Builder")?;
    let builder = env
        .new_object(&builder_class, "()V", &[])
        .map_err(|error| format!("creating the proxy configuration: {error}"))?;
    let rule = env
        .new_string(format!("http://127.0.0.1:{port}"))
        .map_err(|error| format!("creating the proxy rule: {error}"))?;
    env.call_method(
        &builder,
        "addProxyRule",
        "(Ljava/lang/String;)Landroidx/webkit/ProxyConfig$Builder;",
        &[(&rule).into()],
    )
    .map_err(|error| format!("adding the proxy rule: {error}"))?;
    let host = env
        .new_string(host)
        .map_err(|error| format!("creating the proxy bypass rule: {error}"))?;
    env.call_method(
        &builder,
        "addBypassRule",
        "(Ljava/lang/String;)Landroidx/webkit/ProxyConfig$Builder;",
        &[(&host).into()],
    )
    .map_err(|error| format!("adding the proxy bypass rule: {error}"))?;
    env.call_method(
        &builder,
        "setReverseBypassEnabled",
        "(Z)Landroidx/webkit/ProxyConfig$Builder;",
        &[JValue::Bool(1)],
    )
    .map_err(|error| format!("enabling reverse proxy bypass: {error}"))?;
    let config = env
        .call_method(&builder, "build", "()Landroidx/webkit/ProxyConfig;", &[])
        .map_err(|error| format!("building the proxy configuration: {error}"))?
        .l()
        .map_err(|error| format!("reading the proxy configuration: {error}"))?;
    let context = env
        .call_method(
            activity,
            "getApplicationContext",
            "()Landroid/content/Context;",
            &[],
        )
        .map_err(|error| format!("getting the application context: {error}"))?
        .l()
        .map_err(|error| format!("reading the application context: {error}"))?;
    let executor = env
        .call_method(
            &context,
            "getMainExecutor",
            "()Ljava/util/concurrent/Executor;",
            &[],
        )
        .map_err(|error| format!("getting the main executor: {error}"))?
        .l()
        .map_err(|error| format!("reading the main executor: {error}"))?;
    let controller_class = app_class(env, activity, "androidx.webkit.ProxyController")?;
    let controller = env
        .call_static_method(
            &controller_class,
            "getInstance",
            "()Landroidx/webkit/ProxyController;",
            &[],
        )
        .map_err(|error| format!("getting the proxy controller: {error}"))?
        .l()
        .map_err(|error| format!("reading the proxy controller: {error}"))?;
    env.call_method(
        &controller,
        "setProxyOverride",
        "(Landroidx/webkit/ProxyConfig;Ljava/util/concurrent/Executor;Ljava/lang/Runnable;)V",
        &[(&config).into(), (&executor).into(), activity.into()],
    )
    .map_err(|error| format!("installing the proxy configuration: {error}"))?;
    Ok(())
}

fn proxy_feature_supported<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject<'local>,
    feature: &str,
) -> Result<bool, String> {
    let class = app_class(env, activity, "androidx.webkit.WebViewFeature")?;
    let name = feature;
    let feature = env
        .new_string(feature)
        .map_err(|error| format!("creating feature name {name}: {error}"))?;
    env.call_static_method(
        &class,
        "isFeatureSupported",
        "(Ljava/lang/String;)Z",
        &[(&feature).into()],
    )
    .map_err(|error| format!("checking feature {name}: {error}"))?
    .z()
    .map_err(|error| format!("reading feature {name}: {error}"))
}

fn app_class<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject<'local>,
    name: &str,
) -> Result<JClass<'local>, String> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .map_err(|error| format!("getting the app class loader: {error}"))?
        .l()
        .map_err(|error| format!("reading the app class loader: {error}"))?;
    let name = env
        .new_string(name)
        .map_err(|error| format!("creating the app class name: {error}"))?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )
        .map_err(|error| format!("loading the app class: {error}"))?
        .l()
        .map_err(|error| format!("reading the app class: {error}"))?;
    Ok(JClass::from(class))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_AppdActivity_run(_: JNIEnv, _: JObject) {
    if let Some(proxy) = EVENT_PROXY.get() {
        let _ = proxy.send_event(());
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_RustWebViewClient_appdClientCertificate(
    mut env: JNIEnv,
    _: JObject,
    request: JObject,
) {
    let result = certificates()
        .and_then(|certificates| client_identity(&mut env, &certificates).ok())
        .and_then(|(key, certificate)| {
            let class = env.find_class("java/security/cert/X509Certificate").ok()?;
            let chain = env.new_object_array(1, class, JObject::null()).ok()?;
            env.set_object_array_element(&chain, 0, certificate).ok()?;
            env.call_method(
                &request,
                "proceed",
                "(Ljava/security/PrivateKey;[Ljava/security/cert/X509Certificate;)V",
                &[(&key).into(), (&chain).into()],
            )
            .ok()
        });
    if result.is_none() {
        cancel_client_certificate(&mut env, &request);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_RustWebViewClient_appdServerTrust(
    mut env: JNIEnv,
    _: JObject,
    handler: JObject,
    error: JObject,
) {
    let trusted = certificates().is_some_and(|certificates| {
        server_certificate_is_trusted(&mut env, &error, &certificates).is_ok()
    });
    let method = if trusted { "proceed" } else { "cancel" };
    let _ = env.call_method(&handler, method, "()V", &[]);
}

fn client_identity<'local>(
    env: &mut JNIEnv<'local>,
    certificates: &CertificateBundle,
) -> Result<(JObject<'local>, JObject<'local>), String> {
    let key_bytes = env
        .byte_array_from_slice(&certificates.client_key_der)
        .map_err(|error| error.to_string())?;
    let key_spec = env
        .new_object(
            "java/security/spec/PKCS8EncodedKeySpec",
            "([B)V",
            &[(&key_bytes).into()],
        )
        .map_err(|error| error.to_string())?;
    let algorithm = env.new_string("EC").map_err(|error| error.to_string())?;
    let factory = env
        .call_static_method(
            "java/security/KeyFactory",
            "getInstance",
            "(Ljava/lang/String;)Ljava/security/KeyFactory;",
            &[(&algorithm).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let key = env
        .call_method(
            &factory,
            "generatePrivate",
            "(Ljava/security/spec/KeySpec;)Ljava/security/PrivateKey;",
            &[(&key_spec).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let certificate = pem_certificate(&certificates.client_cert_pem)
        .ok_or_else(|| "client certificate is invalid".to_owned())?;
    Ok((key, certificate_from_der(env, &certificate)?))
}

fn server_certificate_is_trusted(
    env: &mut JNIEnv,
    error: &JObject,
    certificates: &CertificateBundle,
) -> Result<(), String> {
    let host = app_host_from_runtime().ok_or_else(|| "runtime is unavailable".to_owned())?;
    let url = env
        .call_method(error, "getUrl", "()Ljava/lang/String;", &[])
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[(&url).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let received_host = env
        .call_method(&uri, "getHost", "()Ljava/lang/String;", &[])
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    if received_host.is_null() || !java_string(env, received_host)?.eq_ignore_ascii_case(&host) {
        return Err("server certificate was requested for a different host".to_owned());
    }
    let untrusted = env
        .get_static_field("android/net/http/SslError", "SSL_UNTRUSTED", "I")
        .map_err(|error| error.to_string())?
        .i()
        .map_err(|error| error.to_string())?;
    let primary_error = env
        .call_method(error, "getPrimaryError", "()I", &[])
        .map_err(|error| error.to_string())?
        .i()
        .map_err(|error| error.to_string())?;
    if primary_error != untrusted {
        return Err("server certificate error is not a private CA challenge".to_owned());
    }
    let certificate = env
        .call_method(
            error,
            "getCertificate",
            "()Landroid/net/http/SslCertificate;",
            &[],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let certificate = env
        .call_method(
            &certificate,
            "getX509Certificate",
            "()Ljava/security/cert/X509Certificate;",
            &[],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    env.call_method(&certificate, "checkValidity", "()V", &[])
        .map_err(|error| error.to_string())?;
    let authority = certificate_from_der(env, &certificates.ca_cert_der)?;
    let key = env
        .call_method(
            &authority,
            "getPublicKey",
            "()Ljava/security/PublicKey;",
            &[],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    env.call_method(
        &certificate,
        "verify",
        "(Ljava/security/PublicKey;)V",
        &[(&key).into()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn certificate_from_der<'local>(
    env: &mut JNIEnv<'local>,
    der: &[u8],
) -> Result<JObject<'local>, String> {
    let algorithm = env.new_string("X.509").map_err(|error| error.to_string())?;
    let factory = env
        .call_static_method(
            "java/security/cert/CertificateFactory",
            "getInstance",
            "(Ljava/lang/String;)Ljava/security/cert/CertificateFactory;",
            &[(&algorithm).into()],
        )
        .map_err(|error| error.to_string())?
        .l()
        .map_err(|error| error.to_string())?;
    let bytes = env
        .byte_array_from_slice(der)
        .map_err(|error| error.to_string())?;
    let stream = env
        .new_object("java/io/ByteArrayInputStream", "([B)V", &[(&bytes).into()])
        .map_err(|error| error.to_string())?;
    env.call_method(
        &factory,
        "generateCertificate",
        "(Ljava/io/InputStream;)Ljava/security/cert/Certificate;",
        &[(&stream).into()],
    )
    .map_err(|error| error.to_string())?
    .l()
    .map_err(|error| error.to_string())
}

fn suspend_runtime() {
    let Some(state) = RUNTIME.get() else {
        return;
    };
    if let Ok(state) = state.lock()
        && let Some(runtime) = state.as_ref()
    {
        let _ = runtime.runtime.suspend();
    }
}

fn resume_runtime() {
    let Some(state) = RUNTIME.get() else {
        return;
    };
    if let Ok(state) = state.lock()
        && let Some(runtime) = state.as_ref()
    {
        let _ = runtime.runtime.resume();
    }
}

fn certificates() -> Option<CertificateBundle> {
    let state = RUNTIME.get()?.lock().ok()?;
    state.as_ref()?.certificates.read().ok()?.as_ref().cloned()
}

fn app_host_from_runtime() -> Option<String> {
    let state = RUNTIME.get()?.lock().ok()?;
    Some(state.as_ref()?.runtime.host.clone())
}

fn frontend_url() -> Option<String> {
    let state = RUNTIME.get()?.lock().ok()?;
    let runtime = state.as_ref()?;
    Some(crate::frontend_url(&runtime.runtime.host))
}

fn pem_certificate(pem: &str) -> Option<Vec<u8>> {
    x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .ok()
        .map(|(_, certificate)| certificate.contents)
}

fn java_string(env: &mut JNIEnv, value: JObject) -> Result<String, String> {
    let value = JString::from(value);
    env.get_string(&value)
        .map(|value| value.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

fn cancel_client_certificate(env: &mut JNIEnv, request: &JObject) {
    let _ = env.call_method(request, "cancel", "()V", &[]);
}

fn loading_page() -> &'static str {
    "<!doctype html><title>Starting appd</title>"
}
