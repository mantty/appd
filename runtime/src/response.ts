import fetch from "bare-fetch";

import type { WorkerWebSocket } from "./websocket.js";

interface ResponseInitWithWebSocket extends ResponseInit {
  readonly webSocket?: WorkerWebSocket;
}

export class RuntimeResponse extends fetch.Response {
  readonly webSocket: WorkerWebSocket | undefined;

  constructor(body?: BodyInit | null, init: ResponseInitWithWebSocket = {}) {
    const { webSocket, ...responseInit } = init;
    super(body, responseInit);
    this.webSocket = webSocket;
  }
}
