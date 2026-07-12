interface BareKitGlobal {
  on(event: "push", listener: (payload: Uint8Array, reply: Reply) => void): void;
}

interface BareGlobal {
  on(event: "unhandledRejection", listener: (reason: unknown, promise: Promise<unknown>) => void): void;
}

type Reply = (error: Error | null, payload?: string) => void;

declare const BareKit: BareKitGlobal;
declare const Bare: BareGlobal;

Bare.on("unhandledRejection", (reason) => {
  console.error("Unhandled worker rejection", reason);
});

void import("./bootstrap.js").catch((error: unknown) => {
  const startupError = error instanceof Error ? error : new Error(String(error));
  BareKit.on("push", (_payload, reply) => {
    reply(startupError);
  });
});
