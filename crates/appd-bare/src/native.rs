use std::ffi::{c_char, c_int, c_void};
use std::ptr::NonNull;

use super::{Error, Result, parse_startup_reply};

const IPC_WOULD_BLOCK: c_int = -1;
const REPLY_CAPACITY: usize = 1024;
const STARTUP_TIMEOUT_MS: c_int = 30_000;

pub(super) struct Runtime {
    _ipc: Ipc,
    worklet: Worklet,
    port: u16,
}

impl Runtime {
    pub(super) fn start(bundle: &[u8], config: &[u8]) -> Result<Self> {
        if bundle.is_empty() || config.is_empty() {
            return Err(Error::Startup("invalid startup arguments".to_owned()));
        }
        let mut worklet = Worklet::new()?;
        worklet.start(bundle)?;
        let ipc = Ipc::new(&worklet)?;
        ipc.write_all(config)?;
        ipc.write_all(b"\n")?;
        let port = parse_startup_reply(&ipc.read_line()?)?;
        Ok(Self {
            _ipc: ipc,
            worklet,
            port,
        })
    }

    pub(super) const fn port(&self) -> u16 {
        self.port
    }

    pub(super) fn suspend(&self, linger: i32) -> Result<()> {
        status(
            unsafe { bare_worklet_suspend(self.worklet.handle.as_ptr(), linger) },
            "lifecycle suspension failed",
        )
    }

    pub(super) fn resume(&self) -> Result<()> {
        status(
            unsafe { bare_worklet_resume(self.worklet.handle.as_ptr()) },
            "lifecycle resume failed",
        )
    }
}

struct Worklet {
    handle: NonNull<BareWorklet>,
    started: bool,
}

impl Worklet {
    fn new() -> Result<Self> {
        let mut handle = std::ptr::null_mut();
        status(
            unsafe { bare_worklet_alloc(&raw mut handle) },
            "worklet allocation failed",
        )?;
        let handle = NonNull::new(handle)
            .ok_or_else(|| native_error(-1, "worklet allocation returned null"))?;
        let options = BareWorkletOptions {
            memory_limit: 0,
            assets: std::ptr::null(),
        };
        let result = status(
            unsafe { bare_worklet_init(handle.as_ptr(), &raw const options) },
            "worklet initialization failed",
        );
        if let Err(error) = result {
            unsafe { libc::free(handle.as_ptr().cast()) };
            return Err(error);
        }
        Ok(Self {
            handle,
            started: false,
        })
    }

    fn start(&mut self, bundle: &[u8]) -> Result<()> {
        let source = UvBuf {
            base: bundle.as_ptr().cast_mut().cast(),
            len: bundle.len(),
        };
        status(
            unsafe {
                bare_worklet_start(
                    self.handle.as_ptr(),
                    c"appd.bundle".as_ptr(),
                    &raw const source,
                    0,
                    std::ptr::null(),
                )
            },
            "worklet failed to start",
        )?;
        self.started = true;
        Ok(())
    }
}

impl Drop for Worklet {
    fn drop(&mut self) {
        if self.started {
            unsafe { bare_worklet_terminate(self.handle.as_ptr()) };
        }
        unsafe {
            bare_worklet_destroy(self.handle.as_ptr());
            libc::free(self.handle.as_ptr().cast());
        }
    }
}

struct Ipc {
    handle: NonNull<BareIpc>,
}

impl Ipc {
    fn new(worklet: &Worklet) -> Result<Self> {
        let mut handle = std::ptr::null_mut();
        status(
            unsafe { bare_ipc_alloc(&raw mut handle) },
            "IPC allocation failed",
        )?;
        let handle =
            NonNull::new(handle).ok_or_else(|| native_error(-1, "IPC allocation returned null"))?;
        let result = status(
            unsafe { bare_ipc_init(handle.as_ptr(), worklet.handle.as_ptr()) },
            "IPC initialization failed",
        );
        if let Err(error) = result {
            unsafe { libc::free(handle.as_ptr().cast()) };
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn write_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let written =
                unsafe { bare_ipc_write(self.handle.as_ptr(), data.as_ptr().cast(), data.len()) };
            if written == IPC_WOULD_BLOCK {
                Self::wait(
                    unsafe { bare_ipc_get_outgoing(self.handle.as_ptr()) },
                    libc::POLLOUT,
                )?;
                continue;
            }
            if written <= 0 {
                return Err(native_error(written, "IPC write failed"));
            }
            let written = usize::try_from(written)
                .map_err(|_| native_error(written, "IPC returned an invalid write length"))?;
            if written > data.len() {
                return Err(native_error(-1, "IPC returned an invalid write length"));
            }
            data = &data[written..];
        }
        Ok(())
    }

