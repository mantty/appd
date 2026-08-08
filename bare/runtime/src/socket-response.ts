import type { HttpResponseWriter } from "./responses.js";

interface Socket {
  end(data?: string | Uint8Array): void;
  once(event: "drain", listener: () => void): this;
  write(data: string | Uint8Array): boolean;
}

export class SocketResponse implements HttpResponseWriter {
  readonly #socket: Socket;
  readonly #headOnly: boolean;
  readonly #statusText: string;
  readonly #bodyIncluded: boolean;
  #chunked = true;

  constructor(socket: Socket, method: string, status: string, bodyIncluded: boolean) {
    this.#socket = socket;
    this.#headOnly = method === "HEAD";
    this.#statusText = status;
    this.#bodyIncluded = bodyIncluded;
  }

  writeHead(status: number, headers: Readonly<Record<string, string | string[]>>): void {
    const entries = headerEntries(headers).filter(([name]) => !isConnectionHeader(name));
    this.#chunked = this.#bodyIncluded && !this.#headOnly && !entries.some(([name]) => name.toLowerCase() === "content-length");
    const transferEncoding = this.#chunked ? "Transfer-Encoding: chunked\r\n" : "";
    const lines = entries.map(([name, value]) => `${name}: ${value}\r\n`).join("");
    this.#socket.write(`HTTP/1.1 ${String(status)} ${this.#statusText}\r\n${lines}${transferEncoding}Connection: close\r\n\r\n`);
  }

  write(chunk: Uint8Array, callback: (error?: Error | null) => void): boolean {
    try {
      if (this.#headOnly) {
        callback(null);
        return true;
      }
      const written = this.#chunked ? this.writeChunk(chunk) : this.#socket.write(chunk);
      if (written) callback(null);
      else {
        this.#socket.once("drain", () => {
          callback(null);
        });
      }
      return written;
    } catch (error) {
      callback(error instanceof Error ? error : new Error(String(error)));
      return false;
    }
  }

  end(): void {
    this.#socket.end(this.#chunked && !this.#headOnly ? "0\r\n\r\n" : undefined);
  }

  private writeChunk(chunk: Uint8Array): boolean {
    const start = this.#socket.write(`${chunk.byteLength.toString(16)}\r\n`);
    const body = this.#socket.write(chunk);
    const end = this.#socket.write("\r\n");
    return start && body && end;
  }
}

function headerEntries(headers: Readonly<Record<string, string | string[]>>): Array<[string, string]> {
  const entries: Array<[string, string]> = [];
  for (const [name, value] of Object.entries(headers)) {
    if (Array.isArray(value)) {
      for (const item of value) entries.push([name, item]);
    } else {
      entries.push([name, value]);
    }
  }
  return entries;
}

function isConnectionHeader(name: string): boolean {
  const normalized = name.toLowerCase();
  return normalized === "connection" || normalized === "transfer-encoding";
}
