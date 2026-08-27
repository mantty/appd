use std::collections::BTreeMap;
#[cfg(target_os = "android")]
use std::ffi::{CString, c_char, c_int};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::fs::VirtualFileSystem;
use crate::fs::{
    MODULE_NAME as NODE_FS_MODULE_NAME, NodeFsModule, NodeFsPromisesModule,
    PROMISES_MODULE_NAME as NODE_FS_PROMISES_MODULE_NAME, install,
};
use flate2::read::GzDecoder;
use rquickjs::loader::{BuiltinResolver, ImportAttributes, Loader, ModuleLoader, Resolver};
use rquickjs::{
    Array, ArrayBuffer, Context, Function, Module, Object, Promise, Runtime as JsRuntime,
};
use tokio::runtime::{Builder as TokioBuilder, Runtime as TokioRuntime};

use super::{Error, RuntimeConfig, WorkerBundle};

use super::transport::{
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
    pub(super) worker: Arc<WorkerBundle>,
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
    fn is_running(&self) -> bool {
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

pub(super) struct WebSocketFrame {
    pub(super) final_frame: bool,
    pub(super) opcode: u8,
    pub(super) payload: Vec<u8>,
}

impl Runtime {
    pub(crate) fn start(bundle: WorkerBundle, config: RuntimeConfig) -> Result<Self, Error> {
        let listener = TcpListener::bind(("127.0.0.1", config.port))?;
        let port = listener.local_addr()?.port();
        let tokio_runtime = TokioBuilder::new_multi_thread()
            .thread_name("appd-tokio")
            .build()
            .map_err(|error| Error::Startup(format!("failed to start Tokio: {error}")))?;
        let shared = Arc::new(Shared {
            worker: Arc::new(bundle),
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
    if let Some(response) = asset_response(&shared.config, &request)? {
        return write_response(&mut tls, response);
    }
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
    if let Err(error) = execute_request(
        &shared.worker,
        &shared.config,
        job,
        &execution,
        &shared.accepting,
    ) {
        let _ = response.send(JobResponse::Http(HttpResponse::text(
            500,
            &format!("Worker error: {error}"),
        )));
    }
}

pub(super) fn configure_worker_loader(runtime: &JsRuntime, worker: &WorkerBundle) {
    runtime.set_loader(
        (
            BuiltinResolver::default()
                .with_module(NODE_FS_MODULE_NAME)
                .with_module(NODE_FS_PROMISES_MODULE_NAME),
            WorkerResolver,
        ),
        (
            ModuleLoader::default()
                .with_module(NODE_FS_MODULE_NAME, NodeFsModule)
                .with_module(NODE_FS_PROMISES_MODULE_NAME, NodeFsPromisesModule),
            WorkerLoader {
                bundle: worker.clone(),
            },
        ),
    );
}

pub(super) fn execute_request(
    worker: &WorkerBundle,
    config: &RuntimeConfig,
    job: Job,
    execution: &Execution<'_>,
    accepting: &Arc<AtomicBool>,
) -> Result<(), Error> {
    let runtime = JsRuntime::new().map_err(|error| js_error("runtime", error))?;
    let interrupt_accepting = Arc::clone(accepting);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        !interrupt_accepting.load(Ordering::Acquire)
    })));
    configure_worker_loader(&runtime, worker);
    let context = Context::full(&runtime).map_err(|error| js_error("context", error))?;
    context.with(|ctx| -> Result<(), Error> {
        let Job {
            request,
            response: response_sender,
            websocket,
        } = job;
        let vfs = Arc::new(Mutex::new(VirtualFileSystem::new(worker.vfs_bundle.clone())));
        install(&ctx, &vfs).map_err(|error| js_error("node fs", error))?;
        let environment = serde_json::to_string(&config.environment)?;
        let descriptor = serde_json::to_string(&request)?;
        let setup = format!(
            "globalThis.__appd_env = {environment}; globalThis.__appd_env.ASSETS = {{ fetch: async () => new Response(null, {{ status: 404 }}) }}; globalThis.__appd_request = {descriptor};"
        );
        ctx.eval::<(), _>(setup)
            .map_err(|error| js_error("setup", error))?;

        let fetch = load_worker(&ctx, worker)?;
        let request: Object = ctx
            .eval("new Request(__appd_request.url, { method: __appd_request.method, headers: __appd_request.headers, body: __appd_request.body })")
            .map_err(|error| js_error("request", error))?;
        let environment: Object = ctx
            .globals()
            .get("__appd_env")
            .map_err(|error| js_error("environment", error))?;
        let execution_context: Object = ctx
            .eval("({ __waitUntil: [], waitUntil(value) { this.__waitUntil.push(Promise.resolve(value)); }, passThroughOnException() {} })")
            .map_err(|error| js_error("execution context", error))?;
        let response: Promise = fetch
            .call((request, environment, execution_context.clone()))
            .map_err(|error| js_error("fetch", error))?;
        let response: Object = response
            .finish()
            .map_err(|error| js_error("response", error))?;
        drain_wait_until(&ctx, &execution_context)?;
        let web_socket: Option<Object> = response
            .get("webSocket")
            .map_err(|error| js_error("response WebSocket", error))?;
        let response = response_from_js(&ctx, &response)?;
        if response.status == 101
            && let (Some(web_socket), Some(websocket)) = (web_socket, websocket)
        {
            let server: Object = web_socket
                .get("__appd_peer")
                .map_err(|error| js_error("WebSocket peer", error))?;
            let receive: Function = server
                .get("__appd_receive")
                .map_err(|error| js_error("WebSocket receive", error))?;
            let close: Function = server
                .get("__appd_close")
                .map_err(|error| js_error("WebSocket close", error))?;
            response_sender
                .send(JobResponse::WebSocket)
                .map_err(|_| Error::Startup("WebSocket response receiver closed".to_owned()))?;
            websocket_loop(
                &ctx,
                &web_socket,
                &receive,
                &close,
                &websocket,
                execution,
            )?;
            return Ok(());
        }
        response_sender
            .send(JobResponse::Http(response))
            .map_err(|_| Error::Startup("HTTP response receiver closed".to_owned()))
    })
}

pub(super) fn load_worker<'js>(
    ctx: &rquickjs::Ctx<'js>,
    bundle: &WorkerBundle,
) -> Result<Function<'js>, Error> {
    let bytes = read_worker_module(bundle, &bundle.entry)?;
    let module =
        unsafe { Module::load(ctx.clone(), &bytes) }.map_err(|error| js_error("load", error))?;
    let (module, evaluation) = module.eval().map_err(|error| js_error("evaluate", error))?;
    evaluation
        .finish::<()>()
        .map_err(|error| js_exception(ctx, "module initialization", error))?;
    let worker: Object = module
        .get("default")
        .map_err(|error| js_error("worker export", error))?;
    worker
        .get("fetch")
        .map_err(|error| js_error("worker fetch", error))
}

