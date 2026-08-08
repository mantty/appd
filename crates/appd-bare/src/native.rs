use std::ffi::{c_char, c_int, c_void};
use std::ptr::NonNull;
#[cfg(not(target_os = "android"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(not(target_os = "android"))]
use std::time::Duration;

use super::{Error, Result, parse_startup_reply};

const IPC_WOULD_BLOCK: c_int = -1;
const REPLY_CAPACITY: usize = 1024;
#[cfg(not(target_os = "android"))]
const IPC_READABLE: c_int = 0x1;
#[cfg(not(target_os = "android"))]
const IPC_WRITABLE: c_int = 0x2;
#[cfg(target_os = "android")]
const STARTUP_TIMEOUT_MS: c_int = 30_000;
#[cfg(not(target_os = "android"))]
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

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
        ipc.write_all(&startup_message(config))?;
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
        #[cfg(target_os = "windows")]
        if bundle.len() > u32::MAX as usize {
            return Err(Error::Startup(
                "worker bundle exceeds Windows libuv limits".to_owned(),
            ));
        }
        let source = UvBuf::from_bytes(bundle);
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
    #[cfg(not(target_os = "android"))]
    poll: NonNull<BareIpcPoll>,
    #[cfg(not(target_os = "android"))]
    state: Arc<PollState>,
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
        #[cfg(target_os = "android")]
        return Ok(Self { handle });

        #[cfg(not(target_os = "android"))]
        {
            let mut poll = std::ptr::null_mut();
            if let Err(error) = status(
                unsafe { bare_ipc_poll_alloc(&raw mut poll) },
                "IPC poll allocation failed",
            ) {
                unsafe {
                    bare_ipc_destroy(handle.as_ptr());
                    libc::free(handle.as_ptr().cast());
                }
                return Err(error);
            }
            let Some(poll) = NonNull::new(poll) else {
                unsafe {
                    bare_ipc_destroy(handle.as_ptr());
                    libc::free(handle.as_ptr().cast());
                }
                return Err(native_error(-1, "IPC poll allocation returned null"));
            };
            if let Err(error) = status(
                unsafe { bare_ipc_poll_init(poll.as_ptr(), handle.as_ptr()) },
                "IPC poll initialization failed",
            ) {
                unsafe {
                    libc::free(poll.as_ptr().cast());
                    bare_ipc_destroy(handle.as_ptr());
                    libc::free(handle.as_ptr().cast());
                }
                return Err(error);
            }
            let state = Arc::new(PollState::default());
            let data = Arc::into_raw(Arc::clone(&state)).cast_mut().cast();
            unsafe { bare_ipc_poll_set_data(poll.as_ptr(), data) };
            Ok(Self {
                handle,
                poll,
                state,
            })
        }
    }

    fn write_all(&self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let written =
                unsafe { bare_ipc_write(self.handle.as_ptr(), data.as_ptr().cast(), data.len()) };
            if written == IPC_WOULD_BLOCK {
                self.wait_writable()?;
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
                self.wait_readable()?;
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

    #[cfg(not(target_os = "android"))]
    fn wait(&self, wanted: c_int) -> Result<()> {
        let mut flags = self
            .state
            .flags
            .lock()
            .map_err(|_| Error::Startup("IPC poll state is unavailable".to_owned()))?;
        *flags &= !wanted;
        if let Err(error) = status(
            unsafe { bare_ipc_poll_start(self.poll.as_ptr(), wanted, Some(notify)) },
            "IPC poll startup failed",
        ) {
            unsafe { bare_ipc_poll_stop(self.poll.as_ptr()) };
            return Err(error);
        }
        let wait_result = self
            .state
            .changed
            .wait_timeout_while(flags, STARTUP_TIMEOUT, |flags| *flags & wanted == 0)
            .map_err(|_| Error::Startup("IPC poll state is unavailable".to_owned()));
        let stopped = status(
            unsafe { bare_ipc_poll_stop(self.poll.as_ptr()) },
            "IPC poll shutdown failed",
        );
        let (mut flags, timeout) = wait_result?;
        stopped?;
        if timeout.timed_out() {
            return Err(Error::Startup("timed out".to_owned()));
        }
        *flags &= !wanted;
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    fn wait_readable(&self) -> Result<()> {
        self.wait(IPC_READABLE)
    }

    #[cfg(not(target_os = "android"))]
    fn wait_writable(&self) -> Result<()> {
        self.wait(IPC_WRITABLE)
    }

    #[cfg(target_os = "android")]
    fn wait_readable(&self) -> Result<()> {
        Self::wait_fd(
            unsafe { bare_ipc_get_incoming(self.handle.as_ptr()) },
            libc::POLLIN,
        )
    }

    #[cfg(target_os = "android")]
    fn wait_writable(&self) -> Result<()> {
        Self::wait_fd(
            unsafe { bare_ipc_get_outgoing(self.handle.as_ptr()) },
            libc::POLLOUT,
        )
    }

    #[cfg(target_os = "android")]
    fn wait_fd(descriptor: c_int, events: i16) -> Result<()> {
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
        #[cfg(not(target_os = "android"))]
        unsafe {
            let data = bare_ipc_poll_get_data(self.poll.as_ptr()).cast::<PollState>();
            bare_ipc_poll_set_data(self.poll.as_ptr(), std::ptr::null_mut());
            bare_ipc_poll_stop(self.poll.as_ptr());
            bare_ipc_poll_destroy(self.poll.as_ptr());
            Arc::decrement_strong_count(data);
            libc::free(self.poll.as_ptr().cast());
        }
        unsafe {
            bare_ipc_destroy(self.handle.as_ptr());
            libc::free(self.handle.as_ptr().cast());
        }
    }
}

#[cfg(not(target_os = "android"))]
#[derive(Default)]
struct PollState {
    flags: Mutex<c_int>,
    changed: Condvar,
}

#[cfg(not(target_os = "android"))]
extern "C" fn notify(poll: *mut BareIpcPoll, events: c_int) {
    let state = unsafe { bare_ipc_poll_get_data(poll).cast::<PollState>().as_ref() };
    let Some(state) = state else {
        return;
    };
    let mut flags = match state.flags.lock() {
        Ok(flags) => flags,
        Err(error) => error.into_inner(),
    };
    *flags |= events;
    state.changed.notify_all();
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

fn startup_message(config: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(config.len() + 1);
    message.extend_from_slice(config);
    message.push(b'\n');
    message
}

#[repr(C)]
struct BareWorkletOptions {
    memory_limit: usize,
    assets: *const c_char,
}

#[repr(C)]
#[cfg(target_os = "windows")]
struct UvBuf {
    len: u32,
    base: *mut c_char,
}

#[repr(C)]
#[cfg(not(target_os = "windows"))]
struct UvBuf {
    base: *mut c_char,
    len: usize,
}

impl UvBuf {
    fn from_bytes(bytes: &[u8]) -> Self {
        #[cfg(target_os = "windows")]
        let len = bytes.len() as u32;
        #[cfg(not(target_os = "windows"))]
        let len = bytes.len();

        Self {
            #[cfg(target_os = "windows")]
            len,
            base: bytes.as_ptr().cast_mut().cast(),
            #[cfg(not(target_os = "windows"))]
            len,
        }
    }
}

#[cfg(test)]
mod buffer_tests {
    use super::{UvBuf, startup_message};

    #[test]
    fn preserves_the_worker_bundle_slice() {
        let bytes = [1_u8, 2, 3];
        let buffer = UvBuf::from_bytes(&bytes);

        assert_eq!(buffer.base.cast_const().cast::<u8>(), bytes.as_ptr());
        #[cfg(target_os = "windows")]
        assert_eq!(buffer.len, 3);
        #[cfg(not(target_os = "windows"))]
        assert_eq!(buffer.len, 3_usize);
    }

    #[test]
    fn frames_startup_configuration_as_one_line() {
        assert_eq!(startup_message(br#"{"port":0}"#), b"{\"port\":0}\n");
    }
}

enum BareWorklet {}
enum BareIpc {}
#[cfg(not(target_os = "android"))]
enum BareIpcPoll {}

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
    #[cfg(target_os = "android")]
    fn bare_ipc_get_incoming(ipc: *mut BareIpc) -> c_int;
    #[cfg(target_os = "android")]
    fn bare_ipc_get_outgoing(ipc: *mut BareIpc) -> c_int;
    fn bare_ipc_read(ipc: *mut BareIpc, data: *mut *mut c_void, len: *mut usize) -> c_int;
    fn bare_ipc_write(ipc: *mut BareIpc, data: *const c_void, len: usize) -> c_int;
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_alloc(result: *mut *mut BareIpcPoll) -> c_int;
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_init(poll: *mut BareIpcPoll, ipc: *mut BareIpc) -> c_int;
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_destroy(poll: *mut BareIpcPoll);
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_get_data(poll: *mut BareIpcPoll) -> *mut c_void;
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_set_data(poll: *mut BareIpcPoll, data: *mut c_void);
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_start(
        poll: *mut BareIpcPoll,
        events: c_int,
        callback: Option<extern "C" fn(*mut BareIpcPoll, c_int)>,
    ) -> c_int;
    #[cfg(not(target_os = "android"))]
    fn bare_ipc_poll_stop(poll: *mut BareIpcPoll) -> c_int;
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
