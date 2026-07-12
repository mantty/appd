import "./globals.js";
import { startServer } from "./server.js";
import type { RuntimeConfig } from "./types.js";

interface BareKitGlobal {
  on(event: "push", listener: (payload: Uint8Array, reply: Reply) => void): void;
}

type Reply = (error: Error | null, payload?: string) => void;

declare const BareKit: BareKitGlobal;
BareKit.on("push", (payload, reply) => {
  void start(payload, reply);
});

async function start(payload: Uint8Array, reply: Reply): Promise<void> {
  try {
    const config = JSON.parse(Buffer.from(payload).toString("utf8")) as RuntimeConfig;
    const port = await startServer(config);
    reply(null, String(port));
  } catch (error) {
    const message = error instanceof Error ? error.stack ?? error.message : String(error);
    reply(new Error(message));
  }
}
