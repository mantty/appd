#[cfg(target_os = "android")]
use std::ffi::{CString, c_char, c_int};
use std::io::{self, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tokio::runtime::{Builder as TokioBuilder, Runtime as TokioRuntime};

use crate::quickjs::{Error, RuntimeConfig};

use crate::transport::{
    HttpRequest, HttpResponse, is_connect, is_websocket, read_headers, read_request, tls_acceptor,
    websocket_session, write_plain_response, write_response,
};

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

const MAX_WEBSOCKET_QUEUE: usize = 100;

pub(crate) struct Runtime {
    shared: Arc<Shared>,
    gateway: Option<JoinHandle<()>>,
    tokio: Option<TokioRuntime>,
}

pub(super) struct Shared {
    pub(super) handler: Arc<dyn Handler>,
    pub(super) config: RuntimeConfig,
    pub(super) tokio: tokio::runtime::Handle,
    pub(super) port: AtomicU16,
    pub(super) accepting: Arc<AtomicBool>,
    pub(super) lifecycle: Arc<Lifecycle>,
    pub(super) connections: Mutex<Vec<Arc<TcpStream>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LifecyclePhase {
    Running,
    Suspending,
    Suspended,
    Stopping,
}

struct LifecycleStatus {
    phase: LifecyclePhase,
    active: usize,
}

pub(super) struct Lifecycle {
    status: Mutex<LifecycleStatus>,
    changed: Condvar,
}

pub(super) struct Execution<'a> {
    lifecycle: &'a Lifecycle,
    accepting: &'a AtomicBool,
}

pub(super) trait Handler: Send + Sync {
    fn handle(
        &self,
        job: Job,
        execution: &Execution<'_>,
        accepting: &Arc<AtomicBool>,
    ) -> Result<(), Error>;
}

impl Lifecycle {
    pub(super) fn new() -> Self {
        Self {
            status: Mutex::new(LifecycleStatus {
                phase: LifecyclePhase::Running,
                active: 0,
            }),
            changed: Condvar::new(),
        }
    }

    pub(super) fn suspend(&self) {
        let mut status = lock_status(&self.status);
        if status.phase == LifecyclePhase::Running {
            status.phase = if status.active == 0 {
                LifecyclePhase::Suspended
            } else {
                LifecyclePhase::Suspending
            };
        }
        self.changed.notify_all();
    }

    pub(super) fn resume(&self) {
        let mut status = lock_status(&self.status);
        if status.phase != LifecyclePhase::Stopping {
            status.phase = LifecyclePhase::Running;
            self.changed.notify_all();
        }
    }

    pub(super) fn stop(&self) {
        let mut status = lock_status(&self.status);
        status.phase = LifecyclePhase::Stopping;
        self.changed.notify_all();
    }

    pub(super) fn enter<'a>(&'a self, accepting: &'a AtomicBool) -> Option<Execution<'a>> {
        let mut status = self.wait_for_running(lock_status(&self.status), accepting)?;
        status.active += 1;
        Some(Execution {
            lifecycle: self,
            accepting,
        })
    }

    fn wait_until_running(&self, accepting: &AtomicBool) -> bool {
        self.wait_for_running(lock_status(&self.status), accepting)
            .is_some()
    }

    fn wait_for_running<'a>(
        &self,
        mut status: MutexGuard<'a, LifecycleStatus>,
        accepting: &AtomicBool,
    ) -> Option<MutexGuard<'a, LifecycleStatus>> {
        if !accepting.load(Ordering::Acquire) {
            return None;
        }
        while status.phase != LifecyclePhase::Running {
            if status.phase == LifecyclePhase::Stopping || !accepting.load(Ordering::Acquire) {
                return None;
            }
            status = wait_for_change(&self.changed, status);
        }
        accepting.load(Ordering::Acquire).then_some(status)
    }
}

impl Execution<'_> {
    pub(super) fn is_running(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
            && lock_status(&self.lifecycle.status).phase == LifecyclePhase::Running
    }
}

