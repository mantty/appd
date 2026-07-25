import type { ExecutionContext, WorkerEnvironment } from "./types.js";

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
  readonly ctx: ExecutionContext;
  readonly env: WorkerEnvironment;

  constructor(ctx: ExecutionContext, environment: WorkerEnvironment) {
    this.ctx = ctx;
    this.env = environment;
  }
}

// eslint-disable-next-line @typescript-eslint/no-extraneous-class
export class DurableObject {
  constructor() {
    throw new Error("Durable Objects are not supported by appd");
  }
}

// eslint-disable-next-line @typescript-eslint/no-extraneous-class
export class RpcTarget {
  constructor() {
    throw new Error("RPC targets are not supported by appd");
  }
}
