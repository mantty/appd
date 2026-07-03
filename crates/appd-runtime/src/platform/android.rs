//! Android JNI runtime entrypoints.

use std::ffi::c_char;
use std::path::PathBuf;
use std::ptr;
use std::sync::{Mutex, Once, OnceLock};
use std::thread;

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jbyteArray, jint, jstring};

use crate::certs::CertificateBundle;
#[cfg(feature = "workerd-ffi")]
use crate::host::StartedRuntime;

static LOGCAT_REDIRECT: Once = Once::new();
static CERTIFICATES: OnceLock<Mutex<Option<CertificateBundle>>> = OnceLock::new();
#[cfg(feature = "workerd-ffi")]
static RUNTIME: OnceLock<Mutex<Option<StartedRuntime>>> = OnceLock::new();

const ANDROID_LOG_INFO: libc::c_int = 4;
const LOG_TAG: &[u8] = b"appd.runtime\0";

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(
        priority: libc::c_int,
        tag: *const c_char,
        text: *const c_char,
    ) -> libc::c_int;
}

/// Android uses Java/Kotlin to own the Activity and `WebView` lifecycle.
pub fn run() -> ! {
    panic!("Android starts appd-runtime through JNI nativeStart");
}

/// Start the native runtime and return the loopback backend port.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_Runtime_nativeStart(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    work_dir: JString<'_>,
) -> jint {
    LOGCAT_REDIRECT.call_once(redirect_stdout_and_stderr_to_logcat);

    let Ok(work_dir) = env.get_string(&work_dir) else {
        return -1;
    };
    let work_dir = work_dir.to_string_lossy().into_owned();

    match start_runtime(PathBuf::from(work_dir)) {
        Some(port) => i32::from(port),
        None => -1,
    }
}

/// Return the client PKCS#12 bundle for Java `WebView` mTLS hooks.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_Runtime_nativeGetClientP12(
    env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jbyteArray {
    byte_array(env, |certs| &certs.client_p12_der)
}

/// Return the PKCS#12 password for Java `WebView` mTLS hooks.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_Runtime_nativeGetP12Password(
    env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jstring {
    let Some(password) = with_certificates(|certs| certs.p12_password.clone()) else {
        return ptr::null_mut();
    };

    match env.new_string(password) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Return the runtime CA certificate for Java `WebView` trust validation.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_appd_runtime_Runtime_nativeGetCaDer(
    env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jbyteArray {
    byte_array(env, |certs| &certs.ca_cert_der)
}

#[cfg(feature = "workerd-ffi")]
fn start_runtime(work_dir: PathBuf) -> Option<u16> {
    let lock = RUNTIME.get_or_init(|| Mutex::new(None));
    let Ok(mut runtime) = lock.lock() else {
        return None;
    };

    if let Some(existing) = runtime.as_ref() {
        return Some(existing.port);
    }

    let Ok(started) = crate::host::start_workerd_bridge(&work_dir) else {
        return None;
    };
    store_certificates(started.certificates.clone());
    let port = started.port;
    *runtime = Some(started);
    Some(port)
}

#[cfg(not(feature = "workerd-ffi"))]
fn start_runtime(_: PathBuf) -> Option<u16> {
    None
}

fn store_certificates(certs: CertificateBundle) {
    if let Ok(mut guard) = CERTIFICATES.get_or_init(|| Mutex::new(None)).lock() {
        *guard = Some(certs);
    }
}

fn with_certificates<T>(read: impl FnOnce(&CertificateBundle) -> T) -> Option<T> {
    CERTIFICATES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(read))
}

fn byte_array(env: JNIEnv<'_>, read: impl FnOnce(&CertificateBundle) -> &[u8]) -> jbyteArray {
    let Some(bytes) = with_certificates(|certs| read(certs).to_vec()) else {
        return ptr::null_mut();
    };

    match env.byte_array_from_slice(&bytes) {
        Ok(value) => JByteArray::into_raw(value),
        Err(_) => ptr::null_mut(),
    }
}

fn redirect_stdout_and_stderr_to_logcat() {
    let mut fds = [0; 2];
    let pipe_result = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if pipe_result != 0 {
        return;
    }

    unsafe {
        let _ = libc::dup2(fds[1], libc::STDOUT_FILENO);
        let _ = libc::dup2(fds[1], libc::STDERR_FILENO);
        let _ = libc::close(fds[1]);
    }

    let read_fd = fds[0];
    let _ = thread::Builder::new()
        .name("appd-logcat".to_owned())
        .spawn(move || read_pipe_to_logcat(read_fd));
}

fn read_pipe_to_logcat(read_fd: libc::c_int) {
    let mut buffer = [0_u8; 4096];
    let mut pending = Vec::with_capacity(4096);

    loop {
        let count = unsafe {
            libc::read(
                read_fd,
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        if count <= 0 {
            break;
        }

        for byte in &buffer[..usize::try_from(count).unwrap_or_default()] {
            if *byte == b'\n' {
                write_logcat_line(&pending);
                pending.clear();
            } else if *byte != 0 {
                pending.push(*byte);
            }
        }
    }

    if !pending.is_empty() {
        write_logcat_line(&pending);
    }
    unsafe {
        let _ = libc::close(read_fd);
    }
}

fn write_logcat_line(line: &[u8]) {
    let mut nul_terminated = Vec::with_capacity(line.len() + 1);
    nul_terminated.extend_from_slice(line);
    nul_terminated.push(0);
    unsafe {
        let _ = __android_log_write(
            ANDROID_LOG_INFO,
            LOG_TAG.as_ptr().cast::<c_char>(),
            nul_terminated.as_ptr().cast::<c_char>(),
        );
    }
}
