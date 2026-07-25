import type { ExecutionContext } from "./types.js";

export class RequestContext implements ExecutionContext {
  readonly #pending: Promise<unknown>[] = [];

  passThroughOnException(): void {
    throw new Error("passThroughOnException is not supported without an origin server");
  }

  waitUntil(promise: Promise<unknown>): void {
    this.#pending.push(promise);
  }

  async drain(): Promise<void> {
    while (this.#pending.length > 0) {
      const pending = this.#pending.splice(0);
      const results = await Promise.allSettled(pending);
      for (const result of results) {
        if (result.status === "rejected") console.error("waitUntil rejected", result.reason);
      }
    }
  }
}
