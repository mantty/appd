type Listener = (event: MessageEvent | Event) => void;
type TransportListener = (value?: Uint8Array | Error, binary?: boolean) => void;

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

  accept(): void {
    this.#accepted = true;
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
    if (!this.#accepted) throw new Error("WebSocket must be accepted before sending");
    this.#peer?.deliver(data);
  }

  close(): void {
    this.#transport?.end();
    this.#peer?.endTransport();
    this.dispatch("close", socketEvent("close"));
    this.#peer?.dispatch("close", socketEvent("close"));
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
    transport.on("close", () => this.#peer?.dispatch("close", socketEvent("close")));
    transport.on("error", (error) => this.#peer?.dispatch("error", socketEvent("error", {
      error,
    })));
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
  }

  private endTransport(): void {
    this.#transport?.end();
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
