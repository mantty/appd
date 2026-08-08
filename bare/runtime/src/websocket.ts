import { Event, EventTarget } from "bare-events/web";

import { MessageEvent } from "./message-channel.js";

type Listener = (event: Event) => void;
type MessageListener = (event: MessageEvent) => void;
type CloseListener = (event: SocketCloseEvent) => void;
type ErrorListener = (event: SocketErrorEvent) => void;
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

export class WorkerWebSocket extends EventTarget {
  #accepted = false;
  #peer?: WorkerWebSocket;
  #transport?: Transport;
  #readyState = CONNECTING;
  #closeCode = 1000;
  #closeReason = "";

  onopen: Listener | null = null;
  onmessage: MessageListener | null = null;
  onclose: CloseListener | null = null;
  onerror: ErrorListener | null = null;

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
    this.emit(new Event("open"));
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
      const failure = error instanceof Error ? error : new Error("WebSocket transport failed");
      this.emit(new SocketErrorEvent(failure));
      this.#peer?.emit(new SocketErrorEvent(failure));
    });
  }

  private deliver(data: string | ArrayBuffer | ArrayBufferView): void {
    if (this.#transport !== undefined) {
      this.#transport.write(toTransportData(data));
      return;
    }
    this.emit(new MessageEvent("message", { data: toMessageData(data) }));
  }

  private emit(event: Event): void {
    this.dispatchEvent(event);
    switch (event.type) {
      case "open": this.onopen?.(event); break;
      case "message": this.onmessage?.(event as MessageEvent); break;
      case "close": this.onclose?.(event as SocketCloseEvent); break;
      case "error": this.onerror?.(event as SocketErrorEvent); break;
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
    this.emit(new SocketCloseEvent(this.#closeCode, this.#closeReason));
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

class SocketErrorEvent extends Event {
  readonly error: Error;

  constructor(error: Error) {
    super("error");
    this.error = error;
  }
}

class SocketCloseEvent extends Event {
  readonly code: number;
  readonly reason: string;
  readonly wasClean = true;

  constructor(code: number, reason: string) {
    super("close");
    this.code = code;
    this.reason = reason;
  }
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
