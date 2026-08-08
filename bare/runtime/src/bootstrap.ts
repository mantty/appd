import "./globals.js";
import { hostStream, readLine, reportListening, reportStartupFailure } from "./ipc.js";
import { startServer } from "./server.js";
import type { RuntimeConfig } from "./types.js";

void start();

async function start(): Promise<void> {
  const stream = hostStream();
  try {
    const config = JSON.parse(await readLine(stream)) as RuntimeConfig;
    reportListening(stream, await startServer(config));
  } catch (error) {
    console.error("Worker runtime failed to start", error);
    reportStartupFailure(stream, error);
  }
}