    fn read_line(&self) -> Result<String> {
        let mut line = Vec::new();
        loop {
            let mut data = std::ptr::null_mut();
            let mut len = 0;
            let result =
                unsafe { bare_ipc_read(self.handle.as_ptr(), &raw mut data, &raw mut len) };
            if result == IPC_WOULD_BLOCK {
                Self::wait(
                    unsafe { bare_ipc_get_incoming(self.handle.as_ptr()) },
                    libc::POLLIN,
                )?;
                continue;
            }
            if result != 0 || len == 0 || data.is_null() {
                return Err(native_error(result, "worklet stopped during startup"));
            }
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
            let end = bytes.iter().position(|byte| *byte == b'\n').unwrap_or(len);
            if line.len() + end >= REPLY_CAPACITY {
                return Err(Error::Startup("reply exceeds 1023 bytes".to_owned()));
            }
            line.extend_from_slice(&bytes[..end]);
            if end != len {
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
        }
    }

    fn wait(descriptor: c_int, events: i16) -> Result<()> {
        let mut waiting = libc::pollfd {
            fd: descriptor,
            events,
            revents: 0,
        };
        loop {
            let result = unsafe { libc::poll(&raw mut waiting, 1, STARTUP_TIMEOUT_MS) };
            if result > 0 {
                return Ok(());
            }
            if result == 0 {
                return Err(Error::Startup("timed out".to_owned()));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(native_error(
                    -error.raw_os_error().unwrap_or(1),
                    "IPC polling failed",
                ));
            }
        }
    }
}

impl Drop for Ipc {
    fn drop(&mut self) {
        unsafe {
            bare_ipc_destroy(self.handle.as_ptr());
            libc::free(self.handle.as_ptr().cast());
        }
    }
}

fn status(status: c_int, message: &'static str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(native_error(status, message))
    }
}

fn native_error(status: c_int, message: &'static str) -> Error {
    Error::Native {
        status,
        message: message.to_owned(),
    }
}

#[repr(C)]
struct BareWorkletOptions {
    memory_limit: usize,
    assets: *const c_char,
}

#[repr(C)]
struct UvBuf {
    base: *mut c_char,
    len: usize,
}

enum BareWorklet {}
enum BareIpc {}

unsafe extern "C" {
    fn bare_worklet_alloc(result: *mut *mut BareWorklet) -> c_int;
    fn bare_worklet_init(worklet: *mut BareWorklet, options: *const BareWorkletOptions) -> c_int;
    fn bare_worklet_destroy(worklet: *mut BareWorklet);
    fn bare_worklet_start(
        worklet: *mut BareWorklet,
        filename: *const c_char,
        source: *const UvBuf,
        argc: c_int,
        argv: *const *const c_char,
    ) -> c_int;
    fn bare_worklet_suspend(worklet: *mut BareWorklet, linger: c_int) -> c_int;
    fn bare_worklet_resume(worklet: *mut BareWorklet) -> c_int;
    fn bare_worklet_terminate(worklet: *mut BareWorklet) -> c_int;

    fn bare_ipc_alloc(result: *mut *mut BareIpc) -> c_int;
    fn bare_ipc_init(ipc: *mut BareIpc, worklet: *mut BareWorklet) -> c_int;
    fn bare_ipc_destroy(ipc: *mut BareIpc);
    fn bare_ipc_get_incoming(ipc: *mut BareIpc) -> c_int;
    fn bare_ipc_get_outgoing(ipc: *mut BareIpc) -> c_int;
    fn bare_ipc_read(ipc: *mut BareIpc, data: *mut *mut c_void, len: *mut usize) -> c_int;
    fn bare_ipc_write(ipc: *mut BareIpc, data: *const c_void, len: usize) -> c_int;
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::Runtime;

    #[test]
    fn starts_and_controls_a_worklet_through_barekit() -> Result<(), Box<dyn std::error::Error>> {
        let bundle = include_bytes!("../tests/fixtures/native-smoke.bundle");
        let runtime = Runtime::start(bundle, b"{}")?;
        assert_eq!(runtime.port(), 8443);
        runtime.suspend(0)?;
        runtime.resume()?;
        Ok(())
    }
}
