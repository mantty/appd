import { hostStream, reportStartupFailure } from "./ipc.js";

interface BareGlobal {
  on(event: "unhandledRejection", listener: (reason: unknown, promise: Promise<unknown>) => void): void;
}

declare const Bare: BareGlobal;

Bare.on("unhandledRejection", (reason) => {
  console.error("Unhandled worker rejection", reason);
});

void import("./bootstrap.js").catch((error: unknown) => {
  reportStartupFailure(hostStream(), error);
});
