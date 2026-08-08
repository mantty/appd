import Buffer from "bare-buffer";

/** The duplex channel connecting the worklet to its native host. */
export interface IpcStream {
  on(event: "data", listener: (chunk: Buffer) => void): this;
  off(event: "data", listener: (chunk: Buffer) => void): this;
  off(event: "error", listener: (error: Error) => void): this;
  once(event: "error", listener: (error: Error) => void): this;
  write(data: Buffer): boolean;
}

interface BareKitGlobal {
  readonly IPC: IpcStream;
}

declare const BareKit: BareKitGlobal;

const NEWLINE = 0x0a;

/** Return the channel to the native host. */
export function hostStream(): IpcStream {
  return BareKit.IPC;
}

/** Read one newline-terminated line from the host. */
export function readLine(stream: IpcStream): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    const onData = (chunk: Buffer) => {
      chunks.push(chunk);
      if (!chunk.includes(NEWLINE)) return;
      cleanup();
      resolve(decodeLine(chunks));
    };
    const onError = (error: Error) => {
      cleanup();
      reject(error);
    };
    const cleanup = () => {
      stream.off("data", onData);
      stream.off("error", onError);
    };
    stream.once("error", onError);
    stream.on("data", onData);
  });
}

/** Tell the host which port the gateway bound. */
export function reportListening(stream: IpcStream, port: number): void {
  writeLine(stream, `listening ${String(port)}`);
}

/** Tell the host that startup failed. */
export function reportStartupFailure(stream: IpcStream, error: unknown): void {
  writeLine(stream, `error ${describe(error)}`);
}

function writeLine(stream: IpcStream, message: string): void {
  stream.write(Buffer.from(`${message}\n`, "utf8"));
}

function decodeLine(chunks: Buffer[]): string {
  const line = Buffer.concat(chunks);
  const end = line.indexOf(NEWLINE);
  return line.toString("utf8", 0, end === -1 ? line.byteLength : end);
}

function describe(error: unknown): string {
  const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
  return message.replace(/[\r\n]+/g, " ").slice(0, 900);
}
