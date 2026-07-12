import "bare-encoding/global";
import "bare-stream/global";

import abort from "bare-abort-controller";
import Buffer from "bare-buffer";
import crypto from "bare-crypto/web";
import fetch from "bare-fetch";
import URL from "bare-url";

import { RuntimeResponse } from "./response.js";
import { WebSocketPair } from "./websocket.js";

const processShim: {
  readonly env: Record<string, string | undefined>;
  readonly nextTick: (callback: (...args: unknown[]) => void, ...args: unknown[]) => void;
  readonly getBuiltinModule: (name: string) => unknown;
} = {
  env: {},
  nextTick: (callback, ...args) => {
    queueMicrotask(() => {
      callback(...args);
    });
  },
  getBuiltinModule: (name) => name === "node:process" ? processShim : undefined,
};
const consoleShim = Object.assign({}, console);

Object.assign(globalThis, {
  AbortController: abort.AbortController,
  AbortSignal: abort.AbortSignal,
  Buffer,
  Headers: fetch.Headers,
  Request: fetch.Request,
  Response: RuntimeResponse,
  URL,
  URLSearchParams: URL.URLSearchParams,
  WebSocketPair,
  atob: Buffer.atob,
  btoa: Buffer.btoa,
  crypto,
  console: consoleShim,
  fetch,
  process: processShim,
});
