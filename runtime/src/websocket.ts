type Listener = (event: Event) => void;
type TransportListener = (value?: Uint8Array | Error, binary?: boolean) => void;

const CONNECTING = 0;
const OPEN = 1;
const CLOSING = 2;
const CLOSED = 3;

export interface Transport {
  destroy(error?: Error): void;
  end(): void;
  on(event: "message" | "close" | "error", listener: TransportListener): void;
  write(data: string | Uint8Array): void;
}

export class WorkerWebSocket {
  #accepted = false;
  readonly #listeners = new Map<string, Set<Listener>>();
  #peer?: WorkerWebSocket;
  #transport?: Transport;
  #readyState = CONNECTING;
  #closeCode = 1000;
  #closeReason = "";

  onopen: Listener | null = null;
  onmessage: Listener | null = null;
  onclose: Listener | null = null;
  onerror: Listener | null = null;

  static readonly CONNECTING = CONNECTING;
  static readonly OPEN = OPEN;
  static readonly CLOSING = CLOSING;
  static readonly CLOSED = CLOSED;

  get readyState(): number {
    return this.#readyState;
  }

  get bufferedAmount(): number {
    return 0;
  }

  get protocol(): string {
    return "";
  }

  get extensions(): string {
    return "";
  }

  accept(): void {
    if (this.#readyState !== CONNECTING) return;
    this.#accepted = true;
    this.#readyState = OPEN;
    this.dispatch("open", socketEvent("open"));
  }

  addEventListener(type: string, listener: Listener): void {
    const listeners = this.#listeners.get(type) ?? new Set<Listener>();
    listeners.add(listener);
    this.#listeners.set(type, listeners);
  }

  removeEventListener(type: string, listener: Listener): void {
    this.#listeners.get(type)?.delete(listener);
  }

  send(data: string | ArrayBuffer | ArrayBufferView): void {
    if (!this.#accepted || this.#readyState !== OPEN) {
      throw new Error("WebSocket must be accepted and open before sending");
    }
    this.#peer?.deliver(data);
  }

  close(code = 1000, reason = ""): void {
    validateClose(code, reason);
    if (this.#readyState === CLOSING || this.#readyState === CLOSED) return;
    this.#closeCode = code;
    this.#closeReason = reason;
    this.#readyState = CLOSING;
    this.#transport?.end();
    this.#peer?.endTransport();
    this.#peer?.receiveClose(code, reason);
    if (this.#transport === undefined) this.markClosed();
  }

  pairWith(peer: WorkerWebSocket): void {
    this.#peer = peer;
  }

  attach(transport: Transport): void {
    this.#transport = transport;
    transport.on("message", (data, binary) => {
      if (data instanceof Uint8Array) {
        this.#peer?.deliver(binary === true ? data : decodeText(data));
      }
    });
    transport.on("close", () => {
      this.markClosed();
      this.#peer?.receiveClose(this.#closeCode, this.#closeReason);
    });
    transport.on("error", (error) => {
      const event = socketEvent("error", { error });
      this.dispatch("error", event);
      this.#peer?.dispatch("error", event);
    });
  }

  private deliver(data: string | ArrayBuffer | ArrayBufferView): void {
    if (this.#transport !== undefined) {
      this.#transport.write(toTransportData(data));
      return;
    }
    this.dispatch("message", socketEvent("message", { data: toMessageData(data) }));
  }

  private dispatch(type: string, event: Event): void {
    for (const listener of this.#listeners.get(type) ?? []) listener(event);
    this.eventHandler(type)?.(event);
  }

  private eventHandler(type: string): Listener | null {
    switch (type) {
      case "open": return this.onopen;
      case "message": return this.onmessage;
      case "close": return this.onclose;
      case "error": return this.onerror;
      default: return null;
    }
  }

  private endTransport(): void {
    this.#transport?.end();
  }

  private receiveClose(code: number, reason: string): void {
    if (this.#readyState === CLOSED) return;
    this.#closeCode = code;
    this.#closeReason = reason;
    this.#readyState = CLOSING;
    this.markClosed();
  }

  private markClosed(): void {
    if (this.#readyState === CLOSED) return;
    this.#readyState = CLOSED;
    this.dispatch("close", socketEvent("close", {
      code: this.#closeCode,
      reason: this.#closeReason,
      wasClean: true,
    }));
  }
}

export class WebSocketPair {
  readonly 0: WorkerWebSocket;
  readonly 1: WorkerWebSocket;

  constructor() {
    this[0] = new WorkerWebSocket();
    this[1] = new WorkerWebSocket();
    this[0].pairWith(this[1]);
    this[1].pairWith(this[0]);
  }
}

function toTransportData(data: string | ArrayBuffer | ArrayBufferView): string | Uint8Array {
  if (typeof data === "string") return data;
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return new Uint8Array(data);
}

function decodeText(data: Uint8Array): string {
  return Buffer.from(data.buffer, data.byteOffset, data.byteLength).toString("utf8");
}

function toMessageData(data: string | ArrayBuffer | ArrayBufferView): string | ArrayBuffer {
  if (typeof data === "string") return data;
  if (data instanceof ArrayBuffer) return data;
  if (data.buffer instanceof ArrayBuffer) {
    if (data.byteOffset === 0 && data.byteLength === data.buffer.byteLength) {
      return data.buffer;
    }
    return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
  }
  return new Uint8Array(data.buffer, data.byteOffset, data.byteLength).slice().buffer;
}

function socketEvent(type: string, fields: Record<string, unknown> = {}): Event {
  return { type, ...fields } as unknown as Event;
}

function validateClose(code: number, reason: string): void {
  const validCode = code === 1000
    || (code >= 1001 && code <= 1003)
    || (code >= 1007 && code <= 1011)
    || (code >= 3000 && code <= 4999);
  if (!validCode) throw new RangeError("Invalid WebSocket close code");
  if (Buffer.byteLength(reason, "utf8") > 123) {
    throw new RangeError("WebSocket close reason is too long");
  }
}
