import type { ExecutionContext } from "./types.js";

export class RequestContext implements ExecutionContext {
  readonly #pending: Promise<unknown>[] = [];

  passThroughOnException(): void {}

  waitUntil(promise: Promise<unknown>): void {
    this.#pending.push(promise);
  }

  drain(): void {
    void Promise.allSettled(this.#pending);
  }
}
