import Buffer, { type Buffer as BareBuffer } from "bare-buffer";
import BareNet from "bare-net";
import BareTls from "bare-tls";

import { unsupportedClass, unsupportedMethod } from "./not-implemented.js";
import { connectTcpSocket, type SocketAddress } from "../sockets.js";

export const CLIENT_RENEG_LIMIT = 3;
export const CLIENT_RENEG_WINDOW = 600;
export const DEFAULT_CIPHERS = "";
export const DEFAULT_ECDH_CURVE = "auto";
export const DEFAULT_MAX_VERSION = "TLSv1.3";
export const DEFAULT_MIN_VERSION = "TLSv1.2";
export const TLSSocket = BareTls.TLSSocket;

interface NodeTlsSocket extends InstanceType<typeof TLSSocket> {
  readonly authorizationError: Error | undefined;
  readonly authorized: boolean;
}

function createConnection(...arguments_: unknown[]): NodeTlsSocket {
  const { address, callback, options } = connectionOptions(arguments_);
  const transport = new BareNet.Socket();
  const socket = new BareTls.TLSSocket(transport, options) as NodeTlsSocket;
  connectTcpSocket(transport, address);
  return withNodeEvents(socket, callback, verifiesPeerCertificate(options));
}

export const connect = createConnection;
export const Server = unsupportedClass("tls", "Server");
export const SecureContext = unsupportedClass("tls", "SecureContext");
export const createSecureContext = unsupportedMethod("tls", "createSecureContext");
export const createSecurePair = unsupportedMethod("tls", "createSecurePair");
export const createServer = unsupportedMethod("tls", "createServer");
export const checkServerIdentity = unsupportedMethod("tls", "checkServerIdentity");
export const getCiphers = (): string[] => [];
export const rootCertificates: readonly string[] = [];

export function convertALPNProtocols(protocols: readonly string[] | Uint8Array): Uint8Array {
  if (protocols instanceof Uint8Array) return protocols;
  const encoded: BareBuffer[] = [];
  for (const protocol of protocols) {
    const value = Buffer.from(protocol);
    if (value.byteLength === 0 || value.byteLength > 255) {
      throw new RangeError("ALPN protocol names must be between 1 and 255 bytes");
    }
    encoded.push(Buffer.from([value.byteLength]), value);
  }
  return Buffer.concat(encoded);
}

function connectionOptions(arguments_: unknown[]): {
  readonly address: SocketAddress;
  callback: (() => void) | undefined;
  readonly options: Record<string, unknown>;
} {
  const [first, second, third] = arguments_;
  if (typeof first === "number") {
    const host = typeof second === "string" ? second : "localhost";
    return {
      address: { hostname: host, port: first },
      callback: callbackOf(second) ?? callbackOf(third),
      options: tlsOptions(host, {}),
    };
  }
  if (typeof first === "string" || first instanceof URL) {
    const url = new URL(first);
    const options = optionsOf(second);
    const port = url.port === "" ? 443 : Number(url.port);
    return {
      address: { hostname: url.hostname, port },
      callback: callbackOf(second) ?? callbackOf(third),
      options: tlsOptions(url.hostname, options),
    };
  }
  if (typeof first !== "object" || first === null) {
    throw new TypeError("TLS connections require a port and hostname");
  }
  if ("path" in first) throw new Error("Unix-domain sockets are not supported");
  const options = first as Record<string, unknown>;
  if (typeof options.port !== "number") throw new TypeError("TLS connections require a numeric port");
  if (options.host !== undefined && typeof options.host !== "string") {
    throw new TypeError("TLS hostnames must be strings");
  }
  const host = options.host ?? "localhost";
  return {
    address: { hostname: host, port: options.port },
    callback: callbackOf(second),
    options: tlsOptions(host, options),
  };
}

function optionsOf(value: unknown): Record<string, unknown> {
  if (value === undefined || typeof value === "function") return {};
  if (typeof value !== "object" || value === null) throw new TypeError("TLS options must be an object");
  return value as Record<string, unknown>;
}

function tlsOptions(host: string, options: Record<string, unknown>): Record<string, unknown> {
  if (options.servername !== undefined && typeof options.servername !== "string") {
    throw new TypeError("TLS server names must be strings");
  }
  return { ...options, host: options.servername ?? host };
}

function callbackOf(value: unknown): (() => void) | undefined {
  return typeof value === "function" ? value as () => void : undefined;
}

function verifiesPeerCertificate(options: unknown): boolean {
  if (typeof options !== "object" || options === null) return true;
  const connectionOptions = options as { rejectUnauthorized?: unknown };
  return !Object.hasOwn(connectionOptions, "rejectUnauthorized") || connectionOptions.rejectUnauthorized !== false;
}

function withNodeEvents(
  socket: NodeTlsSocket,
  callback: (() => void) | undefined,
  verifiesPeerCertificate: boolean,
): NodeTlsSocket {
  let authorized = false;
  let authorizationError: Error | undefined = verifiesPeerCertificate
    ? undefined
    : new Error("Certificate verification is disabled");
  Object.defineProperties(socket, {
    authorizationError: { get: () => authorizationError },
    authorized: { get: () => authorized },
  });
  socket.once("error", (error) => {
    authorizationError = error;
  });
  socket.once("connect", () => {
    authorized = verifiesPeerCertificate;
    socket.emit("secureConnect");
    callback?.call(socket);
  });
  return socket;
}

export default {
  CLIENT_RENEG_LIMIT,
  CLIENT_RENEG_WINDOW,
  DEFAULT_CIPHERS,
  DEFAULT_ECDH_CURVE,
  DEFAULT_MAX_VERSION,
  DEFAULT_MIN_VERSION,
  SecureContext,
  Server,
  TLSSocket,
  checkServerIdentity,
  connect,
  convertALPNProtocols,
  createSecureContext,
  createSecurePair,
  createServer,
  getCiphers,
  rootCertificates,
};