impl Drop for Execution<'_> {
    fn drop(&mut self) {
        let mut status = lock_status(&self.lifecycle.status);
        status.active -= 1;
        if status.phase == LifecyclePhase::Suspending && status.active == 0 {
            status.phase = LifecyclePhase::Suspended;
        }
        self.lifecycle.changed.notify_all();
    }
}

fn lock_status(status: &Mutex<LifecycleStatus>) -> MutexGuard<'_, LifecycleStatus> {
    match status.lock() {
        Ok(status) => status,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_for_change<'a>(
    changed: &Condvar,
    status: MutexGuard<'a, LifecycleStatus>,
) -> MutexGuard<'a, LifecycleStatus> {
    match changed.wait(status) {
        Ok(status) => status,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(super) struct Job {
    pub(super) request: HttpRequest,
    pub(super) response: SyncSender<JobResponse>,
    pub(super) websocket: Option<WebSocketJob>,
}

pub(super) enum JobResponse {
    Http(HttpResponse),
    WebSocket,
}

pub(super) struct WebSocketJob {
    pub(super) incoming: Receiver<WebSocketInbound>,
    pub(super) outgoing: SyncSender<WebSocketOutbound>,
}

pub(super) struct WebSocketBridge {
    pub(super) incoming: SyncSender<WebSocketInbound>,
    pub(super) outgoing: Receiver<WebSocketOutbound>,
}

pub(super) enum WebSocketInbound {
    Message { binary: bool, payload: Vec<u8> },
    Close { code: u16, reason: String },
}

pub(super) enum WebSocketOutbound {
    Message { binary: bool, payload: Vec<u8> },
    Close { code: u16, reason: String },
    Ready,
}

impl Runtime {
    pub(crate) fn start(handler: Arc<dyn Handler>, config: RuntimeConfig) -> Result<Self, Error> {
        let listener = TcpListener::bind(("127.0.0.1", config.port))?;
        let port = listener.local_addr()?.port();
        let tokio_runtime = TokioBuilder::new_multi_thread()
            .thread_name("appd-tokio")
            .build()
            .map_err(|error| Error::Startup(format!("failed to start Tokio: {error}")))?;
        let shared = Arc::new(Shared {
            handler,
            config,
            tokio: tokio_runtime.handle().clone(),
            port: AtomicU16::new(port),
            accepting: Arc::new(AtomicBool::new(true)),
            lifecycle: Arc::new(Lifecycle::new()),
            connections: Mutex::new(Vec::new()),
        });
        let gateway_shared = Arc::clone(&shared);
        let gateway = thread::Builder::new()
            .name("appd-gateway".to_owned())
            .spawn(move || gateway_loop(&gateway_shared, listener))
            .map_err(|error| Error::Startup(format!("failed to start gateway thread: {error}")))?;
        Ok(Self {
            shared,
            gateway: Some(gateway),
            tokio: Some(tokio_runtime),
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.shared.port.load(Ordering::Acquire)
    }

    pub(crate) fn restore_gateway(&self) -> io::Result<u16> {
        wait_for_gateway(|| self.port())
    }

    pub(crate) fn suspend(&self) {
        self.shared.lifecycle.suspend();
        close_connections(&self.shared);
    }

    pub(crate) fn resume(&self) {
        self.shared.lifecycle.resume();
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.shared.lifecycle.stop();
        self.shared.accepting.store(false, Ordering::Release);
        close_connections(&self.shared);
        let _ = TcpStream::connect(("127.0.0.1", self.port()));
        if let Some(thread) = self.gateway.take() {
            let _ = thread.join();
        }
        if let Some(tokio) = self.tokio.take() {
            tokio.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

fn gateway_loop(shared: &Arc<Shared>, mut listener: TcpListener) {
    let mut connection_threads = Vec::new();
    let mut listener_error_reported = false;
    loop {
        reap_finished_connections(&mut connection_threads);
        if !shared.accepting.load(Ordering::Acquire) {
            break;
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if listener_was_closed(&error) => {
                let port = shared.port.load(Ordering::Acquire);
                eprintln!("gateway listener closed: {error}");
                match replace_closed_listener(listener, port) {
                    Ok((replacement, replacement_port)) => {
                        listener = replacement;
                        shared.port.store(replacement_port, Ordering::Release);
                        listener_error_reported = false;
                        eprintln!("appd gateway listening on 127.0.0.1:{replacement_port}");
                        continue;
                    }
                    Err(error) => {
                        eprintln!("gateway listener could not recover: {error}");
                        break;
                    }
                }
            }
            Err(error) => {
                if !listener_error_reported {
                    eprintln!("gateway listener failed: {error}");
                    listener_error_reported = true;
                }
                continue;
            }
        };
        listener_error_reported = false;
        if !shared.lifecycle.wait_until_running(&shared.accepting) {
            break;
        }
        let connection_shared = Arc::clone(shared);
        if let Ok(thread) = thread::Builder::new()
            .name("appd-connection".to_owned())
            .spawn(move || {
                if let Err(error) = serve_connection(&connection_shared, stream) {
                    report_connection_error(&error);
                }
            })
        {
            connection_threads.push(thread);
        }
    }
    close_connections(shared);
    for thread in connection_threads {
        let _ = thread.join();
    }
}

pub(super) fn listener_was_closed(error: &io::Error) -> bool {
    #[cfg(unix)]
    return error.raw_os_error() == Some(libc::EBADF);

    #[cfg(not(unix))]
    false
}

fn replace_closed_listener(listener: TcpListener, port: u16) -> io::Result<(TcpListener, u16)> {
    std::mem::forget(listener);
    bind_replacement_listener(port)
}

pub(super) fn bind_replacement_listener(port: u16) -> io::Result<(TcpListener, u16)> {
    let replacement = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(port_error) => TcpListener::bind(("127.0.0.1", 0)).map_err(|random_error| {
            io::Error::new(
                random_error.kind(),
                format!(
                    "port {port} is unavailable ({port_error}); random port failed: {random_error}"
                ),
            )
        })?,
    };
    let replacement_port = replacement.local_addr()?.port();
    Ok((replacement, replacement_port))
}

struct ConnectionGuard {
    shared: Arc<Shared>,
    connection: Arc<TcpStream>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        lock_connections(&self.shared)
            .retain(|connection| !Arc::ptr_eq(connection, &self.connection));
    }
}

pub(super) fn lock_connections(shared: &Shared) -> MutexGuard<'_, Vec<Arc<TcpStream>>> {
    match shared.connections.lock() {
        Ok(connections) => connections,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(super) fn close_connections(shared: &Shared) {
    for connection in lock_connections(shared).iter() {
        let _ = connection.shutdown(Shutdown::Both);
    }
}

pub(super) fn probe_gateway(port: u16) -> io::Result<()> {
    let timeout = Duration::from_millis(100);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| io::Error::new(error.kind(), format!("TCP connect failed: {error}")))?;
    stream.set_read_timeout(Some(timeout)).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("setting read timeout failed: {error}"),
        )
    })?;
    stream
        .write_all(b"CONNECT appd-probe.invalid:443 HTTP/1.1\r\n\r\n")
        .map_err(|error| io::Error::new(error.kind(), format!("CONNECT write failed: {error}")))?;
    let response = std::io::read_to_string(stream)
        .map_err(|error| io::Error::new(error.kind(), format!("CONNECT read failed: {error}")))?;
    if response.starts_with("HTTP/1.1 400") && response.ends_with("\r\n\r\nBad CONNECT request") {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "unexpected CONNECT response: {}",
            response.lines().next().unwrap_or("empty response")
        )))
    }
}

