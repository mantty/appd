import "bare-encoding/global";
import "bare-stream/global";

import abort from "bare-abort-controller";
import Buffer from "bare-buffer";
import crypto from "bare-crypto/web";
import fetch from "bare-fetch";
import bareProcess from "bare-process";
import URL from "bare-url";

import { RuntimeResponse } from "./response.js";
import { installAesGcm } from "./crypto.js";
import { WebSocketPair } from "./websocket.js";

type ProcessShim = typeof bareProcess & { getBuiltinModule(name: string): unknown };
const processShim = bareProcess as ProcessShim;
processShim.getBuiltinModule = (name) => name === "node:process" ? processShim : undefined;
const consoleShim = Object.assign({}, console);
installAesGcm(crypto as unknown as Parameters<typeof installAesGcm>[0]);
installGetSetCookie();

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

function installGetSetCookie(): void {
  const prototype = fetch.Headers.prototype as unknown as BareHeaders;
  if (prototype.getSetCookie !== undefined) return;
  Object.defineProperty(prototype, "getSetCookie", {
    value(this: BareHeaders): string[] {
      return this._headers?.get("set-cookie")?.slice() ?? [];
    },
  });
}

interface BareHeaders {
  readonly _headers?: ReadonlyMap<string, string[]>;
  getSetCookie?: () => string[];
}
