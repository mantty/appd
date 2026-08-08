import Buffer from "bare-buffer";
import Stream from "bare-stream";

type HeaderValue = string | readonly string[] | number | undefined;

interface RequestOptions {
  readonly auth?: string;
  readonly headers?: Readonly<Record<string, HeaderValue>>;
  readonly host?: string;
  readonly hostname?: string;
  readonly method?: string;
  readonly path?: string;
  readonly port?: number;
  readonly protocol?: string;
  readonly signal?: AbortSignal;
}

type RequestInput = string | URL | RequestOptions | undefined;
type ResponseListener = (response: IncomingMessage) => void;
type EventEmitter = { emit(event: string, ...arguments_: unknown[]): boolean; on(event: string, listener: (...arguments_: unknown[]) => void): unknown };

export class IncomingMessage extends Stream.Readable {
  complete: boolean;
  readonly headers: Record<string, string | string[]>;
  readonly httpVersion = "1.1";
  readonly httpVersionMajor = 1;
  readonly httpVersionMinor = 1;
  readonly rawHeaders: string[];
  readonly statusCode: number;
  readonly statusMessage: string;
  readonly url: string;
  readonly #reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  #reading = false;

  constructor(response: Response) {
    super();
    this.headers = headersFrom(response.headers);
    this.rawHeaders = rawHeaders(this.headers);
    this.statusCode = response.status;
    this.statusMessage = response.statusText;
    this.url = response.url;
    this.#reader = response.body?.getReader();
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
      this.destroy(error instanceof Error ? error : new Error(String(error)));
    } finally {
      this.#reading = false;
    }
  }
}

export class ClientRequest extends Stream.Writable {
  #body: TransformStream<Uint8Array> | undefined;
  #bodyWriter: WritableStreamDefaultWriter<Uint8Array> | undefined;
  readonly #headers: Headers;
  readonly #method: string;
  readonly #controller: AbortController;
  readonly #signal: AbortSignal;
  #started = false;
  readonly #url: URL;

  constructor(url: URL, options: RequestOptions, callback: ResponseListener | undefined) {
    super();
    this.#headers = headersFor(options.headers, options.auth);
    this.#method = options.method?.toUpperCase() ?? "GET";
    this.#controller = new AbortController();
    this.#signal = combinedSignal(options.signal, this.#controller.signal);
    this.#url = url;
    if (callback !== undefined) events(this).on("response", callback as (...arguments_: unknown[]) => void);
  }

  get method(): string {
    return this.#method;
  }

  get path(): string {
    return `${this.#url.pathname}${this.#url.search}`;
  }

  get protocol(): string {
    return this.#url.protocol;
  }

  get host(): string {
    return this.#url.host;
  }

  get hostname(): string {
    return this.#url.hostname;
  }

  get port(): string {
    return this.#url.port;
  }

  abort(): void {
    this.#controller.abort();
    this.destroy(new Error("The request was aborted"));
  }

  flushHeaders(): void {
    this.start();
  }

  getHeader(name: string): string | undefined {
    return this.#headers.get(name) ?? undefined;
  }

  getHeaderNames(): string[] {
    return [...this.#headers.keys()];
  }

  getHeaders(): Record<string, string> {
    return Object.fromEntries(this.#headers.entries());
  }

  hasHeader(name: string): boolean {
    return this.#headers.has(name);
  }

  removeHeader(name: string): void {
    this.assertHeadersMutable();
    this.#headers.delete(name);
  }

  setHeader(name: string, value: HeaderValue): void {
    this.assertHeadersMutable();
    if (value === undefined) this.#headers.delete(name);
    else this.#headers.set(name, Array.isArray(value) ? value.join(", ") : String(value));
  }

  override _write(data: unknown, _encoding: string, callback: (error: Error | null) => void): void {
    if (this.#method === "GET" || this.#method === "HEAD") {
      callback(new TypeError(`${this.#method} requests cannot have a body`));
      return;
    }
    this.start();
    void this.#bodyWriter?.write(toBytes(data)).then(
      () => { callback(null); },
      (error: unknown) => { callback(asError(error)); },
    );
  }

  override _final(callback: (error: Error | null) => void): void {
    this.start();
    if (this.#bodyWriter === undefined) {
      callback(null);
      return;
    }
    void this.#bodyWriter.close().then(
      () => { callback(null); },
      (error: unknown) => { callback(asError(error)); },
    );
  }

