import { installWebGlobals, Headers, Request, Response, URL, URLSearchParams, TextEncoder, TextDecoder, ReadableStream, Blob, DOMException } from "./web.mjs";
import { EventEmitter } from "./events.mjs";
import { Writable, Readable, Duplex, Transform, PassThrough, Stream } from "./stream.mjs";

installWebGlobals();

const builtinModules = {
  "node:events": { EventEmitter, default: EventEmitter },
  "node:stream": { Writable, Readable, Duplex, Transform, PassThrough, Stream, default: { Writable, Readable, Duplex, Transform, PassThrough, Stream } },
  "node:process": globalThis.process ?? {},
};

const nativeGetBuiltinModule = globalThis.process?.getBuiltinModule;
globalThis.process ??= {};
globalThis.process.env ??= {};
globalThis.process.nextTick ??= (callback, ...args) => queueMicrotask(() => callback(...args));
globalThis.process.getBuiltinModule = (name) => builtinModules[name] ?? nativeGetBuiltinModule?.(name);
globalThis.process.platform ??= "appd";
globalThis.process.arch ??= "unknown";
globalThis.process.versions ??= { node: "22.14.0" };
globalThis.process.cwd ??= () => "/bundle";
globalThis.process.hrtime ??= (start) => {
  const now = Date.now();
  const value = [Math.floor(now / 1000), (now % 1000) * 1e6];
  if (!start) return value;
  const seconds = value[0] - start[0];
  const nanos = value[1] - start[1];
  return nanos < 0 ? [seconds - 1, nanos + 1e9] : [seconds, nanos];
};

globalThis.console ??= {
  log() {}, info() {}, warn() {}, error() {}, debug() {}, trace() {}, dir() {}, time() {}, timeEnd() {},
};

export const env = globalThis.__appd_env ?? {};
export const caches = globalThis.__appd_caches ?? {};
export const waitUntil = (promise) => promise;
export const scheduler = { wait: () => Promise.resolve() };

export class WebSocket extends EventEmitter {
  constructor() {
    super();
    this.readyState = 0;
    this.__appd_peer = undefined;
    this.__appd_outbox = [];
    this.__appd_receive = (data, binary) => {
      if (this.readyState === 3) return;
      this.emit("message", { type: "message", data, target: this, binary });
    };
    this.__appd_close = (code, reason) => {
      if (this.readyState === 3) return;
      this.readyState = 3;
      this.emit("close", closeEvent(this, code, reason));
    };
  }

  accept() { this.readyState = 1; }

  send(data) {
    if (this.readyState !== 1) throw new Error("WebSocket is not open");
    const peer = this.__appd_peer;
    if (peer === undefined) throw new Error("WebSocket has no peer");
    peer.__appd_outbox.push({ type: "message", binary: typeof data !== "string", data: websocketData(data) });
  }

  close(code = 1000, reason = "") {
    if (this.readyState === 3) return;
    this.readyState = 3;
    const peer = this.__appd_peer;
    if (peer !== undefined) peer.__appd_outbox.push({ type: "close", code, reason });
    this.emit("close", closeEvent(this, code, reason));
  }
}

function closeEvent(target, code, reason) {
  return { type: "close", code, reason, wasClean: true, target };
}

export class WebSocketPair {
  constructor() {
    const client = new WebSocket();
    const server = new WebSocket();
    client.__appd_peer = server;
    server.__appd_peer = client;
    this[0] = client;
    this[1] = server;
  }
}

function websocketData(data) {
  if (typeof data === "string") return data;
  if (data instanceof ArrayBuffer) return data.slice(0);
  if (ArrayBuffer.isView(data)) return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
  throw new TypeError("WebSocket data must be a string, ArrayBuffer, or ArrayBufferView");
}

globalThis.WebSocket ??= WebSocket;
globalThis.WebSocketPair ??= WebSocketPair;
