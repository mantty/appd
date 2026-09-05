import { installWebGlobals } from "../globals/web.mjs";
import { installWebSocketGlobals, WebSocket, WebSocketPair } from "../network/websocket.mjs";
import { EventEmitter } from "../events/events.mjs";
import { Writable, Readable, Duplex, Transform, PassThrough, Stream } from "../streams/node.mjs";
import { installProcessGlobals } from "../globals/process.mjs";
import { installConsoleGlobal } from "../globals/console.mjs";

installWebGlobals();
installWebSocketGlobals();

const builtinModules = {
  "node:events": { EventEmitter, default: EventEmitter },
  "node:stream": { Writable, Readable, Duplex, Transform, PassThrough, Stream, default: { Writable, Readable, Duplex, Transform, PassThrough, Stream } },
};
installProcessGlobals(builtinModules);
builtinModules["node:process"] = globalThis.process;
installConsoleGlobal();

export const env = globalThis.__tokamak_env ?? {};
export const caches = globalThis.__tokamak_caches ?? {};
export const waitUntil = (promise) => promise;
export const scheduler = { wait: () => Promise.resolve() };

export { WebSocket, WebSocketPair };
