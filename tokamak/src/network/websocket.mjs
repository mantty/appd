import { EventEmitter } from "../events/events.mjs";

export class WebSocket extends EventEmitter {
  constructor() {
    super();
    this.readyState = 0;
    this.__tokamak_peer = undefined;
    this.__tokamak_outbox = [];
    this.__tokamak_receive = (data, binary) => {
      if (this.readyState === 3) return;
      this.emit("message", { type: "message", data, target: this, binary });
    };
    this.__tokamak_close = (code, reason) => {
      if (this.readyState === 3) return;
      this.readyState = 3;
      this.emit("close", closeEvent(this, code, reason));
    };
  }

  accept() { this.readyState = 1; }

  send(data) {
    if (this.readyState !== 1) throw new Error("WebSocket is not open");
    const peer = this.__tokamak_peer;
    if (peer === undefined) throw new Error("WebSocket has no peer");
    peer.__tokamak_outbox.push({ type: "message", binary: typeof data !== "string", data: websocketData(data) });
  }

  close(code = 1000, reason = "") {
    if (this.readyState === 3) return;
    this.readyState = 3;
    const peer = this.__tokamak_peer;
    if (peer !== undefined) peer.__tokamak_outbox.push({ type: "close", code, reason });
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
    client.__tokamak_peer = server;
    server.__tokamak_peer = client;
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

export function installWebSocketGlobals() {
  globalThis.WebSocket ??= WebSocket;
  globalThis.WebSocketPair ??= WebSocketPair;
}
