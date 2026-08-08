import { METHODS, STATUS_CODES } from "bare-http1";

import { ClientRequest, get as getRequest, IncomingMessage, request as createRequest } from "./http-client.js";
import { createServer, Server, ServerResponse } from "./http-server.js";
import { unsupportedMethod } from "./not-implemented.js";

export const maxHeaderSize = 16_384;
export { METHODS, STATUS_CODES };
export { ClientRequest, IncomingMessage };
export const Agent = class Agent {
  readonly protocol = "http:";
  destroy(): void {}
};
export const globalAgent = new Agent();
export const get = (...arguments_: unknown[]): ClientRequest => getRequest("http:", arguments_);
export const request = (...arguments_: unknown[]): ClientRequest => createRequest("http:", arguments_);
export { Server, ServerResponse, createServer };
export const OutgoingMessage = ClientRequest;
export const validateHeaderName = unsupportedMethod("http", "validateHeaderName");
export const validateHeaderValue = unsupportedMethod("http", "validateHeaderValue");
export const setMaxIdleHTTPParsers = unsupportedMethod("http", "setMaxIdleHTTPParsers");
export const _connectionListener = unsupportedMethod("http", "_connectionListener");

export default {
  Agent,
  ClientRequest,
  IncomingMessage,
  METHODS,
  OutgoingMessage,
  STATUS_CODES,
  Server,
  _connectionListener,
  createServer,
  get,
  globalAgent,
  request,
  setMaxIdleHTTPParsers,
  maxHeaderSize,
  validateHeaderName,
  validateHeaderValue,
};
