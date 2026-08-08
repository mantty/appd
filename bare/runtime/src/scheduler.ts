import { clearTimeout, setTimeout } from "bare-timers";

interface WaitOptions {
  readonly signal?: AbortSignal;
}

export const scheduler = {
  wait(delay: number, options: WaitOptions = {}): Promise<void> {
    return new Promise((resolve, reject) => {
      const { signal } = options;
      if (signal?.aborted) {
        reject(abortError());
        return;
      }
      const abort = () => {
        clearTimeout(timeout);
        reject(abortError());
      };
      const timeout = setTimeout(() => {
        signal?.removeEventListener("abort", abort);
        resolve();
      }, Math.max(0, delay));
      signal?.addEventListener("abort", abort, { once: true });
    });
  },
};

function abortError(): Error {
  const error = new Error("The operation was aborted");
  error.name = "AbortError";
  return error;
}
