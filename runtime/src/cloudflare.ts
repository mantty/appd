import type { WorkerEnvironment } from "./types.js";

let bindings: WorkerEnvironment = {};

export const env = new Proxy<WorkerEnvironment>({}, {
  get: (_target, property): unknown => Reflect.get(bindings, property) as unknown,
  has: (_target, property) => Reflect.has(bindings, property),
  ownKeys: () => Reflect.ownKeys(bindings),
  getOwnPropertyDescriptor: (_target, property) => Reflect.getOwnPropertyDescriptor(
    bindings,
    property,
  ),
});

export function setEnvironment(next: WorkerEnvironment): void {
  bindings = next;
}

export class WorkerEntrypoint {
  readonly ctx: unknown;
  readonly env: unknown;

  constructor(ctx?: unknown, environment?: unknown) {
    this.ctx = ctx;
    this.env = environment;
  }
}

export class DurableObject extends WorkerEntrypoint {}

export class RpcTarget {
  readonly __rpcTarget = true;
}
