import assert from "node:assert/strict";
import test from "node:test";
import { Event } from "bare-events/web";

import { MessageEvent } from "../src/message-channel.js";
import { WebSocketPair, WorkerWebSocket } from "../src/websocket.js";

void test("delivers messages across an accepted pair", () => {
  const pair = new WebSocketPair();
  const messages: MessageEvent[] = [];
  pair[1].accept();
  pair[0].accept();
  pair[1].addEventListener("message", (event) => messages.push(event as MessageEvent));

  pair[0].send("ping");

  assert.equal(messages.length, 1);
  assert.ok(messages[0] instanceof MessageEvent);
  assert.equal(messages[0].data, "ping");
});

void test("uses EventTarget listeners and event instances", () => {
  const pair = new WebSocketPair();
  const events: Event[] = [];
  let opened = false;
  pair[1].onopen = () => { opened = true; };
  pair[1].onmessage = (event) => events.push(event);
  pair[1].addEventListener("message", (event) => events.push(event), { once: true });
  pair[1].accept();
  pair[0].accept();

  pair[0].send("first");
  pair[0].send("second");

  assert.equal(events.length, 3);
  assert.ok(events.every((event) => event instanceof Event));
  assert.equal(opened, true);
});

void test("emits a distinct error event for each socket", () => {
  const pair = new WebSocketPair();
  const listeners = new Map<string, (data?: Uint8Array | Error, binary?: boolean) => void>();
  const errors: Event[] = [];
  let handled: Error | undefined;
  pair[0].onerror = (event) => { handled = event.error; };
  pair[0].addEventListener("error", (event) => errors.push(event));
  pair[1].addEventListener("error", (event) => errors.push(event));
  pair[0].attach({
    destroy: () => undefined,
    end: () => undefined,
    on: (event, listener) => listeners.set(event, listener),
    write: () => undefined,
  });

  const failure = new Error("transport failed");
  listeners.get("error")?.(failure);

  assert.equal(errors.length, 2);
  const [source, peer] = errors;
  assert.ok(source);
  assert.ok(peer);
  assert.notEqual(source, peer);
  assert.ok(source instanceof Event);
  assert.ok(peer instanceof Event);
  assert.equal(source.target, pair[0]);
  assert.equal(peer.target, pair[1]);
  assert.equal((source as Event & { error: Error }).error, failure);
  assert.equal((peer as Event & { error: Error }).error, failure);
  assert.equal(handled, failure);
});

void test("requires accept before sending", () => {
  const pair = new WebSocketPair();
  assert.throws(() => {
    pair[0].send("ping");
  }, /accepted/);
});

void test("tracks close state and exposes close details", () => {
  const pair = new WebSocketPair();
  const closeEvents: Event[] = [];
  let handled: Event | undefined;
  pair[0].onclose = (event) => { handled = event; };
  pair[0].addEventListener("close", (event) => closeEvents.push(event));
  pair[1].accept();
  pair[0].accept();

  assert.equal(pair[0].readyState, WorkerWebSocket.OPEN);
  pair[0].close(1000, "done");

  assert.equal(pair[0].readyState, WorkerWebSocket.CLOSED);
  assert.equal(pair[1].readyState, WorkerWebSocket.CLOSED);
  assert.equal((closeEvents[0] as Event & { code: number }).code, 1000);
  assert.equal((closeEvents[0] as Event & { reason: string }).reason, "done");
  assert.equal(handled, closeEvents[0]);
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

void test("closes the pair when its transport closes", () => {
  const pair = new WebSocketPair();
  const listeners = new Map<string, (data?: Uint8Array | Error, binary?: boolean) => void>();
  let closeEvent: Event | undefined;
  pair[0].onclose = (event) => { closeEvent = event; };
  pair[0].attach({
    destroy: () => undefined,
    end: () => undefined,
    on: (event, listener) => listeners.set(event, listener),
    write: () => undefined,
  });
  pair[0].accept();
  pair[1].accept();

  listeners.get("close")?.();

  assert.equal(pair[0].readyState, WorkerWebSocket.CLOSED);
  assert.equal(pair[1].readyState, WorkerWebSocket.CLOSED);
  assert.ok(closeEvent instanceof Event);
  assert.equal((closeEvent as Event & { code: number }).code, 1000);
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
