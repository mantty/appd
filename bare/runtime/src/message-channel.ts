import { Event } from "bare-events/web";

type Listener = (event: MessageEvent) => void;

interface MessageEventInit {
  readonly data?: unknown;
  readonly lastEventId?: string;
  readonly origin?: string;
  readonly ports?: readonly MessagePort[];
  readonly source?: MessagePort | null;
}

export class MessageEvent extends Event {
  readonly data: unknown;
  readonly lastEventId: string;
  readonly origin: string;
  readonly ports: readonly MessagePort[];
  readonly source: MessagePort | null;

  constructor(type: string, options: MessageEventInit = {}) {
    super(type);
    this.data = options.data;
    this.lastEventId = options.lastEventId ?? "";
    this.origin = options.origin ?? "";
    this.ports = options.ports ?? [];
    this.source = options.source ?? null;
  }
}

export class MessagePort {
  #closed = false;
  #listener: Listener | null = null;
  readonly #listeners = new Set<Listener>();
  readonly #queue: MessageEvent[] = [];
  #scheduled = false;
  #started = false;
  #peer: MessagePort | undefined;

  get onmessage(): Listener | null {
    return this.#listener;
  }

  set onmessage(listener: Listener | null) {
    this.#listener = listener;
    if (listener !== null) this.start();
  }

  get onmessageerror(): null {
    return null;
  }

  set onmessageerror(_listener: null) {}

  addEventListener(type: string, listener: Listener): void {
    if (type !== "message") return;
    this.#listeners.add(listener);
  }

  close(): void {
    this.#closed = true;
    this.#queue.splice(0);
  }

  postMessage(value: unknown, transfer: Transferable[] = []): void {
    if (this.#closed || this.#peer === undefined || this.#peer.#closed) return;
    const data = structuredClone(value, { transfer });
    this.#peer.enqueue(new MessageEvent("message", { data }));
  }

  removeEventListener(type: string, listener: Listener): void {
    if (type === "message") this.#listeners.delete(listener);
  }

  start(): void {
    this.#started = true;
    this.schedule();
  }

  static pair(): readonly [MessagePort, MessagePort] {
    const first = new MessagePort();
    const second = new MessagePort();
    first.#peer = second;
    second.#peer = first;
    return [first, second];
  }

  private enqueue(event: MessageEvent): void {
    this.#queue.push(event);
    this.schedule();
  }

  private schedule(): void {
    if (!this.#started || this.#scheduled || this.#closed) return;
    this.#scheduled = true;
    queueMicrotask(() => {
      this.#scheduled = false;
      this.dispatch();
    });
  }

  private dispatch(): void {
    while (this.#queue.length > 0 && !this.#closed) {
      const event = this.#queue.shift()!;
      this.#listener?.(event);
      for (const listener of this.#listeners) listener(event);
    }
  }
}

export class MessageChannel {
  readonly port1: MessagePort;
  readonly port2: MessagePort;

  constructor() {
    [this.port1, this.port2] = MessagePort.pair();
  }
}
