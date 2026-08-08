import Buffer from "bare-buffer";
import EventEmitter from "bare-events";
import Stream from "bare-stream";

type HeaderValue = string | number | readonly string[];
type RequestListener = (request: IncomingMessage, response: ServerResponse) => void;
type ListenCallback = () => void;

interface ListenOptions {
  readonly port?: number;
}

interface ServerSocket {
  readonly encrypted: boolean;
  readonly localAddress: string;
  readonly localPort: number;
  readonly remoteAddress: string;
  readonly remoteFamily: "IPv4";
  readonly remotePort: number;
  destroy(): void;
}

const servers = new Map<number, Server>();
let nextPort = 10_000;

export class IncomingMessage extends Stream.Readable {
  complete: boolean;
  readonly headers: Record<string, string | string[]>;
  readonly httpVersion = "1.1";
  readonly httpVersionMajor = 1;
  readonly httpVersionMinor = 1;
  readonly method: string;
  readonly rawHeaders: string[];
  readonly socket: ServerSocket;
  readonly url: string;
  #reading = false;
  readonly #reader: ReadableStreamDefaultReader<Uint8Array> | undefined;

  constructor(request: Request, port: number) {
    super();
    const url = new URL(request.url);
    this.headers = headersFrom(request.headers);
    this.method = request.method;
    this.rawHeaders = rawHeaders(this.headers);
    this.socket = socketFor(request, port);
    this.url = `${url.pathname}${url.search}`;
    this.#reader = request.body?.getReader();
    this.complete = this.#reader === undefined;
  }

