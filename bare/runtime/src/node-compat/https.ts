import { ClientRequest, get as getRequest, request as createRequest } from "./http-client.js";
import { createServer, Server } from "./http-server.js";

export const Agent = class Agent {
  readonly protocol = "https:";
  destroy(): void {}
};
export const globalAgent = new Agent();
export const get = (...arguments_: unknown[]): ClientRequest => getRequest("https:", arguments_);
export const request = (...arguments_: unknown[]): ClientRequest => createRequest("https:", arguments_);
export { Server, createServer };

export default { Agent, Server, createServer, get, globalAgent, request };
