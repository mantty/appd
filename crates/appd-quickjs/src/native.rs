use std::collections::BTreeMap;
#[cfg(target_os = "android")]
use std::ffi::CString;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use openssl::sha::sha1;
use openssl::ssl::{SslAcceptor, SslMethod, SslStream, SslVerifyMode};
use openssl::x509::X509;
use rquickjs::{
    Array, ArrayBuffer, Context, Function, Module, Object, Promise, Runtime as JsRuntime,
};
use serde::Serialize;
use tokio::runtime::{Builder as TokioBuilder, Runtime as TokioRuntime};

use super::{Error, RuntimeConfig};

const MAX_HEADERS: usize = 64 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_WEBSOCKET_QUEUE: usize = 100;

pub(super) struct Runtime {
    shared: Arc<Shared>,
    gateway: Option<JoinHandle<()>>,
    tokio: Option<TokioRuntime>,
}

struct Shared {
    bundle: Arc<Vec<u8>>,
    config: RuntimeConfig,
    tokio: tokio::runtime::Handle,
    port: AtomicU16,
    accepting: Arc<AtomicBool>,
    lifecycle: Arc<Lifecycle>,
    connections: Mutex<Vec<Arc<TcpStream>>>,
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

struct Lifecycle {
    status: Mutex<LifecycleStatus>,
    changed: Condvar,
}

struct Execution<'a> {
    lifecycle: &'a Lifecycle,
    accepting: &'a AtomicBool,
}

impl Lifecycle {
    fn new() -> Self {
        Self {
            status: Mutex::new(LifecycleStatus {
                phase: LifecyclePhase::Running,
                active: 0,
            }),
            changed: Condvar::new(),
        }
    }

