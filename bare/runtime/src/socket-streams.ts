import Stream from "bare-stream";

export interface ByteTransport {
  destroy(error?: Error): void;
  end(): void;
  off(event: "close", listener: () => void): void;
  off(event: "data", listener: (data: Uint8Array) => void): void;
  off(event: "drain", listener: () => void): void;
  off(event: "error", listener: (error: Error) => void): void;
  on(event: "close", listener: () => void): void;
  on(event: "data", listener: (data: Uint8Array) => void): void;
  on(event: "error", listener: (error: Error) => void): void;
  once(event: "drain", listener: () => void): void;
  pause(): void;
  resume(): void;
  write(data: Uint8Array): boolean;
}

export interface RevocableWebStreams {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
  revoke(): void;
}

export function revocableWebStreams(transport: ByteTransport): RevocableWebStreams {
  let active = true;
  let paused = false;
  let cancelPendingWrite: (() => void) | undefined;
  const incoming = new Stream.Readable({
    read() {
      if (!active || !paused) return;
      paused = false;
      transport.resume();
    },
  });
  const outgoing = new Stream.Writable({
    destroy(error, done) {
      if (active) transport.destroy(error ?? undefined);
      done(null);
    },
    final(done) {
      if (active) transport.end();
      done(null);
    },
    write(chunk, _encoding, done) {
      if (!active) {
        done(unavailable());
        return;
      }
      if (!(chunk instanceof Uint8Array)) {
        done(new TypeError("Socket writes must contain byte arrays"));
        return;
      }
      if (transport.write(chunk)) {
        done(null);
        return;
      }
      const onDrain = () => {
        cancelPendingWrite = undefined;
        done(null);
      };
      cancelPendingWrite = () => {
        transport.off("drain", onDrain);
        done(unavailable());
      };
      transport.once("drain", onDrain);
    },
  });
  const onData = (data: Uint8Array) => {
    if (!active) return;
    if (!incoming.push(data)) {
      paused = true;
      transport.pause();
    }
  };
  const onClose = () => {
    if (active) incoming.push(null);
  };
  const onError = (error: Error) => {
    if (active) incoming.destroy(error);
  };

  incoming.on("error", noop);
  outgoing.on("error", noop);
  transport.on("data", onData);
  transport.on("close", onClose);
  transport.on("error", onError);

  return {
    readable: Stream.Readable.toWeb(incoming) as unknown as ReadableStream<Uint8Array>,
    writable: Stream.Writable.toWeb(outgoing) as WritableStream<Uint8Array>,
    revoke() {
      if (!active) return;
      active = false;
      transport.off("data", onData);
      transport.off("close", onClose);
      transport.off("error", onError);
      if (paused) transport.resume();
      cancelPendingWrite?.();
      cancelPendingWrite = undefined;
      const error = unavailable();
      incoming.destroy(error);
      outgoing.destroy(error);
    },
  };
}

function unavailable(): Error {
  return new Error("The socket is no longer usable after startTls()");
}

function noop(): void {}
