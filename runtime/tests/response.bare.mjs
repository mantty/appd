import { RuntimeResponse } from "../../target/runtime-js/response.js";

const webSocket = {};
const upgrade = new RuntimeResponse(null, { status: 101, webSocket });

if (upgrade.status !== 101 || upgrade.ok || upgrade.webSocket !== webSocket) {
  throw new Error("invalid WebSocket upgrade response");
}

if (new RuntimeResponse("missing", { status: 404 }).status !== 404) {
  throw new Error("ordinary response status was not preserved");
}
