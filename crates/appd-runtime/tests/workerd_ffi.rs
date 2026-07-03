#![cfg(feature = "workerd-ffi")]

use appd_runtime::RuntimeError;
use appd_runtime::workerd_ffi::WorkerdFfi;
use std::net::TcpListener;
#[cfg(feature = "workerd-test-stubs")]
use std::sync::{Mutex, MutexGuard};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[cfg(feature = "workerd-test-stubs")]
static WORKERD_STUB_LOCK: Mutex<()> = Mutex::new(());

#[cfg(feature = "workerd-test-stubs")]
#[test]
fn workerd_stub_reports_non_zero_exit_status() -> TestResult {
    let _guard = lock_workerd_stub()?;
    let temp_dir = tempfile::tempdir()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let handle = WorkerdFfi::start(
        temp_dir.path().join("config.capnp"),
        temp_dir.path(),
        listener,
    )?;
    let result = handle.join().map_err(|_| "workerd thread panicked")?;
    let Err(error) = result else {
        return Err("stubbed workerd should report startup failure".into());
    };

    assert!(error.to_string().contains("workerd exited with status -1"));
    Ok(())
}

#[cfg(feature = "workerd-test-stubs")]
#[test]
fn start_transfers_listener_to_workerd_stub() -> TestResult {
    use appd_runtime::workerd_ffi::{reset_stub_listener_socket, stub_listener_socket};

    let _guard = lock_workerd_stub()?;
    reset_stub_listener_socket();
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let socket_id = listener_socket_id(&listener)?;
    let temp_dir = tempfile::tempdir()?;

    let handle = WorkerdFfi::start(
        temp_dir.path().join("config.capnp"),
        temp_dir.path(),
        listener,
    )?;

    assert_ne!(socket_id, 0);
    assert_eq!(stub_listener_socket(), socket_id);
    let _ = handle.join().map_err(|_| "workerd thread panicked")?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_paths_that_cannot_cross_c_abi() -> TestResult {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    let invalid_utf8 = Path::new(OsStr::from_bytes(b"\xff"));
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let Err(invalid_utf8_error) = WorkerdFfi::start(invalid_utf8, Path::new("."), listener) else {
        panic!("invalid UTF-8 paths should fail before spawning workerd");
    };
    assert!(matches!(
        invalid_utf8_error,
        RuntimeError::InvalidUtf8Path(_)
    ));

    let interior_nul = Path::new(OsStr::from_bytes(b"bad\0path"));
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let Err(interior_nul_error) = WorkerdFfi::start(interior_nul, Path::new("."), listener) else {
        panic!("interior NUL paths should fail before spawning workerd");
    };
    assert!(matches!(interior_nul_error, RuntimeError::InteriorNul(_)));
    Ok(())
}

#[cfg(feature = "workerd-test-stubs")]
fn lock_workerd_stub() -> TestResult<MutexGuard<'static, ()>> {
    WORKERD_STUB_LOCK
        .lock()
        .map_err(|_| "workerd stub lock poisoned".into())
}

#[cfg(unix)]
fn listener_socket_id(listener: &TcpListener) -> TestResult<usize> {
    use std::os::fd::AsRawFd;

    Ok(usize::try_from(listener.as_raw_fd())?)
}

#[cfg(windows)]
fn listener_socket_id(listener: &TcpListener) -> TestResult<usize> {
    use std::os::windows::io::AsRawSocket;

    Ok(usize::try_from(listener.as_raw_socket())?)
}