  override _read(): void {
    if (!this.#reading) void this.readBody();
  }

  private async readBody(): Promise<void> {
    if (this.#reader === undefined) {
      this.push(null);
      return;
    }
    this.#reading = true;
    try {
      while (true) {
        const result = await this.#reader.read();
        if (result.done) {
          this.complete = true;
          this.push(null);
          return;
        }
        if (!this.push(Buffer.from(result.value))) return;
      }
    } catch (error) {
      this.destroy(asError(error));
    } finally {
      this.#reading = false;
    }
  }
}

export class ServerResponse extends Stream.Writable {
  #headers = new Headers();
  #headersSent = false;
  #hasBody = true;
  #bodyController!: ReadableStreamDefaultController<Uint8Array>;
  readonly #body = new ReadableStream<Uint8Array>({
    start: (controller) => { this.#bodyController = controller; },
  });
  #resolve!: (response: Response) => void;
  readonly #method: string;
  readonly response = new Promise<Response>((resolve) => { this.#resolve = resolve; });
  statusCode = 200;
  statusMessage = "";

  constructor(method: string) {
    super();
    this.#method = method;
  }

  get headersSent(): boolean {
    return this.#headersSent;
  }

  getHeader(name: string): string | undefined {
    return this.#headers.get(name) ?? undefined;
  }

  getHeaderNames(): string[] {
    return [...this.#headers.keys()];
  }

  getHeaders(): Record<string, string | string[]> {
    return headersFrom(this.#headers);
  }

  hasHeader(name: string): boolean {
    return this.#headers.has(name);
  }

  removeHeader(name: string): void {
    this.assertHeadersMutable();
    this.#headers.delete(name);
  }

  setHeader(name: string, value: HeaderValue): this {
    this.assertHeadersMutable();
    this.#headers.delete(name);
    for (const entry of Array.isArray(value) ? value : [value]) this.#headers.append(name, String(entry));
    return this;
  }

  writeHead(statusCode: number, statusMessage?: string | Record<string, HeaderValue>, headers?: Record<string, HeaderValue>): this {
    this.statusCode = statusCode;
    if (typeof statusMessage === "string") this.statusMessage = statusMessage;
    const values = typeof statusMessage === "object" ? statusMessage : headers;
    for (const [name, value] of Object.entries(values ?? {})) this.setHeader(name, value);
    this.sendHeaders();
    return this;
  }

  override _final(callback: (error: Error | null) => void): void {
    this.sendHeaders();
    if (this.#hasBody) this.#bodyController.close();
    callback(null);
  }

  override _write(data: unknown, _encoding: string, callback: (error: Error | null) => void): void {
    try {
      this.sendHeaders();
      if (this.#hasBody) this.#bodyController.enqueue(toBytes(data));
      callback(null);
    } catch (error) {
      callback(asError(error));
    }
  }

  private assertHeadersMutable(): void {
    if (this.#headersSent) throw new Error("Cannot modify headers after they are sent");
  }

  private sendHeaders(): void {
    if (this.#headersSent) return;
    this.#headersSent = true;
    this.#hasBody = this.#method !== "HEAD" && this.statusCode !== 204 && this.statusCode !== 304;
    this.#resolve(new Response(this.#hasBody ? this.#body : null, {
      headers: this.#headers,
      status: this.statusCode,
      statusText: this.statusMessage,
    }));
  }
}

export class Server extends EventEmitter {
  #port: number | undefined;
  #closed = false;

  constructor(listener?: RequestListener) {
    super();
    if (listener !== undefined) this.on("request", listener as (...arguments_: unknown[]) => void);
  }

  address(): { address: string; family: "IPv4"; port: number } | null {
    if (this.#port === undefined) return null;
    return { address: "127.0.0.1", family: "IPv4", port: this.#port };
  }

  close(callback?: (error?: Error) => void): this {
    if (this.#port !== undefined) servers.delete(this.#port);
    this.#port = undefined;
    this.#closed = true;
    callback?.();
    this.emit("close");
    return this;
  }

  listen(...arguments_: unknown[]): this {
    const { callback, port } = listenOptions(arguments_);
    if (this.#port !== undefined) throw new Error("Server is already listening");
    const assigned = port === 0 ? availablePort() : port;
    if (!Number.isInteger(assigned) || assigned < 0 || assigned > 65_535) {
      throw new RangeError("The port must be an integer between 0 and 65535");
    }
    if (servers.has(assigned)) throw new Error(`Port ${assigned} is already registered`);
    this.#closed = false;
    this.#port = assigned;
    servers.set(assigned, this);
    callback?.();
    this.emit("listening");
    return this;
  }

  async dispatch(request: Request): Promise<Response> {
    if (this.#closed || this.#port === undefined) throw new Error("Server is not listening");
    const incoming = new IncomingMessage(request, this.#port);
    const outgoing = new ServerResponse(incoming.method);
    try {
      this.emit("request", incoming, outgoing);
    } catch (error) {
      outgoing.statusCode = 500;
      outgoing.end("Internal Server Error\n");
      throw error;
    }
    return outgoing.response;
  }
}

export function createServer(listener?: RequestListener): Server {
  return new Server(listener);
}

export function handleAsNodeRequest(port: number, request: Request): Promise<Response> {
  const server = servers.get(port);
  if (server === undefined) throw new Error(`No Node HTTP server is listening on port ${port}`);
  return server.dispatch(request);
}

export function httpServerHandler(serverOrOptions: Server | ListenOptions): { fetch(request: Request): Promise<Response> } {
  if (serverOrOptions instanceof Server) {
    if (serverOrOptions.address() === null) serverOrOptions.listen(0);
    return { fetch: (request) => serverOrOptions.dispatch(request) };
  }
  if (serverOrOptions.port === undefined) throw new TypeError("httpServerHandler requires a server or port");
  return { fetch: (request) => handleAsNodeRequest(serverOrOptions.port!, request) };
}

function availablePort(): number {
  while (servers.has(nextPort)) nextPort += 1;
  return nextPort++;
}

function headersFrom(headers: Headers): Record<string, string | string[]> {
  const result: Record<string, string | string[]> = Object.fromEntries(headers.entries());
  const cookies = (headers as Headers & { getSetCookie?: () => string[] }).getSetCookie?.() ?? [];
  if (cookies.length > 0) result["set-cookie"] = cookies;
  return result;
}

function listenOptions(arguments_: unknown[]): { callback: ListenCallback | undefined; port: number } {
  const values = arguments_.slice();
  const callback = typeof values.at(-1) === "function" ? values.pop() as ListenCallback : undefined;
  const input = values[0];
  if (typeof input === "number") return { callback, port: input };
  if (typeof input === "object" && input !== null) return { callback, port: (input as ListenOptions).port ?? 0 };
  return { callback, port: 0 };
}

function rawHeaders(headers: Record<string, string | string[]>): string[] {
  return Object.entries(headers).flatMap(([name, value]) => [name, ...(Array.isArray(value) ? value : [value])]);
}

function socketFor(request: Request, port: number): ServerSocket {
  const url = new URL(request.url);
  return {
    encrypted: url.protocol === "https:",
    localAddress: request.headers.get("host") ?? "127.0.0.1",
    localPort: port,
    remoteAddress: "127.0.0.1",
    remoteFamily: "IPv4",
    remotePort: 32_768,
    destroy(): void {},
  };
}

function toBytes(data: unknown): Uint8Array {
  if (typeof data === "string") return Buffer.from(data);
  if (data instanceof Uint8Array) return Buffer.from(data);
  throw new TypeError("HTTP response bodies must contain strings or byte arrays");
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
