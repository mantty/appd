import { Event, EventTarget } from "bare-events/web";

import { MessageEvent } from "./message-channel.js";
import { scheduler } from "./scheduler.js";
import type { Fetcher } from "./types.js";

interface EventSourceInit {
  readonly fetcher?: Fetcher;
  readonly withCredentials?: boolean;
}

type EventHandler = ((event: Event) => void) | null;
type MessageEventHandler = ((event: MessageEvent) => void) | null;

export class EventSource extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;

  readonly url: string;
  readonly withCredentials: boolean;
  onerror: EventHandler = null;
  onmessage: MessageEventHandler = null;
  onopen: EventHandler = null;
  #controller: AbortController | undefined;
  #fetcher: Fetcher;
  #lastEventId = "";
  #readyState = EventSource.CONNECTING;
  #retry = 3_000;

  constructor(url: string, options: EventSourceInit = {}) {
    super();
    this.url = new URL(url).toString();
    this.withCredentials = options.withCredentials ?? false;
    this.#fetcher = options.fetcher ?? { fetch };
    void this.connect();
  }

  get readyState(): number {
    return this.#readyState;
  }

  close(): void {
    if (this.#readyState === EventSource.CLOSED) return;
    this.#readyState = EventSource.CLOSED;
    this.#controller?.abort();
  }

  private async connect(): Promise<void> {
    while (this.#readyState !== EventSource.CLOSED) {
      this.#readyState = EventSource.CONNECTING;
      try {
        const response = await this.request();
        if (this.#readyState === EventSource.CLOSED) return;
        if (response.status === 204) {
          this.close();
          return;
        }
        if (!isEventStream(response)) throw new TypeError("EventSource requires a text/event-stream response");
        this.#readyState = EventSource.OPEN;
        this.dispatch(new Event("open"));
        await this.consume(response);
        if (this.#readyState !== EventSource.CLOSED) this.dispatch(new Event("error"));
      } catch {
        if (this.#readyState === EventSource.CLOSED) return;
        this.dispatch(new Event("error"));
      }
      if (this.#readyState === EventSource.CLOSED) return;
      this.#readyState = EventSource.CONNECTING;
      await this.waitForRetry();
    }
  }

  private async request(): Promise<Response> {
    const controller = new AbortController();
    this.#controller = controller;
    const headers = new Headers({ accept: "text/event-stream" });
    if (this.#lastEventId !== "") headers.set("last-event-id", this.#lastEventId);
    return this.#fetcher.fetch(new Request(this.url, { headers, signal: controller.signal }));
  }

  private async consume(response: Response): Promise<void> {
    if (response.body === null) throw new TypeError("EventSource response has no body");
    const decoder = new TextDecoder();
    const reader = response.body.getReader();
    let buffer = "";
    let message = emptyMessage();
    while (this.#readyState !== EventSource.CLOSED) {
      const chunk = await reader.read();
      if (this.#readyState === EventSource.CLOSED) return;
      if (chunk.done) {
        buffer += decoder.decode();
        ({ message } = this.lines(`${buffer}\n`, message));
        return;
      }
      buffer += decoder.decode(chunk.value, { stream: true });
      ({ buffer, message } = this.lines(buffer, message));
    }
  }

  private async waitForRetry(): Promise<void> {
    const signal = this.#controller?.signal;
    const wait = signal === undefined
      ? scheduler.wait(this.#retry)
      : scheduler.wait(this.#retry, { signal });
    await wait.catch(() => undefined);
  }

  private lines(input: string, message: EventMessage): ParsedEvents {
    while (true) {
      const next = nextLine(input);
      if (next === undefined) return { buffer: input, message };
      input = next.remainder;
      message = this.line(next.value, message);
    }
  }

  private line(line: string, message: EventMessage): EventMessage {
    if (line === "") {
      this.dispatchMessage(message);
      return emptyMessage();
    }
    if (line.startsWith(":")) return message;
    const [field, value] = splitField(line);
    if (field === "data") return { ...message, data: [...message.data, value] };
    if (field === "event") return { ...message, event: value };
    if (field === "id" && !value.includes("\0")) return { ...message, id: value };
    if (field === "retry" && /^\d+$/.test(value)) this.#retry = Number(value);
    return message;
  }

  private dispatchMessage(message: EventMessage): void {
    if (message.id !== undefined) this.#lastEventId = message.id;
    if (message.data.length === 0) return;
    const type = message.event === "" ? "message" : message.event;
    this.dispatch(new MessageEvent(type, {
      data: message.data.join("\n"),
      lastEventId: this.#lastEventId,
      origin: new URL(this.url).origin,
    }));
  }

  private dispatch(event: Event): void {
    this.dispatchEvent(event);
    if (event.type === "message" && event instanceof MessageEvent) {
      this.onmessage?.(event);
      return;
    }
    eventHandler(this, event.type)?.(event);
  }
}

interface EventMessage {
  readonly data: readonly string[];
  readonly event: string;
  readonly id?: string;
}

interface ParsedEvents {
  readonly buffer: string;
  readonly message: EventMessage;
}

function emptyMessage(): EventMessage {
  return { data: [], event: "" };
}

function isEventStream(response: Response): boolean {
  return response.status === 200
    && response.headers.get("content-type")?.split(";", 1)[0]?.trim().toLowerCase() === "text/event-stream";
}

function splitField(line: string): readonly [string, string] {
  const separator = line.indexOf(":");
  if (separator === -1) return [line, ""];
  const value = line.slice(separator + 1);
  return [line.slice(0, separator), value.startsWith(" ") ? value.slice(1) : value];
}

function nextLine(input: string): { readonly remainder: string; readonly value: string } | undefined {
  const end = input.search(/[\r\n]/);
  if (end === -1) return undefined;
  if (input[end] === "\r" && end + 1 === input.length) return undefined;
  const separatorLength = input.startsWith("\r\n", end) ? 2 : 1;
  return { remainder: input.slice(end + separatorLength), value: input.slice(0, end) };
}

function eventHandler(source: EventSource, type: string): EventHandler {
  if (type === "open") return source.onopen;
  if (type === "error") return source.onerror;
  return null;
}
