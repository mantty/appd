use std::collections::BTreeMap;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

use crate::fs::VirtualFileSystem;
use crate::fs::{
    MODULE_NAME as NODE_FS_MODULE_NAME, NodeFsModule, NodeFsPromisesModule,
    PROMISES_MODULE_NAME as NODE_FS_PROMISES_MODULE_NAME, install,
};
use crate::gateway::{
    Execution, Handler, Job, JobResponse, WebSocketInbound, WebSocketJob, WebSocketOutbound,
};
use crate::quickjs::{Error, RuntimeConfig, WorkerBundle};
use crate::transport::{HttpRequest, HttpResponse};
use flate2::read::GzDecoder;
use rquickjs::loader::{BuiltinResolver, ImportAttributes, Loader, ModuleLoader, Resolver};
use rquickjs::{
    Array, ArrayBuffer, Context, Function, Module, Object, Promise, Runtime as JsRuntime,
};

/// Dispatches packaged application requests through `QuickJS`.
pub(crate) struct Dispatcher {
    worker: Arc<WorkerBundle>,
    config: RuntimeConfig,
}

impl Dispatcher {
    pub(crate) fn new(bundle: WorkerBundle, config: RuntimeConfig) -> Arc<Self> {
        Arc::new(Self {
            worker: Arc::new(bundle),
            config,
        })
    }
}

impl Handler for Dispatcher {
    fn handle(
        &self,
        job: Job,
        execution: &Execution<'_>,
        accepting: &Arc<AtomicBool>,
    ) -> Result<(), Error> {
        let response = job.response.clone();
        if let Some(asset) = asset_response(&self.config, &job.request)? {
            response
                .send(JobResponse::Http(asset))
                .map_err(|_| Error::Startup("HTTP response receiver closed".to_owned()))?;
            return Ok(());
        }
        execute_request(&self.worker, &self.config, job, execution, accepting)
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
        let resolved = crate::quickjs::resolve_module_name(base, name);
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
