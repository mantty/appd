import type { ExecutionContext, WorkerEnvironment } from "./types.js";

let bindings: WorkerEnvironment = {};
let workerExports: Readonly<Record<string, unknown>> = {};

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

export function setWorkerExports(next: Readonly<Record<string, unknown>>): void {
  workerExports = next;
}

export class WorkerEntrypoint<Environment extends WorkerEnvironment = WorkerEnvironment, Props = unknown> {
  readonly ctx: ExecutionContext<Props>;
  readonly env: Environment;

  constructor(ctx: ExecutionContext<Props>, environment: Environment) {
    this.ctx = ctx;
    this.env = environment;
  }
}

export function createExports(context: ExecutionContext): Record<string, unknown> {
  return new Proxy({}, {
    get(_target, name): unknown {
      if (typeof name !== "string") return undefined;
      const entrypoint = workerExports[name];
      if (!isEntrypoint(entrypoint)) return undefined;
      return binding(entrypoint, context);
    },
    ownKeys: () => Object.keys(workerExports).filter((name) => isEntrypoint(workerExports[name])),
    getOwnPropertyDescriptor: (_target, name) => {
      if (typeof name !== "string" || !isEntrypoint(workerExports[name])) return undefined;
      return { configurable: true, enumerable: true };
    },
  });
}

type EntrypointConstructor = new (
  context: ExecutionContext<unknown>,
  environment: WorkerEnvironment,
) => WorkerEntrypoint;

function binding(Entrypoint: EntrypointConstructor, context: ExecutionContext): unknown {
  const target = (options?: { readonly props?: unknown }) => entrypoint(Entrypoint, context, options?.props);
  return new Proxy(target, {
    apply: (_target, _this, arguments_) => target(arguments_[0] as { readonly props?: unknown } | undefined),
    get: (_target, name): unknown => {
      if (name === "then") return undefined;
      return Reflect.get(entrypoint(Entrypoint, context, undefined), name);
    },
  });
}

function entrypoint(
  Entrypoint: EntrypointConstructor,
  parent: ExecutionContext,
  props: unknown,
): WorkerEntrypoint {
  const context = new ContextWithProps(parent, props);
  const instance = new Entrypoint(context, bindings);
  return new Proxy(instance, {
    get(target, name, receiver): unknown {
      const value = Reflect.get(target, name, receiver);
      if (typeof value !== "function") return value;
      return (...arguments_: unknown[]) => Promise.resolve().then(() => Reflect.apply(value, target, arguments_));
    },
  });
}

class ContextWithProps implements ExecutionContext<unknown> {
  readonly exports: Record<string, unknown>;
  readonly props: unknown;
  readonly #parent: ExecutionContext;

  constructor(parent: ExecutionContext, props: unknown) {
    this.#parent = parent;
    this.props = props;
    this.exports = createExports(this);
  }

  passThroughOnException(): void {
    this.#parent.passThroughOnException();
  }

  waitUntil(promise: Promise<unknown>): void {
    this.#parent.waitUntil(promise);
  }
}

function isEntrypoint(value: unknown): value is EntrypointConstructor {
  return typeof value === "function" && value.prototype instanceof WorkerEntrypoint;
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
