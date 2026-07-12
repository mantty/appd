import assert from "node:assert/strict";
import test from "node:test";

import { WebSocketPair } from "../src/websocket.js";

void test("delivers messages across an accepted pair", () => {
  const pair = new WebSocketPair();
  const messages: unknown[] = [];
  pair[1].accept();
  pair[0].accept();
  pair[1].addEventListener("message", (event) => messages.push((event as MessageEvent).data));

  pair[0].send("ping");

  assert.deepEqual(messages, ["ping"]);
});

void test("requires accept before sending", () => {
  const pair = new WebSocketPair();
  assert.throws(() => {
    pair[0].send("ping");
  }, /accepted/);
});

void test("bridges worker messages and close to a transport", () => {
  const pair = new WebSocketPair();
  const written: Array<string | Uint8Array> = [];
  let ended = false;
  pair[0].attach({
    destroy: () => undefined,
    end: () => {
      ended = true;
    },
    on: () => undefined,
    write: (data) => written.push(data),
  });
  pair[1].accept();

  pair[1].send("pong");
  pair[1].close();

  assert.deepEqual(written, ["pong"]);
  assert.equal(ended, true);
});

void test("preserves text and binary message types from a transport", () => {
  const pair = new WebSocketPair();
  const listeners = new Map<string, (data?: Uint8Array | Error, binary?: boolean) => void>();
  const messages: unknown[] = [];
  pair[1].accept();
  pair[1].addEventListener("message", (event) => messages.push((event as MessageEvent).data));
  pair[0].attach({
    destroy: () => undefined,
    end: () => undefined,
    on: (event, listener) => listeners.set(event, listener),
    write: () => undefined,
  });

  const binary = new Uint8Array([1, 2]);
  const partial = new Uint8Array([9, 3, 4, 9]).subarray(1, 3);
  listeners.get("message")?.(new TextEncoder().encode("ping 😀"), false);
  listeners.get("message")?.(binary, true);
  listeners.get("message")?.(partial, true);

  assert.equal(messages[0], "ping 😀");
  assert.equal(messages[1], binary.buffer);
  assert.notEqual(messages[2], partial.buffer);
  assert.deepEqual([...new Uint8Array(messages[2] as ArrayBuffer)], [3, 4]);
});