    fn suspend(&self) {
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

    fn resume(&self) {
        let mut status = lock_status(&self.status);
        if status.phase != LifecyclePhase::Stopping {
            status.phase = LifecyclePhase::Running;
            self.changed.notify_all();
        }
    }

    fn stop(&self) {
        let mut status = lock_status(&self.status);
        status.phase = LifecyclePhase::Stopping;
        self.changed.notify_all();
    }

    fn enter<'a>(&'a self, accepting: &'a AtomicBool) -> Option<Execution<'a>> {
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

struct Job {
    request: HttpRequest,
    response: SyncSender<JobResponse>,
    websocket: Option<WebSocketJob>,
}

enum JobResponse {
    Http(HttpResponse),
    WebSocket,
}

struct WebSocketJob {
    incoming: Receiver<WebSocketInbound>,
    outgoing: SyncSender<WebSocketOutbound>,
}

struct WebSocketBridge {
    incoming: SyncSender<WebSocketInbound>,
    outgoing: Receiver<WebSocketOutbound>,
}

enum WebSocketInbound {
    Message { binary: bool, payload: Vec<u8> },
    Close { code: u16, reason: String },
}

enum WebSocketOutbound {
    Message { binary: bool, payload: Vec<u8> },
    Close { code: u16, reason: String },
    Ready,
}

struct WebSocketFrame {
    final_frame: bool,
    opcode: u8,
    payload: Vec<u8>,
}

impl Runtime {
    pub(super) fn start(bundle: &[u8], config: RuntimeConfig) -> Result<Self, Error> {
        let listener = TcpListener::bind(("127.0.0.1", config.port))?;
        let port = listener.local_addr()?.port();
        let tokio_runtime = TokioBuilder::new_multi_thread()
            .thread_name("appd-tokio")
            .build()
            .map_err(|error| Error::Startup(format!("failed to start Tokio: {error}")))?;
        let shared = Arc::new(Shared {
            bundle: Arc::new(bundle.to_vec()),
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

    pub(super) fn port(&self) -> u16 {
        self.shared.port.load(Ordering::Acquire)
    }

    pub(super) fn suspend(&self) {
        self.shared.lifecycle.suspend();
        close_connections(&self.shared);
    }

    pub(super) fn resume(&self) {
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

fn listener_was_closed(error: &io::Error) -> bool {
    #[cfg(unix)]
    return error.raw_os_error() == Some(libc::EBADF);

    #[cfg(not(unix))]
    false
}

fn replace_closed_listener(listener: TcpListener, port: u16) -> io::Result<(TcpListener, u16)> {
    std::mem::forget(listener);
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

fn lock_connections(shared: &Shared) -> MutexGuard<'_, Vec<Arc<TcpStream>>> {
    match shared.connections.lock() {
        Ok(connections) => connections,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn close_connections(shared: &Shared) {
    for connection in lock_connections(shared).iter() {
        let _ = connection.shutdown(Shutdown::Both);
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

fn serve_connection(shared: &Arc<Shared>, mut stream: TcpStream) -> Result<(), Error> {
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
        &shared.bundle,
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

fn execute_request(
    bundle: &[u8],
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
    let context = Context::full(&runtime).map_err(|error| js_error("context", error))?;
    context.with(|ctx| -> Result<(), Error> {
        let Job {
            request,
            response: response_sender,
            websocket,
        } = job;
        let environment = serde_json::to_string(&config.environment)?;
        let descriptor = serde_json::to_string(&request)?;
        let setup = format!(
            "globalThis.__appd_env = {environment}; globalThis.__appd_env.ASSETS = {{ fetch: async () => new Response(null, {{ status: 404 }}) }}; globalThis.__appd_request = {descriptor}; globalThis.__appd_tmp = new Map(); globalThis.__appd_tmp_directories = new Set(['/tmp']);"
        );
        ctx.eval::<(), _>(setup)
            .map_err(|error| js_error("setup", error))?;

        let fetch = load_worker(&ctx, bundle)?;
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

fn load_worker<'js>(ctx: &rquickjs::Ctx<'js>, bundle: &[u8]) -> Result<Function<'js>, Error> {
    let module =
        unsafe { Module::load(ctx.clone(), bundle) }.map_err(|error| js_error("load", error))?;
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

fn asset_response(
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
struct AssetManifest {
    files: BTreeMap<String, String>,
    #[serde(rename = "htmlHandling")]
    html_handling: String,
}

impl AssetManifest {
    fn path_for(&self, path: &str) -> Option<String> {
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

    fn content_type(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        self.files
            .get(path)
            .or_else(|| self.files.get(&format!("/{path}")))
            .cloned()
            .unwrap_or_else(|| "application/octet-stream".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AssetManifest, HttpRequest, HttpResponse, Job, JobResponse, Lifecycle, Shared,
        WebSocketBridge, WebSocketInbound, WebSocketJob, WebSocketOutbound, asset_response,
        close_connections, execute_request, listener_was_closed, lock_connections,
        queue_websocket_message, replace_closed_listener, serve_connection,
    };
    use crate::{Assets, Certificates, RuntimeConfig};
    use std::collections::BTreeMap;
    use std::net::{TcpListener, TcpStream};
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
    use std::sync::mpsc::{self, Receiver, SyncSender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    const WEBSOCKET_WORKER: &[u8] = br#"
globalThis.Request = class { constructor(url, init = {}) { this.url = url; this.method = init.method ?? "GET"; this.headers = init.headers ?? {}; this.body = init.body; } };
globalThis.Response = class { constructor(body = null, init = {}) { this.status = init.status ?? 200; this.headers = new Map(); this.webSocket = init.webSocket; } async text() { return ""; } };
class Socket {
  constructor() {
    this.__appd_outbox = [];
    this.__appd_peer = undefined;
    this.__appd_listener = undefined;
    this.__appd_receive = (data) => this.__appd_listener?.({ data });
    this.__appd_close = () => {};
  }
  accept() {}
  addEventListener(name, listener) { if (name === "message") this.__appd_listener = listener; }
  send(data) { this.__appd_peer.__appd_outbox.push({ type: "message", binary: false, data }); }
}
globalThis.WebSocketPair = class {
  constructor() {
    this[0] = new Socket();
    this[1] = new Socket();
    this[0].__appd_peer = this[1];
    this[1].__appd_peer = this[0];
  }
};
export default {
  async fetch() {
    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];
    server.accept();
    server.addEventListener("message", (event) => server.send(`pong ${event.data}`));
    return new Response(null, { status: 101, webSocket: client });
  }
};
"#;

    const SLOW_WORKER: &[u8] = br#"
globalThis.Request = class { constructor(url, init = {}) { this.url = url; this.method = init.method ?? "GET"; this.headers = init.headers ?? {}; this.body = init.body; } };
globalThis.Response = class { constructor(_body = null, init = {}) { this.status = init.status ?? 200; this.headers = new Map(); } async text() { return "ok"; } };
export default {
  async fetch() {
    const deadline = Date.now() + 250;
    while (Date.now() < deadline) {}
    return new Response(null);
  }
};
"#;

    #[test]
    fn uses_the_resolved_asset_for_content_type() {
        let manifest = AssetManifest {
            files: BTreeMap::from([("about/index.html".to_owned(), "text/html".to_owned())]),
            html_handling: "auto-trailing-slash".to_owned(),
        };

        assert_eq!(
            manifest.path_for("/about").as_deref(),
            Some("about/index.html")
        );
        assert_eq!(manifest.content_type("about/index.html"), "text/html");
    }

    #[test]
    fn serves_the_resolved_asset_with_its_content_type() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path();
        std::fs::create_dir_all(root.join("about"))?;
        std::fs::write(
            root.join("asset-manifest.json"),
            br#"{"files":{"about/index.html":"text/html"},"htmlHandling":"auto-trailing-slash"}"#,
        )?;
        std::fs::write(root.join("about/index.html"), b"about")?;

        let config = RuntimeConfig {
            assets: Some(Assets {
                manifest: root.join("asset-manifest.json"),
                root: root.to_owned(),
            }),
            cache: root.join("cache"),
            certificates: Certificates {
                ca: root.join("ca.pem"),
                certificate: root.join("certificate.pem"),
                private_key: root.join("private-key.pem"),
            },
            environment: BTreeMap::new(),
            host: "example.test".to_owned(),
            require_client_certificate: false,
            port: 0,
        };
        let request = HttpRequest {
            method: "GET".to_owned(),
            target: "/about".to_owned(),
            url: "https://example.test/about".to_owned(),
            headers: BTreeMap::new(),
            body: None,
        };
        let response = asset_response(&config, &request)?.ok_or("asset was not found")?;

        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("text/html")
        );
        assert_eq!(response.body, b"about");
        Ok(())
    }

    #[test]
    fn routes_worker_websocket_messages_through_the_native_bridge()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let bundle = crate::compile_worker(WEBSOCKET_WORKER)?;
        let config = websocket_config(directory.path());
        let request = websocket_request();
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        let (incoming_sender, incoming_receiver) = mpsc::sync_channel(1);
        let (outgoing_sender, outgoing_receiver) = mpsc::sync_channel(1);
        let accepting = Arc::new(AtomicBool::new(true));
        let thread = std::thread::spawn(move || {
            let lifecycle = Lifecycle::new();
            let Some(execution) = lifecycle.enter(&accepting) else {
                return Err(crate::Error::Startup(
                    "execution was not admitted".to_owned(),
                ));
            };
            execute_request(
                &bundle,
                &config,
                Job {
                    request,
                    response: response_sender,
                    websocket: Some(WebSocketJob {
                        incoming: incoming_receiver,
                        outgoing: outgoing_sender,
                    }),
                },
                &execution,
                &accepting,
            )
        });

        let response = response_receiver.recv_timeout(Duration::from_secs(1));
        if let Err(error) = response {
            let worker_error = thread.join().map_err(|_| "WebSocket worker panicked")?;
            return Err(format!(
                "WebSocket worker stopped before upgrade: {error}; {worker_error:?}"
            )
            .into());
        }
        assert!(matches!(response?, JobResponse::WebSocket));
        assert!(matches!(
            outgoing_receiver.recv_timeout(Duration::from_secs(1))?,
            WebSocketOutbound::Ready
        ));
        incoming_sender.send(WebSocketInbound::Message {
            binary: false,
            payload: b"ping 42".to_vec(),
        })?;
        match outgoing_receiver.recv_timeout(Duration::from_secs(1))? {
            WebSocketOutbound::Message { binary, payload } => {
                assert!(!binary);
                assert_eq!(payload, b"pong ping 42");
            }
            WebSocketOutbound::Close { .. } => panic!("Worker closed the WebSocket"),
            WebSocketOutbound::Ready => panic!("Worker completed without a response"),
        }
        assert!(matches!(
            outgoing_receiver.recv_timeout(Duration::from_secs(1))?,
            WebSocketOutbound::Ready
        ));
        incoming_sender.send(WebSocketInbound::Close {
            code: 1000,
            reason: String::new(),
        })?;
        thread.join().map_err(|_| "WebSocket worker panicked")??;
        Ok(())
    }

    #[test]
    fn assembles_fragmented_text_and_binary_websocket_messages()
    -> Result<(), Box<dyn std::error::Error>> {
        let (incoming_sender, incoming_receiver) = mpsc::sync_channel(2);
        let (_outgoing_sender, outgoing_receiver) = mpsc::sync_channel(1);
        let bridge = WebSocketBridge {
            incoming: incoming_sender,
            outgoing: outgoing_receiver,
        };
        let mut fragmented = None;

        queue_websocket_message(&bridge, &mut fragmented, false, 0x1, b"ping ".to_vec())?;
        queue_websocket_message(&bridge, &mut fragmented, true, 0x0, b"42".to_vec())?;
        queue_websocket_message(&bridge, &mut fragmented, true, 0x2, vec![1, 2, 3])?;

        assert!(fragmented.is_none());
        assert!(matches!(
            incoming_receiver.recv()?,
            WebSocketInbound::Message { binary: false, payload } if payload == b"ping 42"
        ));
        assert!(matches!(
            incoming_receiver.recv()?,
            WebSocketInbound::Message { binary: true, payload } if payload == vec![1, 2, 3]
        ));
        Ok(())
    }

    #[test]
    fn request_tasks_run_concurrently_on_tokio_blocking_workers()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()?;
        let (started_sender, started_receiver) = mpsc::sync_channel(2);
        let (first_release_sender, first_release_receiver) = mpsc::sync_channel(1);
        let (second_release_sender, second_release_receiver) = mpsc::sync_channel(1);

        let first = spawn_blocking_request(&tokio, started_sender.clone(), first_release_receiver);
        let second = spawn_blocking_request(&tokio, started_sender, second_release_receiver);

        let first_started = started_receiver.recv_timeout(Duration::from_secs(1));
        let second_started = started_receiver.recv_timeout(Duration::from_secs(1));
        first_release_sender.send(())?;
        second_release_sender.send(())?;
        tokio.block_on(async {
            first.await??;
            second.await??;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })?;

        first_started?;
        second_started?;
        Ok(())
    }

    #[test]
    fn shutdown_closes_registered_connections()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let tokio = tokio::runtime::Builder::new_current_thread().build()?;
        let shared = Arc::new(Shared {
            bundle: Arc::new(Vec::new()),
            config: RuntimeConfig {
                assets: None,
                cache: PathBuf::default(),
                certificates: Certificates {
                    ca: PathBuf::default(),
                    certificate: PathBuf::default(),
                    private_key: PathBuf::default(),
                },
                environment: BTreeMap::new(),
                host: "example.test".to_owned(),
                require_client_certificate: false,
                port: 0,
            },
            tokio: tokio.handle().clone(),
            port: AtomicU16::new(0),
            accepting: Arc::new(AtomicBool::new(true)),
            lifecycle: Arc::new(Lifecycle::new()),
            connections: Mutex::new(Vec::new()),
        });
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let client = TcpStream::connect(listener.local_addr()?)?;
        let (server, _) = listener.accept()?;
        let connection_shared = Arc::clone(&shared);
        let connection = thread::spawn(move || serve_connection(&connection_shared, server));

        while lock_connections(&shared).is_empty() {
            thread::yield_now();
        }
        shared.accepting.store(false, Ordering::Release);
        close_connections(&shared);
        let _ = connection
            .join()
            .map_err(|_| "connection thread panicked")?;
        assert!(lock_connections(&shared).is_empty());
        drop(client);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn closed_listener_reclaims_its_port() -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        unsafe { libc::close(listener.as_raw_fd()) };
        let error = listener.accept().expect_err("closed listener accepted");
        assert!(listener_was_closed(&error));

        let (_listener, replacement_port) = replace_closed_listener(listener, port)?;

        assert_eq!(replacement_port, port);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn closed_listener_uses_a_random_port_when_its_port_was_taken()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        unsafe { libc::close(listener.as_raw_fd()) };
        let _occupied = TcpListener::bind(("127.0.0.1", port))?;

        let (_listener, replacement_port) = replace_closed_listener(listener, port)?;

        assert_ne!(replacement_port, port);
        Ok(())
    }

    #[test]
    fn suspension_drains_active_work_and_blocks_new_work_until_resume()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let lifecycle = Arc::new(Lifecycle::new());
        let accepting = Arc::new(AtomicBool::new(true));
        let execution = lifecycle
            .enter(&accepting)
            .ok_or("initial execution was not admitted")?;
        lifecycle.suspend();

        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (admitted_sender, admitted_receiver) = mpsc::sync_channel(1);
        let waiting_lifecycle = Arc::clone(&lifecycle);
        let waiting_accepting = Arc::clone(&accepting);
        let waiting = thread::spawn(move || {
            started_sender
                .send(())
                .map_err(|error| crate::Error::Startup(error.to_string()))?;
            let Some(_execution) = waiting_lifecycle.enter(&waiting_accepting) else {
                return Err("execution was not admitted after resume".into());
            };
            admitted_sender.send(())?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });

        started_receiver.recv_timeout(Duration::from_secs(1))?;
        assert!(
            admitted_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        drop(execution);
        assert!(
            admitted_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );

        lifecycle.resume();
        admitted_receiver.recv_timeout(Duration::from_secs(1))?;
        waiting.join().map_err(|_| "lifecycle waiter panicked")??;
        Ok(())
    }

    #[test]
    fn suspension_without_active_work_blocks_until_resume()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let lifecycle = Arc::new(Lifecycle::new());
        let accepting = Arc::new(AtomicBool::new(true));
        lifecycle.suspend();
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (admitted_sender, admitted_receiver) = mpsc::sync_channel(1);
        let waiting_lifecycle = Arc::clone(&lifecycle);
        let waiting_accepting = Arc::clone(&accepting);
        let waiting = thread::spawn(move || {
            started_sender.send(())?;
            admitted_sender.send(waiting_lifecycle.enter(&waiting_accepting).is_some())?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });

        started_receiver.recv_timeout(Duration::from_secs(1))?;
        assert!(
            admitted_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        lifecycle.resume();
        assert!(admitted_receiver.recv_timeout(Duration::from_secs(1))?);
        waiting.join().map_err(|_| "lifecycle waiter panicked")??;
        Ok(())
    }

    #[test]
    fn stopping_releases_blocked_admission() -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        let lifecycle = Arc::new(Lifecycle::new());
        let accepting = Arc::new(AtomicBool::new(true));
        lifecycle.suspend();
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (admitted_sender, admitted_receiver) = mpsc::sync_channel(1);
        let waiting_lifecycle = Arc::clone(&lifecycle);
        let waiting_accepting = Arc::clone(&accepting);
        let waiting = thread::spawn(move || {
            started_sender.send(())?;
            admitted_sender.send(waiting_lifecycle.enter(&waiting_accepting).is_some())?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });

        started_receiver.recv_timeout(Duration::from_secs(1))?;
        assert!(
            admitted_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        lifecycle.stop();
        assert!(!admitted_receiver.recv_timeout(Duration::from_secs(1))?);
        waiting.join().map_err(|_| "lifecycle waiter panicked")??;
        Ok(())
    }

    #[test]
    fn suspension_allows_an_active_javascript_turn_to_finish()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let directory = tempfile::tempdir()?;
        let bundle = crate::compile_worker(SLOW_WORKER)?;
        let config = websocket_config(directory.path());
        let request = websocket_request();
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        let lifecycle = Arc::new(Lifecycle::new());
        let accepting = Arc::new(AtomicBool::new(true));
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let worker_lifecycle = Arc::clone(&lifecycle);
        let worker_accepting = Arc::clone(&accepting);
        let worker = thread::spawn(move || {
            let Some(execution) = worker_lifecycle.enter(&worker_accepting) else {
                return Err(crate::Error::Startup(
                    "execution was not admitted".to_owned(),
                ));
            };
            started_sender
                .send(())
                .map_err(|error| crate::Error::Startup(error.to_string()))?;
            execute_request(
                &bundle,
                &config,
                Job {
                    request,
                    response: response_sender,
                    websocket: None,
                },
                &execution,
                &worker_accepting,
            )
        });

        started_receiver.recv_timeout(Duration::from_secs(1))?;
        lifecycle.suspend();
        assert!(
            response_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
        assert!(matches!(
            response_receiver.recv_timeout(Duration::from_secs(1))?,
            JobResponse::Http(HttpResponse { status: 200, .. })
        ));
        worker.join().map_err(|_| "worker panicked")??;
        Ok(())
    }

    fn spawn_blocking_request(
        tokio: &tokio::runtime::Runtime,
        started: SyncSender<()>,
        release: Receiver<()>,
    ) -> tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        tokio.spawn_blocking(move || {
            started.send(())?;
            release.recv_timeout(Duration::from_secs(1))?;
            Ok(())
        })
    }

    fn websocket_config(root: &Path) -> RuntimeConfig {
        RuntimeConfig {
            assets: None,
            cache: root.join("cache"),
            certificates: Certificates {
                ca: root.join("ca.pem"),
                certificate: root.join("certificate.pem"),
                private_key: root.join("private-key.pem"),
            },
            environment: BTreeMap::new(),
            host: "example.test".to_owned(),
            require_client_certificate: false,
            port: 0,
        }
    }

    fn websocket_request() -> HttpRequest {
        HttpRequest {
            method: "GET".to_owned(),
            target: "/socket".to_owned(),
            url: "https://example.test/socket".to_owned(),
            headers: BTreeMap::new(),
            body: None,
        }
    }
}

#[derive(Serialize)]
struct HttpRequest {
    method: String,
    target: String,
    url: String,
    headers: BTreeMap<String, String>,
    body: Option<String>,
}

struct HttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn text(status: u16, body: &str) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_owned(),
            "text/plain; charset=utf-8".to_owned(),
        );
        Self {
            status,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }
}

fn read_headers(stream: &mut TcpStream) -> Result<Vec<u8>, Error> {
    read_header_block(stream)
}

fn read_header_block(stream: &mut impl Read) -> Result<Vec<u8>, Error> {
    let mut data = Vec::new();
    loop {
        let mut byte = [0; 1];
        stream.read_exact(&mut byte)?;
        data.push(byte[0]);
        if data.len() > MAX_HEADERS {
            return Err(Error::Startup("HTTP headers exceed the limit".to_owned()));
        }
        if data.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(data)
}

fn is_connect(data: &[u8], host: &str) -> bool {
    let text = String::from_utf8_lossy(data);
    let line = text.lines().next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    parts.next() == Some("CONNECT")
        && parts
            .next()
            .is_some_and(|target| target.eq_ignore_ascii_case(&format!("{host}:443")))
}

fn tls_acceptor(config: &RuntimeConfig) -> Result<SslAcceptor, Error> {
    let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls())
        .map_err(|error| tls_error(&error))?;
    let certificate = X509::from_pem(&std::fs::read(&config.certificates.certificate)?)
        .map_err(|error| tls_error(&error))?;
    let private_key = openssl::pkey::PKey::private_key_from_pem(&std::fs::read(
        &config.certificates.private_key,
    )?)
    .map_err(|error| tls_error(&error))?;
    builder
        .set_certificate(&certificate)
        .map_err(|error| tls_error(&error))?;
    builder
        .set_private_key(&private_key)
        .map_err(|error| tls_error(&error))?;
    builder
        .check_private_key()
        .map_err(|error| tls_error(&error))?;
    if config.require_client_certificate {
        let ca = X509::from_pem(&std::fs::read(&config.certificates.ca)?)
            .map_err(|error| tls_error(&error))?;
        builder
            .cert_store_mut()
            .add_cert(ca)
            .map_err(|error| tls_error(&error))?;
        builder.set_verify(SslVerifyMode::PEER);
    }
    Ok(builder.build())
}

fn read_request(stream: &mut SslStream<TcpStream>, host: &str) -> Result<HttpRequest, Error> {
    let headers = read_header_block(stream)?;
    let text = String::from_utf8_lossy(&headers);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_owned();
    let target = request_parts.next().unwrap_or("/").to_owned();
    if method.is_empty() {
        return Err(Error::Startup("HTTP method is missing".to_owned()));
    }
    let mut request_headers = BTreeMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name == "content-length" {
            content_length = value
                .parse()
                .map_err(|_| Error::Startup("invalid content length".to_owned()))?;
        }
        request_headers.insert(name, value);
    }
    if content_length > MAX_BODY {
        return Err(Error::Startup("HTTP body exceeds the limit".to_owned()));
    }
    let mut body = vec![0; content_length];
    stream.read_exact(&mut body)?;
    let body = if body.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&body).into_owned())
    };
    Ok(HttpRequest {
        method,
        target: target.clone(),
        url: format!("https://{host}{target}"),
        headers: request_headers,
        body,
    })
}

fn is_websocket(request: &HttpRequest) -> bool {
    request
        .headers
        .get("upgrade")
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

fn websocket_session(
    stream: &mut SslStream<TcpStream>,
    key: Option<&str>,
    bridge: &WebSocketBridge,
) -> Result<(), Error> {
    let key = key.ok_or_else(|| Error::Startup("WebSocket key is missing".to_owned()))?;
    let digest = sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes());
    let accept = STANDARD.encode(digest);
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    stream.flush()?;
    if flush_websocket_outbound(stream, &bridge.outgoing)? {
        return Ok(());
    }

    let mut fragmented: Option<(u8, Vec<u8>)> = None;
    loop {
        let Some(frame) = read_websocket_frame(stream)? else {
            return Ok(());
        };
        match frame.opcode {
            0x8 => {
                let (code, reason) = websocket_close(&frame.payload)?;
                write_websocket_frame(stream, frame.opcode, &frame.payload)?;
                let _ = bridge
                    .incoming
                    .send(WebSocketInbound::Close { code, reason });
                return Ok(());
            }
            0x9 => write_websocket_frame(stream, 0xA, &frame.payload)?,
            0xA => {}
            0x0..=0x2 => {
                queue_websocket_message(
                    bridge,
                    &mut fragmented,
                    frame.final_frame,
                    frame.opcode,
                    frame.payload,
                )?;
                if frame.final_frame && flush_websocket_outbound(stream, &bridge.outgoing)? {
                    return Ok(());
                }
            }
            _ => return Err(Error::Startup("invalid WebSocket opcode".to_owned())),
        }
    }
}

fn read_websocket_frame(
    stream: &mut SslStream<TcpStream>,
) -> Result<Option<WebSocketFrame>, Error> {
    let mut header = [0; 2];
    if stream.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    let final_frame = header[0] & 0x80 != 0;
    let opcode = header[0] & 0x0f;
    if header[0] & 0x70 != 0 {
        return Err(Error::Startup(
            "WebSocket reserved bits are unsupported".to_owned(),
        ));
    }
    if header[1] & 0x80 == 0 {
        return Err(Error::Startup(
            "client WebSocket frames must be masked".to_owned(),
        ));
    }
    let mut length = usize::from(header[1] & 0x7f);
    if length == 126 {
        let mut value = [0; 2];
        stream.read_exact(&mut value)?;
        length = usize::from(u16::from_be_bytes(value));
    } else if length == 127 {
        let mut value = [0; 8];
        stream.read_exact(&mut value)?;
        let length64 = u64::from_be_bytes(value);
        length = usize::try_from(length64)
            .map_err(|_| Error::Startup("WebSocket frame is too large".to_owned()))?;
    }
    if length > MAX_BODY {
        return Err(Error::Startup("WebSocket frame is too large".to_owned()));
    }
    if opcode >= 0x8 && (!final_frame || length > 125) {
        return Err(Error::Startup(
            "WebSocket control frame is invalid".to_owned(),
        ));
    }
    let mut mask = [0; 4];
    stream.read_exact(&mut mask)?;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload)?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % mask.len()];
    }
    Ok(Some(WebSocketFrame {
        final_frame,
        opcode,
        payload,
    }))
}

fn queue_websocket_message(
    bridge: &WebSocketBridge,
    fragmented: &mut Option<(u8, Vec<u8>)>,
    final_frame: bool,
    opcode: u8,
    payload: Vec<u8>,
) -> Result<(), Error> {
    let (message_opcode, payload) = match opcode {
        0x0 => {
            let Some((initial_opcode, mut message)) = fragmented.take() else {
                return Err(Error::Startup(
                    "WebSocket continuation has no initial frame".to_owned(),
                ));
            };
            if message.len().saturating_add(payload.len()) > MAX_BODY {
                return Err(Error::Startup("WebSocket message is too large".to_owned()));
            }
            message.extend_from_slice(&payload);
            (initial_opcode, message)
        }
        0x1 | 0x2 => {
            if fragmented.is_some() {
                return Err(Error::Startup(
                    "WebSocket message starts before the previous message ended".to_owned(),
                ));
            }
            (opcode, payload)
        }
        _ => return Err(Error::Startup("invalid WebSocket opcode".to_owned())),
    };
    if final_frame {
        bridge
            .incoming
            .send(WebSocketInbound::Message {
                binary: message_opcode == 0x2,
                payload,
            })
            .map_err(|_| Error::Startup("WebSocket worker closed".to_owned()))?;
    } else {
        *fragmented = Some((message_opcode, payload));
    }
    Ok(())
}

fn flush_websocket_outbound(
    stream: &mut SslStream<TcpStream>,
    outgoing: &Receiver<WebSocketOutbound>,
) -> Result<bool, Error> {
    while let Ok(frame) = outgoing.recv() {
        match frame {
            WebSocketOutbound::Message { binary, payload } => {
                write_websocket_frame(stream, if binary { 0x2 } else { 0x1 }, &payload)?;
            }
            WebSocketOutbound::Close { code, reason } => {
                let payload = websocket_close_payload(code, &reason)?;
                write_websocket_frame(stream, 0x8, &payload)?;
                return Ok(true);
            }
            WebSocketOutbound::Ready => return Ok(false),
        }
    }
    Err(Error::Startup("WebSocket worker closed".to_owned()))
}

fn websocket_close(payload: &[u8]) -> Result<(u16, String), Error> {
    if payload.is_empty() {
        return Ok((1000, String::new()));
    }
    if payload.len() == 1 {
        return Err(Error::Startup(
            "WebSocket close payload is invalid".to_owned(),
        ));
    }
    let code = u16::from_be_bytes([payload[0], payload[1]]);
    if !valid_websocket_close_code(code) {
        return Err(Error::Startup("WebSocket close code is invalid".to_owned()));
    }
    let reason = std::str::from_utf8(&payload[2..])
        .map_err(|_| Error::Startup("WebSocket close reason is invalid".to_owned()))?
        .to_owned();
    Ok((code, reason))
}

fn valid_websocket_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

fn websocket_close_payload(code: u16, reason: &str) -> Result<Vec<u8>, Error> {
    if !valid_websocket_close_code(code) {
        return Err(Error::Startup("WebSocket close code is invalid".to_owned()));
    }
    if reason.len() > 123 {
        return Err(Error::Startup(
            "WebSocket close reason is too long".to_owned(),
        ));
    }
    let mut payload = Vec::with_capacity(2 + reason.len());
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());
    Ok(payload)
}

fn write_websocket_frame(
    stream: &mut SslStream<TcpStream>,
    opcode: u8,
    payload: &[u8],
) -> Result<(), Error> {
    let length = payload.len();
    stream.write_all(&[0x80 | opcode])?;
    if length <= 125 {
        stream.write_all(&[u8::try_from(length)
            .map_err(|_| Error::Startup("WebSocket frame length is invalid".to_owned()))?])?;
    } else if u16::try_from(length).is_ok() {
        stream.write_all(&[126])?;
        stream.write_all(
            &u16::try_from(length)
                .map_err(|_| Error::Startup("WebSocket frame length is invalid".to_owned()))?
                .to_be_bytes(),
        )?;
    } else {
        stream.write_all(&[127])?;
        stream.write_all(&(length as u64).to_be_bytes())?;
    }
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

fn write_plain_response(stream: &mut TcpStream, response: HttpResponse) -> Result<(), Error> {
    write_response_inner(stream, response)
}

fn write_response(stream: &mut SslStream<TcpStream>, response: HttpResponse) -> Result<(), Error> {
    write_response_inner(stream, response)
}

fn write_response_inner(mut stream: impl Write, mut response: HttpResponse) -> Result<(), Error> {
    response
        .headers
        .entry("content-length".to_owned())
        .or_insert_with(|| response.body.len().to_string());
    response
        .headers
        .entry("connection".to_owned())
        .or_insert_with(|| "close".to_owned());
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    };
    write!(stream, "HTTP/1.1 {} {reason}\r\n", response.status)?;
    for (name, value) in response.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
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

fn tls_error(error: &openssl::error::ErrorStack) -> Error {
    Error::Tls(error.to_string())
}

fn report_connection_error(error: &Error) {
    let message = format!("gateway request failed: {error}");
    #[cfg(target_os = "android")]
    {
        let (Ok(tag), Ok(message)) = (CString::new("appd"), CString::new(message)) else {
            return;
        };
        unsafe extern "C" {
            fn __android_log_write(
                priority: std::ffi::c_int,
                tag: *const std::ffi::c_char,
                text: *const std::ffi::c_char,
            ) -> std::ffi::c_int;
        }
        // SAFETY: both strings are valid, NUL-terminated strings for the call.
        unsafe {
            __android_log_write(6, tag.as_ptr(), message.as_ptr());
        }
    }
    #[cfg(not(target_os = "android"))]
    eprintln!("{message}");
}