pub(super) fn wait_for_gateway(mut port: impl FnMut() -> u16) -> io::Result<u16> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let current = port();
        match probe_gateway(current) {
            Ok(()) => return Ok(current),
            Err(error) if Instant::now() >= deadline => {
                return Err(io::Error::other(format!(
                    "gateway did not recover: {error}"
                )));
            }
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn reap_finished_connections(connections: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < connections.len() {
        if connections[index].is_finished() {
            let thread = connections.swap_remove(index);
            let _ = thread.join();
        } else {
            index += 1;
        }
    }
}

pub(super) fn serve_connection(shared: &Arc<Shared>, mut stream: TcpStream) -> Result<(), Error> {
    let connection = Arc::new(stream.try_clone()?);
    lock_connections(shared).push(Arc::clone(&connection));
    let _guard = ConnectionGuard {
        shared: Arc::clone(shared),
        connection,
    };
    if !shared.accepting.load(Ordering::Acquire) {
        return Ok(());
    }
    if !shared.lifecycle.wait_until_running(&shared.accepting) {
        return Ok(());
    }
    let connect = read_headers(&mut stream)?;
    if !is_connect(&connect, &shared.config.host) {
        write_plain_response(&mut stream, HttpResponse::text(400, "Bad CONNECT request"))?;
        return Ok(());
    }
    stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    stream.flush()?;

    let acceptor = tls_acceptor(&shared.config)?;
    let mut tls = acceptor
        .accept(stream)
        .map_err(|error| Error::Tls(error.to_string()))?;
    if shared.config.require_client_certificate && tls.ssl().peer_certificate().is_none() {
        write_response(
            &mut tls,
            HttpResponse::text(403, "Client certificate required"),
        )?;
        return Ok(());
    }
    let request = read_request(&mut tls, &shared.config.host)?;
    let websocket = is_websocket(&request);
    let websocket_key = request.headers.get("sec-websocket-key").cloned();
    let (websocket_job, websocket_bridge) = if websocket {
        let (incoming_sender, incoming_receiver) = mpsc::sync_channel(MAX_WEBSOCKET_QUEUE);
        let (outgoing_sender, outgoing_receiver) = mpsc::sync_channel(MAX_WEBSOCKET_QUEUE);
        (
            Some(WebSocketJob {
                incoming: incoming_receiver,
                outgoing: outgoing_sender,
            }),
            Some(WebSocketBridge {
                incoming: incoming_sender,
                outgoing: outgoing_receiver,
            }),
        )
    } else {
        (None, None)
    };
    let (response, result) = mpsc::sync_channel(1);
    let job = Job {
        request,
        response,
        websocket: websocket_job,
    };
    let execution_shared = Arc::clone(shared);
    drop(
        shared
            .tokio
            .spawn_blocking(move || execute_job(&execution_shared, job)),
    );
    let response = result
        .recv()
        .map_err(|_| Error::Startup("JavaScript request was dropped".to_owned()))?;
    match response {
        JobResponse::WebSocket => websocket_session(
            &mut tls,
            websocket_key.as_deref(),
            websocket_bridge
                .as_ref()
                .ok_or_else(|| Error::Startup("WebSocket bridge was not created".to_owned()))?,
        ),
        JobResponse::Http(response) => write_response(&mut tls, response),
    }
}

fn execute_job(shared: &Shared, job: Job) {
    let Some(execution) = shared.lifecycle.enter(&shared.accepting) else {
        return;
    };
    let response = job.response.clone();
    if let Err(error) = shared.handler.handle(job, &execution, &shared.accepting) {
        let _ = response.send(JobResponse::Http(HttpResponse::text(
            500,
            &format!("Worker error: {error}"),
        )));
    }
}

fn report_connection_error(error: &Error) {
    let message = format!("gateway request failed: {error}");
    #[cfg(target_os = "android")]
    {
        let (Ok(tag), Ok(message)) = (CString::new("appd"), CString::new(message)) else {
            return;
        };
        // SAFETY: both strings are valid, NUL-terminated strings for the call.
        unsafe {
            __android_log_write(6, tag.as_ptr(), message.as_ptr());
        }
    }
    #[cfg(not(target_os = "android"))]
    eprintln!("{message}");
}