pub(super) struct WorkerResolver;

impl Resolver for WorkerResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &rquickjs::Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let resolved = super::api::resolve_module_name(base, name);
        if is_module_name(&resolved) {
            Ok(resolved)
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

pub(super) struct WorkerLoader {
    pub(super) bundle: WorkerBundle,
}

impl Loader for WorkerLoader {
    fn load<'js>(
        &mut self,
        ctx: &rquickjs::Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let bytes = read_worker_module(&self.bundle, name)
            .map_err(|error| rquickjs::Error::new_loading_message(name, error.to_string()))?;
        // Packaged bytecode is produced by appd itself and is trusted here.
        unsafe { Module::load(ctx.clone(), &bytes) }
    }
}

fn is_module_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn read_worker_module(bundle: &WorkerBundle, name: &str) -> io::Result<Vec<u8>> {
    if !is_module_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Worker module name",
        ));
    }
    if let Some(bytecode) = &bundle.legacy {
        if name == bundle.entry {
            return Ok((**bytecode).clone());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Worker module not found",
        ));
    }
    let bytes = std::fs::read(bundle.modules.join(format!("{name}.qjs")))?;
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes);
    }
    let mut decoder = GzDecoder::new(bytes.as_slice());
    let mut bytecode = Vec::new();
    decoder.read_to_end(&mut bytecode)?;
    Ok(bytecode)
}

