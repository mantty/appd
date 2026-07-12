import assert from "node:assert/strict";
import test from "node:test";

import { requestBody, responseBody, type ResponseWriter } from "../src/streams.js";

void test("turns an async request source into a readable stream", async () => {
  async function* chunks(): AsyncGenerator<Uint8Array> {
    await Promise.resolve();
    yield Uint8Array.from([1, 2]);
    yield Uint8Array.from([3, 4]);
  }

  const reader = requestBody(chunks()).getReader();
  assert.deepEqual(await reader.read(), { done: false, value: Uint8Array.from([1, 2]) });
  assert.deepEqual(await reader.read(), { done: false, value: Uint8Array.from([3, 4]) });
  assert.deepEqual(await reader.read(), { done: true, value: undefined });
});

void test("pipes response chunks with backpressure", async () => {
  const chunks: Uint8Array[] = [];
  let ended = false;
  let drained = false;
  const writer: ResponseWriter = {
    end: () => { ended = true; },
    once: (_event, listener) => {
      if (!drained) {
        drained = true;
        queueMicrotask(listener);
      }
    },
    write: (chunk) => {
      chunks.push(chunk);
      return chunks.length > 1;
    },
  };
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(Uint8Array.from([1]));
      controller.enqueue(Uint8Array.from([2]));
      controller.close();
    },
  });

  await responseBody(body, writer);

  assert.deepEqual(chunks, [Uint8Array.from([1]), Uint8Array.from([2])]);
  assert.equal(ended, true);
});
