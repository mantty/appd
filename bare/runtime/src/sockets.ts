import BareNet from "bare-net";
import BareTls from "bare-tls";
import BareDns from "bare-dns";
import Stream from "bare-stream";
import ipaddr from "ipaddr.js";

import { type ByteTransport, revocableWebStreams } from "./socket-streams.js";

type SecureTransport = "off" | "on" | "starttls";

export interface SocketAddress {
  readonly hostname: string;
  readonly port: number;
}

export interface SocketOptions {
  readonly allowHalfOpen?: boolean;
  readonly secureTransport?: SecureTransport;
}

interface SocketInfo {
  readonly localAddress: string | null;
  readonly remoteAddress: string | null;
}

interface DuplexStreams {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
}

interface Transport extends ByteTransport {
  on(event: "close", listener: () => void): this;
  on(event: "connect", listener: () => void): this;
  on(event: "data", listener: (data: Uint8Array) => void): this;
  on(event: "error", listener: (error: Error) => void): this;
  off(event: "close", listener: () => void): this;
  off(event: "data", listener: (data: Uint8Array) => void): this;
  off(event: "drain", listener: () => void): this;
  off(event: "error", listener: (error: Error) => void): this;
  readonly localAddress?: string;
  readonly remoteAddress?: string;
}

export class Socket {
  readonly closed: Promise<void>;
  readonly opened: Promise<SocketInfo>;
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
  readonly #address: SocketAddress;
  readonly #allowStartTls: boolean;
  readonly #allowHalfOpen: boolean;
  readonly #transport: Transport;
  readonly #revokeStreams: () => void;
  #upgraded = false;

  constructor(
    transport: Transport,
    address: SocketAddress,
    allowHalfOpen: boolean,
    allowStartTls: boolean,
  ) {
    this.#address = address;
    this.#allowHalfOpen = allowHalfOpen;
    this.#allowStartTls = allowStartTls;
    this.#transport = transport;
    const streams = allowStartTls ? revocableWebStreams(transport) : webStreams(transport);
    this.readable = streams.readable;
    this.writable = streams.writable;
    this.#revokeStreams = streams.revoke;
    this.opened = opened(transport);
    this.closed = closed(transport);
  }

  async close(): Promise<void> {
    if (this.#upgraded) throw new Error("The socket is no longer usable after startTls()");
    this.#transport.destroy();
    await this.closed;
  }

  startTls(): Socket {
    if (!this.#allowStartTls || this.#upgraded) {
      throw new Error("startTls() is only available once on a starttls socket");
    }
    const transport = new BareTls.TLSSocket(this.#transport as InstanceType<typeof BareNet.Socket>, {
      allowHalfOpen: this.#allowHalfOpen,
      host: this.#address.hostname,
    }) as unknown as Transport;
    this.#upgraded = true;
    this.#revokeStreams();
    return new Socket(transport, this.#address, this.#allowHalfOpen, false);
  }
}

export function connect(input: SocketAddress | string, options: SocketOptions = {}): Socket {
  const address = socketAddress(input);
  const secureTransport = options.secureTransport ?? "off";
  const allowHalfOpen = options.allowHalfOpen ?? false;
  const socket = new BareNet.Socket({ allowHalfOpen });
  const transport = secureTransport === "on"
    ? new BareTls.TLSSocket(socket, { allowHalfOpen, host: address.hostname })
    : socket;
  connectTcpSocket(socket, address);
  return new Socket(transport as unknown as Transport, address, allowHalfOpen, secureTransport === "starttls");
}

export function connectTcpSocket(socket: InstanceType<typeof BareNet.Socket>, address: SocketAddress): void {
  void open(socket, validateAddress(address));
}

function socketAddress(input: SocketAddress | string): SocketAddress {
  if (typeof input !== "string") return validateAddress(input);
  const url = new URL(input.includes("://") ? input : `tcp://${input}`);
  return validateAddress({ hostname: url.hostname, port: Number(url.port) });
}

function validateAddress(address: SocketAddress): SocketAddress {
  if (address.hostname === "" || !Number.isInteger(address.port) || address.port < 1 || address.port > 65_535) {
    throw new TypeError("A socket address requires a hostname and port");
  }
  if (address.port === 25) throw new Error("TCP connections to port 25 are not allowed");
  if (isDisallowedAddress(address.hostname)) throw new Error("TCP connections to private network addresses are not allowed");
  return address;
}

async function open(socket: InstanceType<typeof BareNet.Socket>, address: SocketAddress): Promise<void> {
  try {
    Reflect.apply(BareNet.Socket.prototype.connect, socket, [
      { host: await resolveAddress(address.hostname), port: address.port },
    ]);
  } catch (error) {
    socket.destroy(error instanceof Error ? error : new Error(String(error)));
  }
}

function resolveAddress(hostname: string): Promise<string> {
  if (ipaddr.isValid(hostname)) return Promise.resolve(hostname);
  return new Promise((resolve, reject) => {
    BareDns.lookup(hostname, (error, address) => {
      if (error !== null || address === null) reject(error ?? new Error(`Unable to resolve ${hostname}`));
      else if (isDisallowedAddress(address)) reject(new Error("TCP connections to private network addresses are not allowed"));
      else resolve(address);
    });
  });
}

function isDisallowedAddress(value: string): boolean {
  const hostname = value.replace(/^\[|\]$/g, "").replace(/\.$/, "").toLowerCase();
  if (hostname === "localhost" || hostname.endsWith(".localhost")) return true;
  if (!ipaddr.isValid(hostname)) return false;
  return ipaddr.process(hostname).range() !== "unicast";
}

function webStreams(transport: Transport): DuplexStreams & { readonly revoke: () => void } {
  const streams = Stream.Duplex.toWeb(transport as InstanceType<typeof Stream.Duplex>) as unknown as DuplexStreams;
  return { ...streams, revoke: () => {} };
}

function opened(transport: Transport): Promise<SocketInfo> {
  return new Promise((resolve, reject) => {
    transport.on("connect", () => {
      resolve({
        localAddress: transport.localAddress ?? null,
        remoteAddress: transport.remoteAddress ?? null,
      });
    });
    transport.on("error", reject);
  });
}

function closed(transport: Transport): Promise<void> {
  return new Promise((resolve, reject) => {
    transport.on("close", resolve);
    transport.on("error", reject);
  });
}