fn websocket_loop<'js>(
    ctx: &rquickjs::Ctx<'js>,
    client: &Object<'js>,
    receive: &Function<'js>,
    close: &Function<'js>,
    websocket: &WebSocketJob,
    execution: &Execution<'_>,
) -> Result<(), Error> {
    let take_outbox: Function = ctx
        .eval(
            "socket => { const outbox = socket.__appd_outbox; socket.__appd_outbox = []; return outbox; }",
        )
        .map_err(|error| js_error("WebSocket outbox", error))?;
    drain_pending_jobs(ctx);
    drain_websocket_outbox(client, &take_outbox, &websocket.outgoing)?;
    signal_websocket_ready(&websocket.outgoing)?;

    loop {
        if !execution.is_running() {
            return Ok(());
        }
        let event = match websocket.incoming.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        };
        let should_close = match event {
            WebSocketInbound::Message { binary, payload } => {
                if binary {
                    let data = ArrayBuffer::new_copy(ctx.clone(), payload)
                        .map_err(|error| js_error("WebSocket message", error))?;
                    receive
                        .call::<_, ()>((data, true))
                        .map_err(|error| js_exception(ctx, "WebSocket message", error))?;
                } else {
                    let data = String::from_utf8_lossy(&payload).into_owned();
                    receive
                        .call::<_, ()>((data, false))
                        .map_err(|error| js_exception(ctx, "WebSocket message", error))?;
                }
                false
            }
            WebSocketInbound::Close { code, reason } => {
                close
                    .call::<_, ()>((code, reason))
                    .map_err(|error| js_exception(ctx, "WebSocket close", error))?;
                true
            }
        };
        drain_pending_jobs(ctx);
        drain_websocket_outbox(client, &take_outbox, &websocket.outgoing)?;
        signal_websocket_ready(&websocket.outgoing)?;
        if should_close {
            return Ok(());
        }
    }
}

fn drain_pending_jobs(ctx: &rquickjs::Ctx<'_>) {
    while ctx.execute_pending_job() {}
}

fn drain_websocket_outbox<'js>(
    client: &Object<'js>,
    take_outbox: &Function<'js>,
    outgoing: &SyncSender<WebSocketOutbound>,
) -> Result<(), Error> {
    let messages: Array = take_outbox
        .call((client.clone(),))
        .map_err(|error| js_error("WebSocket outbox", error))?;
    for entry in messages.iter::<Object>() {
        let entry = entry.map_err(|error| js_error("WebSocket outbox entry", error))?;
        let message_type: String = entry
            .get("type")
            .map_err(|error| js_error("WebSocket outbox type", error))?;
        match message_type.as_str() {
            "message" => {
                let binary: bool = entry
                    .get("binary")
                    .map_err(|error| js_error("WebSocket outbox binary flag", error))?;
                let payload = if binary {
                    let data: ArrayBuffer = entry
                        .get("data")
                        .map_err(|error| js_error("WebSocket outbox data", error))?;
                    data.as_bytes()
                        .ok_or_else(|| Error::Engine("WebSocket data was detached".to_owned()))?
                        .to_vec()
                } else {
                    let data: String = entry
                        .get("data")
                        .map_err(|error| js_error("WebSocket outbox data", error))?;
                    data.into_bytes()
                };
                outgoing
                    .send(WebSocketOutbound::Message { binary, payload })
                    .map_err(|_| Error::Startup("WebSocket connection closed".to_owned()))?;
            }
            "close" => {
                let code: u16 = entry
                    .get("code")
                    .map_err(|error| js_error("WebSocket close code", error))?;
                let reason: String = entry
                    .get("reason")
                    .map_err(|error| js_error("WebSocket close reason", error))?;
                outgoing
                    .send(WebSocketOutbound::Close { code, reason })
                    .map_err(|_| Error::Startup("WebSocket connection closed".to_owned()))?;
            }
            _ => return Err(Error::Engine("unknown WebSocket outbox entry".to_owned())),
        }
    }
    Ok(())
}

fn signal_websocket_ready(outgoing: &SyncSender<WebSocketOutbound>) -> Result<(), Error> {
    outgoing
        .send(WebSocketOutbound::Ready)
        .map_err(|_| Error::Startup("WebSocket connection closed".to_owned()))
}

fn drain_wait_until<'js>(
    ctx: &rquickjs::Ctx<'js>,
    execution_context: &Object<'js>,
) -> Result<(), Error> {
    let drain: Function = ctx
        .eval("context => Promise.allSettled(context.__waitUntil)")
        .map_err(|error| js_error("waitUntil", error))?;
    let pending: Promise = drain
        .call((execution_context.clone(),))
        .map_err(|error| js_error("waitUntil", error))?;
    pending
        .finish::<Array>()
        .map_err(|error| js_exception(ctx, "waitUntil", error))?;
    Ok(())
}