  private assertHeadersMutable(): void {
    if (this.#started) throw new Error("Cannot modify headers after the request starts");
  }

  private start(): void {
    if (this.#started) return;
    this.#started = true;
    const init: RequestInit = {
      headers: this.#headers,
      method: this.#method,
      signal: this.#signal,
    };
    if (this.#method !== "GET" && this.#method !== "HEAD") init.body = this.createBody();
    void globalThis.fetch(this.#url, init).then(
      (response) => { events(this).emit("response", new IncomingMessage(response)); },
      (error: unknown) => { this.destroy(asError(error)); },
    );
  }

  private createBody(): ReadableStream<Uint8Array> {
    this.#body ??= new TransformStream<Uint8Array>();
    this.#bodyWriter ??= this.#body.writable.getWriter();
    return this.#body.readable;
  }
}

export function request(defaultProtocol: "http:" | "https:", arguments_: unknown[]): ClientRequest {
  const { callback, options, url } = requestOptions(defaultProtocol, arguments_);
  return new ClientRequest(url, options, callback);
}

export function get(defaultProtocol: "http:" | "https:", arguments_: unknown[]): ClientRequest {
  const client = request(defaultProtocol, arguments_);
  client.end();
  return client;
}

function requestOptions(defaultProtocol: "http:" | "https:", arguments_: unknown[]): {
  callback: ResponseListener | undefined;
  options: RequestOptions;
  url: URL;
} {
  const values = arguments_.slice();
  const last = values.at(-1);
  const callback = typeof last === "function" ? values.pop() as ResponseListener : undefined;
  const input = values.shift() as RequestInput;
  const inputIsUrl = typeof input === "string" || input instanceof URL;
  const options = {
    ...(inputIsUrl ? {} : input),
    ...(values.shift() as RequestOptions | undefined),
  };
  const url = inputIsUrl
    ? applyOptions(new URL(input), options)
    : urlFromOptions(defaultProtocol, options);
  return { callback, options, url };
}

function urlFromOptions(defaultProtocol: "http:" | "https:", options: RequestOptions): URL {
  const protocol = options.protocol ?? defaultProtocol;
  const hostname = options.hostname ?? options.host ?? "localhost";
  const authority = options.port === undefined ? hostname : `${hostname}:${options.port}`;
  return new URL(options.path ?? "/", `${protocol}//${authority}`);
}

function applyOptions(url: URL, options: RequestOptions): URL {
  const result = new URL(url);
  if (options.protocol !== undefined) result.protocol = options.protocol;
  if (options.hostname !== undefined) result.hostname = options.hostname;
  else if (options.host !== undefined) {
    const authority = new URL(`${result.protocol}//${options.host}`);
    result.hostname = authority.hostname;
    result.port = authority.port;
  }
  if (options.port !== undefined) result.port = String(options.port);
  if (options.path !== undefined) return new URL(options.path, result);
  return result;
}

function headersFor(headers: RequestOptions["headers"], auth: string | undefined): Headers {
  const result = new Headers();
  for (const [name, value] of Object.entries(headers ?? {})) {
    if (value !== undefined) result.set(name, Array.isArray(value) ? value.join(", ") : String(value));
  }
  if (auth !== undefined && !result.has("authorization")) {
    result.set("authorization", `Basic ${Buffer.from(auth).toString("base64")}`);
  }
  return result;
}

function headersFrom(headers: Headers): Record<string, string | string[]> {
  const result: Record<string, string | string[]> = Object.fromEntries(headers.entries());
  const cookies = (headers as Headers & { getSetCookie?: () => string[] }).getSetCookie?.() ?? [];
  if (cookies.length > 0) result["set-cookie"] = cookies;
  return result;
}

function rawHeaders(headers: Record<string, string | string[]>): string[] {
  return Object.entries(headers).flatMap(([name, value]) => [name, ...(Array.isArray(value) ? value : [value])]);
}

function toBytes(data: unknown): Uint8Array {
  if (typeof data === "string") return Buffer.from(data);
  if (data instanceof Uint8Array) return Buffer.from(data);
  throw new TypeError("HTTP request bodies must contain strings or byte arrays");
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function combinedSignal(external: AbortSignal | undefined, internal: AbortSignal): AbortSignal {
  if (external === undefined) return internal;
  if (typeof AbortSignal.any === "function") return AbortSignal.any([external, internal]);
  const controller = new AbortController();
  const abort = () => { controller.abort(); };
  external.addEventListener("abort", abort, { once: true });
  internal.addEventListener("abort", abort, { once: true });
  return controller.signal;
}

function events(value: unknown): EventEmitter {
  return value as EventEmitter;
}
