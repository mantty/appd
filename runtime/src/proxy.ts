export interface ConnectRequest {
  readonly host: string;
  readonly port: number;
  readonly remainder: Uint8Array;
}

const HEADER_END = new Uint8Array([13, 10, 13, 10]);
export const MAX_CONNECT_HEADER_BYTES = 8 * 1024;

export function parseConnectRequest(data: Uint8Array): ConnectRequest | null {
  const headerEnd = indexOf(data, HEADER_END);
  if (headerEnd < 0) {
    if (data.byteLength > MAX_CONNECT_HEADER_BYTES) {
      throw new Error("Proxy CONNECT headers exceed 8 KiB");
    }
    return null;
  }
  if (headerEnd + HEADER_END.byteLength > MAX_CONNECT_HEADER_BYTES) {
    throw new Error("Proxy CONNECT headers exceed 8 KiB");
  }
  const header = new TextDecoder().decode(data.subarray(0, headerEnd));
  const requestLine = header.split("\r\n")[0] ?? "";
  const parts = requestLine.split(" ");
  const [method, authority, version] = parts;
  if (parts.length !== 3 || method !== "CONNECT" || authority === undefined || version !== "HTTP/1.1") {
    throw new Error("Proxy requires HTTP CONNECT");
  }
  const authorityParts = authority.split(":");
  if (authorityParts.length !== 2) throw new Error("Proxy CONNECT authority is invalid");
  const [host, portText] = authorityParts;
  const port = Number(portText);
  validateAuthority(host, port);
  return { host: host.toLowerCase(), port, remainder: data.subarray(headerEnd + HEADER_END.byteLength) };
}

function validateAuthority(host: string | undefined, port: number): asserts host is string {
  if (host === undefined || host === "" || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("Proxy CONNECT authority is invalid");
  }
}

function indexOf(data: Uint8Array, needle: Uint8Array): number {
  outer: for (let index = 0; index <= data.byteLength - needle.byteLength; index += 1) {
    for (let offset = 0; offset < needle.byteLength; offset += 1) {
      if (data[index + offset] !== needle[offset]) continue outer;
    }
    return index;
  }
  return -1;
}