fn response_from_js<'js>(
    ctx: &rquickjs::Ctx<'js>,
    response: &Object<'js>,
) -> Result<HttpResponse, Error> {
    let status: u16 = response
        .get("status")
        .map_err(|error| js_error("response status", error))?;
    let mut headers = BTreeMap::new();
    let object: Object = response
        .get("headers")
        .map_err(|error| js_error("response headers", error))?;
    let entries_fn: Function = ctx
        .eval("headers => Array.from(headers)")
        .map_err(|error| js_error("response headers", error))?;
    let entries: Array = entries_fn
        .call((object,))
        .map_err(|error| js_error("response headers", error))?;
    for entry in entries.iter::<Array>() {
        let entry = entry.map_err(|error| js_error("response header", error))?;
        let name: String = entry
            .get(0)
            .map_err(|error| js_error("response header name", error))?;
        let value: String = entry
            .get(1)
            .map_err(|error| js_error("response header value", error))?;
        headers.insert(name, value);
    }
    let read_body: Function = ctx
        .eval("response => response.text()")
        .map_err(|error| js_error("response body", error))?;
    let body: Promise = read_body
        .call((response.clone(),))
        .map_err(|error| js_error("response body", error))?;
    let body: String = body
        .finish()
        .map_err(|error| js_exception(ctx, "response body", error))?;
    Ok(HttpResponse {
        status,
        headers,
        body: body.into_bytes(),
    })
}

pub(super) fn asset_response(
    config: &RuntimeConfig,
    request: &HttpRequest,
) -> Result<Option<HttpResponse>, Error> {
    let Some(assets) = &config.assets else {
        return Ok(None);
    };
    if request.method != "GET" && request.method != "HEAD" {
        return Ok(None);
    }
    let manifest: AssetManifest = serde_json::from_slice(&std::fs::read(&assets.manifest)?)?;
    let path = request.target.split('?').next().unwrap_or("/");
    let Some(relative) = manifest.path_for(path) else {
        return Ok(None);
    };
    let file = assets.root.join(&relative);
    let body = std::fs::read(file)?;
    let mut response = HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: if request.method == "HEAD" {
            Vec::new()
        } else {
            body
        },
    };
    response
        .headers
        .insert("content-type".to_owned(), manifest.content_type(&relative));
    Ok(Some(response))
}

#[derive(serde::Deserialize)]
pub(super) struct AssetManifest {
    pub(super) files: BTreeMap<String, String>,
    #[serde(rename = "htmlHandling")]
    pub(super) html_handling: String,
}

impl AssetManifest {
    pub(super) fn path_for(&self, path: &str) -> Option<String> {
        let path = path.trim_start_matches('/');
        let candidates = match self.html_handling.as_str() {
            "force-trailing-slash" | "auto-trailing-slash" => {
                if path.is_empty() {
                    vec!["index.html".to_owned()]
                } else if path.ends_with('/') {
                    vec![
                        format!("{path}index.html"),
                        path.trim_end_matches('/').to_owned(),
                    ]
                } else {
                    vec![format!("{path}/index.html"), path.to_owned()]
                }
            }
            "drop-trailing-slash" => vec![
                path.trim_end_matches('/').to_owned(),
                format!("{path}.html"),
            ],
            _ => vec![path.to_owned()],
        };
        candidates.into_iter().find(|candidate| {
            self.files.contains_key(&format!("/{candidate}")) || self.files.contains_key(candidate)
        })
    }

    pub(super) fn content_type(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        self.files
            .get(path)
            .or_else(|| self.files.get(&format!("/{path}")))
            .cloned()
            .unwrap_or_else(|| "application/octet-stream".to_owned())
    }
}
fn js_error(stage: &str, error: impl std::fmt::Display) -> Error {
    Error::Engine(format!("{stage}: {error}"))
}

fn js_exception(ctx: &rquickjs::Ctx<'_>, stage: &str, error: impl std::fmt::Display) -> Error {
    let value = ctx.catch();
    let detail = value.as_exception().map_or_else(
        || error.to_string(),
        |exception| {
            let message = exception.message().unwrap_or_default();
            let stack = exception.stack().unwrap_or_default();
            if stack.is_empty() {
                message
            } else {
                format!("{message}\n{stack}")
            }
        },
    );
    Error::Engine(format!("{stage}: {detail}"))
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
