import fs from "bare-fs";
import * as bareHttp from "bare-http1";
import * as bareTcp from "bare-tcp";
import * as bareTls from "bare-tls";
import * as bareWs from "bare-ws";

import { AssetService } from "./assets.js";
import { configureCaches } from "./cache.js";
import { setEnvironment, setWorkerExports } from "./cloudflare.js";
import { setProcessEnvironment } from "./globals.js";
import { RequestContext } from "./context.js";
import { parseConnectRequest, type ConnectRequest } from "./proxy.js";
import { responseHeaders, writeResponse } from "./responses.js";
import { SocketResponse } from "./socket-response.js";
import { requestBody } from "./streams.js";
import { writeUpgradeResponse } from "./upgrade-response.js";
import { invokeWorker } from "./worker.js";
import type { RuntimeConfig, WorkerEnvironment, WorkerExport } from "./types.js";
import type { Transport, WorkerWebSocket } from "./websocket.js";

interface IncomingRequest extends AsyncIterable<Uint8Array> {
  readonly headers: Readonly<Record<string, string | string[] | undefined>>;
  readonly method: string;
  readonly url: string;
}

interface ServerResponse {
  readonly socket: Socket;
  end(body?: string | Uint8Array): void;
  write(body: Uint8Array, callback?: (error?: Error | null) => void): boolean;
  writeHead(status: number, headers: Readonly<Record<string, string | string[]>>): void;
}

interface Socket {
  destroy(error?: Error): void;
  end(data?: string | Uint8Array): void;
  on(event: "data", listener: (chunk: Uint8Array) => void): this;
  on(event: "error", listener: (error: Error) => void): this;
  off(event: "data", listener: (chunk: Uint8Array) => void): this;
  unshift(data: Uint8Array): void;
  once(event: "drain" | "timeout", listener: () => void): this;
  once(event: "error", listener: (error: Error) => void): this;
  pipe(destination: Socket): Socket;
  setNoDelay(enable?: boolean): void;
  setTimeout(milliseconds: number, listener?: () => void): this;
  write(data: string | Uint8Array): boolean;
}

interface Listener {
  address(): { port: number } | null;
  listen(port: number, host: string, listener: () => void): void;
  on(event: "connection", listener: (socket: Socket) => void): void;
  once(event: "error", listener: (error: Error) => void): void;
  ref(): void;
}

interface HttpServer {
  on(event: "request", listener: (request: IncomingRequest, response: ServerResponse) => void): void;
  on(event: "upgrade", listener: (request: IncomingRequest, socket: Socket, head: Uint8Array) => void): void;
}

interface Invocation {
  readonly environment: WorkerEnvironment;
  readonly host: string;
  readonly worker: WorkerExport;
}

interface WebSocketModule {
  readonly Server: {
    handshake(
      request: IncomingRequest,
      socket: Socket,
      head: Uint8Array,
      listener: (error?: Error | null) => void,
    ): void;
  };
  readonly Socket: new (options: { isServer: boolean; socket: Socket }) => Transport;
}

const tcp = bareTcp as unknown as { Server: new (options: object) => Listener };
const http = bareHttp as unknown as { Server: new () => HttpServer; ServerConnection: new (server: HttpServer, socket: Socket) => unknown };
const webSocket = bareWs as unknown as WebSocketModule;
const tls = bareTls as unknown as { Socket: new (socket: Socket, options: Readonly<Record<string, unknown>>) => Socket };
let activeServer: Listener | undefined;
const CONNECT_TIMEOUT = 5_000;

export async function startServer(config: RuntimeConfig): Promise<number> {
  configureCaches(config.cache);
  const environment = createEnvironment(config);
  setEnvironment(environment);
  setProcessEnvironment(config.environment);
  const module = await import("appd-worker");
  setWorkerExports(module);
  const worker = module.default as WorkerExport;
  const httpServer = new http.Server();
  httpServer.on("request", (request, response) => {
    void dispatch(request, response, { worker, environment, host: config.host });
  });
  httpServer.on("upgrade", (request, socket, head) => {
    void upgrade(request, socket, head, { worker, environment, host: config.host });
  });

  const server = new tcp.Server({ allowHalfOpen: false });
  server.on("connection", (socket) => {
    handleProxyConnection(socket, httpServer, config);
  });
  await listen(server, config.port);
  server.ref();
  activeServer = server;
  const address = activeServer.address();
  if (address === null) throw new Error("Bare returned no TCP port");
  return address.port;
}

function createEnvironment(config: RuntimeConfig): WorkerEnvironment {
  if (config.assets === undefined) return { ...config.environment };
  const assets = new AssetService(config.assets);
  return { ...config.environment, [assets.binding]: assets };
}

