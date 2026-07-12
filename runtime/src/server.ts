import fs from "bare-fs";
import * as bareHttps from "bare-https";
import * as bareWs from "bare-ws";

import { AssetService } from "./assets.js";
import { setEnvironment } from "./cloudflare.js";
import { RequestContext } from "./context.js";
import { requestBody, responseBody } from "./streams.js";
import type { RuntimeConfig, WorkerEnvironment, WorkerModule } from "./types.js";
import type { Transport, WorkerWebSocket } from "./websocket.js";

interface IncomingRequest extends AsyncIterable<Uint8Array> {
  readonly headers: Readonly<Record<string, string | string[] | undefined>>;
  readonly method: string;
  readonly url: string;
}

interface ServerResponse {
  end(body?: string | Uint8Array): void;
  once(event: "drain", listener: () => void): void;
  write(body: Uint8Array): boolean;
  writeHead(status: number, headers: Readonly<Record<string, string>>): void;
}

interface RawSocket {
  destroy(error?: Error): void;
  setNoDelay(enable?: boolean): void;
}

interface Server {
  address(): { port: number } | null;
  listen(port: number, host: string, listener: () => void): void;
  ref(): void;
  on(event: "upgrade", listener: UpgradeListener): void;
  once(event: "error", listener: (error: Error) => void): void;
}

type UpgradeListener = (request: IncomingRequest, socket: RawSocket, head: Uint8Array) => void;

interface HttpsModule {
  createServer(
    options: Readonly<Record<string, unknown>>,
    listener: (request: IncomingRequest, response: ServerResponse) => void,
  ): Server;
}

interface WebSocketModule {
  readonly Server: {
    handshake(
      request: IncomingRequest,
      socket: RawSocket,
      head: Uint8Array,
      listener: (error?: Error | null) => void,
    ): void;
  };
  readonly Socket: new (options: { isServer: boolean; socket: RawSocket }) => Transport;
}

const https = bareHttps as unknown as HttpsModule;
const webSocket = bareWs as unknown as WebSocketModule;
let activeServer: Server | undefined;

export async function startServer(config: RuntimeConfig): Promise<number> {
  const environment = createEnvironment(config);
  setEnvironment(environment);
  const module = await import("appd-worker");
  const worker = module.default as WorkerModule;
  const server = https.createServer(tlsOptions(config), (request, response) => {
    void dispatch(request, response, worker, environment);
  });
  server.on("upgrade", (request, socket, head) => {
    void upgrade(request, socket, head, worker, environment);
  });

  await listen(server, config.port);
  server.ref();
  activeServer = server;
  const address = activeServer.address();
  if (address === null) throw new Error("Bare returned no TCP port");
  return address.port;
}

function createEnvironment(config: RuntimeConfig): WorkerEnvironment {
  if (config.assets === undefined) return {};
  const assets = new AssetService(config.assets);
  return { [assets.binding]: assets };
}

function tlsOptions(config: RuntimeConfig): Readonly<Record<string, unknown>> {
  return {
    ca: readBytes(config.certificates.ca),
    cert: readBytes(config.certificates.certificate),
    key: readBytes(config.certificates.privateKey),
    requestCert: true,
    rejectUnauthorized: true,
  };
}

function readBytes(path: string): Uint8Array {
  return new Uint8Array(fs.readFileSync(path) as Uint8Array);
}

async function dispatch(
  incoming: IncomingRequest,
  outgoing: ServerResponse,
  worker: WorkerModule,
  environment: WorkerEnvironment,
): Promise<void> {
  try {
    const { request, context } = workerRequest(incoming);
    const response = await worker.fetch(request, environment, context);
    await writeResponse(outgoing, request, response);
    context.drain();
  } catch (error) {
    writeError(outgoing, error);
  }
}

function workerRequest(incoming: IncomingRequest): {
  context: RequestContext;
  request: Request;
} {
  const init: RequestInit = {
    headers: requestHeaders(incoming.headers),
    method: incoming.method,
  };
  if (incoming.method !== "GET" && incoming.method !== "HEAD") {
    init.body = requestBody(incoming);
  }
  const request = new Request(`https://localhost${incoming.url}`, init);
  return { context: new RequestContext(), request };
}

function requestHeaders(
  headers: Readonly<Record<string, string | string[] | undefined>>,
): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [name, value] of Object.entries(headers)) {
    if (value !== undefined) result[name] = Array.isArray(value) ? value.join(", ") : value;
  }
  return result;
}

async function writeResponse(
  outgoing: ServerResponse,
  request: Request,
  response: Response,
): Promise<void> {
  outgoing.writeHead(response.status, Object.fromEntries(response.headers));
  if (request.method === "HEAD" || response.body === null) {
    outgoing.end();
    return;
  }
  await responseBody(response.body, outgoing);
}

async function upgrade(
  incoming: IncomingRequest,
  socket: RawSocket,
  head: Uint8Array,
  worker: WorkerModule,
  environment: WorkerEnvironment,
): Promise<void> {
  socket.setNoDelay(true);
  try {
    const { request, context } = workerRequest(incoming);
    const response = await worker.fetch(request, environment, context) as Response & {
      webSocket?: WorkerWebSocket;
    };
    if (response.status !== 101 || response.webSocket === undefined) {
      socket.destroy(new Error("Worker rejected WebSocket upgrade"));
      return;
    }
    finishUpgrade(incoming, socket, head, response.webSocket, context);
  } catch (error) {
    socket.destroy(error instanceof Error ? error : new Error(String(error)));
  }
}

function finishUpgrade(
  request: IncomingRequest,
  socket: RawSocket,
  head: Uint8Array,
  workerSocket: WorkerWebSocket,
  context: RequestContext,
): void {
  webSocket.Server.handshake(request, socket, head, (error) => {
    if (error != null) {
      socket.destroy(error);
      return;
    }
    workerSocket.attach(new webSocket.Socket({ isServer: true, socket }));
    context.drain();
  });
}

function listen(server: Server, port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
}

function writeError(response: ServerResponse, error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  response.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
  response.end(`Internal Server Error\n${message}`);
}
