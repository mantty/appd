import BareNet from "bare-net";
import ipaddr from "ipaddr.js";

import { unsupportedClass, unsupportedMethod } from "./not-implemented.js";
import { connectTcpSocket, type SocketAddress as TcpSocketAddress } from "../sockets.js";

export class Socket extends BareNet.Socket {
  override connect(...arguments_: unknown[]): this {
    const { address, callback } = connectionArguments(arguments_);
    if (callback !== undefined) this.once("connect", callback);
    connectTcpSocket(this, address);
    return this;
  }
}

export function createConnection(...arguments_: unknown[]): Socket {
  return new Socket().connect(...arguments_);
}

export const connect = createConnection;
export const isIP = BareNet.isIP;
export const isIPv4 = BareNet.isIPv4;
export const isIPv6 = BareNet.isIPv6;
export const Server = unsupportedClass("net", "Server");
export const Stream = Socket;
export const _normalizeArgs = unsupportedMethod("net", "_normalizeArgs");
export const createServer = unsupportedMethod("net", "createServer");
export const getDefaultAutoSelectFamily = (): boolean => false;
export const getDefaultAutoSelectFamilyAttemptTimeout = (): number => 250;
export const setDefaultAutoSelectFamily = unsupportedMethod("net", "setDefaultAutoSelectFamily");
export const setDefaultAutoSelectFamilyAttemptTimeout = unsupportedMethod(
  "net",
  "setDefaultAutoSelectFamilyAttemptTimeout",
);

function connectionArguments(arguments_: unknown[]): {
  readonly address: TcpSocketAddress;
  readonly callback: (() => void) | undefined;
} {
  const [first, second, third] = arguments_;
  if (typeof first === "number") {
    return {
      address: { hostname: typeof second === "string" ? second : "localhost", port: first },
      callback: callbackOf(second) ?? callbackOf(third),
    };
  }
  if (typeof first !== "object" || first === null) {
    throw new TypeError("Socket connections require a port and hostname");
  }
  if ("path" in first) throw new Error("Unix-domain sockets are not supported");
  const options = first as { host?: unknown; port?: unknown };
  if (typeof options.port !== "number") throw new TypeError("Socket connections require a numeric port");
  if (options.host !== undefined && typeof options.host !== "string") {
    throw new TypeError("Socket hostnames must be strings");
  }
  return {
    address: { hostname: options.host ?? "localhost", port: options.port },
    callback: callbackOf(second),
  };
}

function callbackOf(value: unknown): (() => void) | undefined {
  return typeof value === "function" ? value as () => void : undefined;
}

type Address = ipaddr.IPv4 | ipaddr.IPv6;
type Family = "ipv4" | "ipv6";

interface AddressRule {
  readonly address: Address;
  readonly prefix: number;
}

interface RangeRule {
  readonly end: Address;
  readonly start: Address;
}

export class BlockList {
  readonly #addresses: AddressRule[] = [];
  readonly #ranges: RangeRule[] = [];

  addAddress(address: string, family?: Family): void {
    const parsed = parseAddress(address, family);
    this.#addresses.push({ address: parsed, prefix: prefixLength(parsed) });
  }

  addRange(start: string, end: string, family?: Family): void {
    const first = parseAddress(start, family);
    const last = parseAddress(end, family);
    if (first.kind() !== last.kind() || compare(first, last) > 0) {
      throw new RangeError("The address range is invalid");
    }
    this.#ranges.push({ end: last, start: first });
  }

  addSubnet(network: string, prefix: number, family?: Family): void {
    const address = parseAddress(network, family);
    if (!Number.isInteger(prefix) || prefix < 0 || prefix > prefixLength(address)) {
      throw new RangeError("The subnet prefix is invalid");
    }
    this.#addresses.push({ address, prefix });
  }

  check(address: string, family?: Family): boolean {
    const candidate = parseAddress(address, family);
    return this.#addresses.some((rule) => matches(candidate, rule.address, rule.prefix))
      || this.#ranges.some((rule) => inRange(candidate, rule));
  }

  get rules(): string[] {
    return [
      ...this.#addresses.map((rule) => `Subnet: ${familyName(rule.address)} ${rule.address}/${rule.prefix}`),
      ...this.#ranges.map((rule) => `Range: ${familyName(rule.start)} ${rule.start}-${rule.end}`),
    ];
  }
}

interface SocketAddressOptions {
  readonly address?: string;
  readonly family?: Family;
  readonly flowlabel?: number;
  readonly port?: number;
}

export class SocketAddress {
  readonly address: string;
  readonly family: Family;
  readonly flowlabel: number;
  readonly port: number;

  constructor(options: SocketAddressOptions = {}) {
    const parsed = parseAddress(options.address ?? "0.0.0.0", options.family);
    if (!Number.isInteger(options.port ?? 0) || (options.port ?? 0) < 0 || (options.port ?? 0) > 65_535) {
      throw new RangeError("The port is invalid");
    }
    this.address = parsed.toString();
    this.family = parsed.kind();
    this.flowlabel = options.flowlabel ?? 0;
    this.port = options.port ?? 0;
  }

  static isSocketAddress(value: unknown): value is SocketAddress {
    return value instanceof SocketAddress;
  }

  static parse(value: string): SocketAddress | undefined {
    try {
      const url = new URL(`tcp://${value}`);
      if (url.port === "") return undefined;
      return new SocketAddress({ address: url.hostname, port: Number(url.port) });
    } catch {
      return undefined;
    }
  }
}

function parseAddress(value: string, family: Family | undefined): Address {
  const address = ipaddr.parse(value);
  if (family !== undefined && address.kind() !== family) {
    throw new TypeError(`Expected an ${family} address`);
  }
  return address;
}

function prefixLength(address: Address): number {
  return address.kind() === "ipv4" ? 32 : 128;
}

function matches(candidate: Address, address: Address, prefix: number): boolean {
  return candidate.kind() === address.kind() && candidate.match(address, prefix);
}

function inRange(candidate: Address, range: RangeRule): boolean {
  return candidate.kind() === range.start.kind()
    && compare(candidate, range.start) >= 0
    && compare(candidate, range.end) <= 0;
}

function compare(first: Address, second: Address): number {
  const firstBytes = first.toByteArray();
  const secondBytes = second.toByteArray();
  for (let index = 0; index < firstBytes.length; index += 1) {
    const difference = firstBytes[index]! - secondBytes[index]!;
    if (difference !== 0) return difference;
  }
  return 0;
}

function familyName(address: Address): "IPv4" | "IPv6" {
  return address.kind() === "ipv4" ? "IPv4" : "IPv6";
}

export default {
  BlockList,
  Server,
  Socket,
  SocketAddress,
  Stream,
  _normalizeArgs,
  connect,
  createConnection,
  createServer,
  getDefaultAutoSelectFamily,
  getDefaultAutoSelectFamilyAttemptTimeout,
  isIP,
  isIPv4,
  isIPv6,
  setDefaultAutoSelectFamily,
  setDefaultAutoSelectFamilyAttemptTimeout,
};