function handleProxyConnection(
  socket: Socket,
  httpServer: HttpServer,
  config: RuntimeConfig,
): void {
  socket.setNoDelay(true);
  let buffered: Uint8Array = new Uint8Array(0);
  socket.setTimeout(CONNECT_TIMEOUT, () => {
    socket.off("data", onData);
    socket.destroy(new Error("Proxy CONNECT timed out"));
  });
  const onData = (chunk: Uint8Array) => {
    try {
      buffered = appendBytes(buffered, chunk);
      const result = parseConnectRequest(buffered);
      if (result === null) return;
      socket.off("data", onData);
      socket.setTimeout(0);
      routeProxyConnection(socket, result, httpServer, config);
    } catch (error) {
      socket.off("data", onData);
      socket.write("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
      socket.destroy(error instanceof Error ? error : new Error(String(error)));
    }
  };
  socket.on("data", onData);
  socket.once("error", () => { socket.destroy(); });
}

function routeProxyConnection(
  socket: Socket,
  request: ConnectRequest,
  httpServer: HttpServer,
  config: RuntimeConfig,
): void {
  if (request.host !== config.host || request.port !== 443) {
    socket.write("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");
    socket.destroy();
    return;
  }
  socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
  const identity = readBytes(config.certificates.identity);
  const tlsOptions: Record<string, unknown> = {
    cert: identity,
    key: identity,
    isServer: true,
    rejectUnauthorized: config.requireClientCertificate,
    requestCert: config.requireClientCertificate,
  };
  if (config.requireClientCertificate) {
    tlsOptions.ca = readBytes(config.certificates.ca);
  }
  if (request.remainder.byteLength > 0) socket.unshift(request.remainder);
  const secure = new tls.Socket(socket, tlsOptions);
  new http.ServerConnection(httpServer, secure);
}

function appendBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
  const result = new Uint8Array(left.byteLength + right.byteLength);
  result.set(left);
  result.set(right, left.byteLength);
  return result;
}

function readBytes(path: string): Uint8Array {
  return new Uint8Array(fs.readFileSync(path) as Uint8Array);
}

async function dispatch(
  incoming: IncomingRequest,
  outgoing: ServerResponse,
  invocation: Invocation,
): Promise<void> {
  let context: RequestContext | undefined;
  try {
    const result = workerRequest(incoming, invocation.host);
    context = result.context;
    const response = await invokeWorker(invocation.worker, result.request, invocation.environment, context);
    await writeWorkerResponse(outgoing, result.request, response);
  } catch (error) {
    writeError(outgoing, error);
  } finally {
    await context?.drain();
  }
}

function writeWorkerResponse(outgoing: ServerResponse, request: Request, response: Response): Promise<void> {
  const headers = responseHeaders(response);
  const writer = hasMultipleCookies(headers)
    ? new SocketResponse(outgoing.socket, request.method, response.statusText, response.body !== null)
    : outgoing;
  return writeResponse(writer, request, response, headers);
}

function hasMultipleCookies(headers: Readonly<Record<string, string | string[]>>): boolean {
  return Array.isArray(headers["set-cookie"]);
}

function workerRequest(incoming: IncomingRequest, host: string): { context: RequestContext; request: Request } {
  const init: RequestInit = {
    headers: requestHeaders(incoming.headers),
    method: incoming.method,
  };
  if (incoming.method !== "GET" && incoming.method !== "HEAD") init.body = requestBody(incoming);
  const request = new Request(new URL(incoming.url, `https://${host}`), init);
  return { context: new RequestContext(), request };
}

function requestHeaders(headers: Readonly<Record<string, string | string[] | undefined>>): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [name, value] of Object.entries(headers)) {
    if (value !== undefined) result[name] = Array.isArray(value) ? value.join(", ") : value;
  }
  return result;
}

async function upgrade(
  incoming: IncomingRequest,
  socket: Socket,
  head: Uint8Array,
  invocation: Invocation,
): Promise<void> {
  let context: RequestContext | undefined;
  let response: (Response & { webSocket?: WorkerWebSocket }) | undefined;
  try {
    const result = workerRequest(incoming, invocation.host);
    context = result.context;
    response = await invokeWorker(invocation.worker, result.request, invocation.environment, context);
    if (response.status !== 101 || response.webSocket === undefined) {
      await writeUpgradeResponse(socket, result.request, response);
      return;
    }
    await finishUpgrade(incoming, socket, head, response.webSocket);
  } catch (error) {
    if (response === undefined) {
      console.error("Worker WebSocket upgrade failed", error);
      const fallback = new Request(`https://${invocation.host}`, { method: incoming.method });
      await writeUpgradeResponse(socket, fallback, new Response("Internal Server Error\n", {
        status: 500,
        headers: { "content-type": "text/plain; charset=utf-8" },
      }));
    } else {
      socket.destroy();
    }
  } finally {
    await context?.drain();
  }
}

function finishUpgrade(request: IncomingRequest, socket: Socket, head: Uint8Array, workerSocket: WorkerWebSocket): Promise<void> {
  return new Promise((resolve, reject) => {
    webSocket.Server.handshake(request, socket, head, (error) => {
      if (error != null) {
        reject(error);
        return;
      }
      workerSocket.attach(new webSocket.Socket({ isServer: true, socket }));
      resolve();
    });
  });
}

function listen(server: Listener, port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
}

function writeError(response: ServerResponse, error: unknown): void {
  if (error instanceof Error) console.error("Worker request failed", error);
  else console.error("Worker request failed", String(error));
  response.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
  response.end("Internal Server Error\n");
}
