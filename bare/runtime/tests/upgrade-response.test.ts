import assert from "node:assert/strict";
import test from "node:test";

import { writeUpgradeResponse } from "../src/upgrade-response.js";

void test("writes a streamed HTTP response when a worker rejects an upgrade", async () => {
  const socket = new TestSocket();
  const request = new Request("https://app.appd.local/socket");
  const response = new Response("missing", { status: 404 });

  await writeUpgradeResponse(socket, request, response);

  assert.equal(socket.output(), "HTTP/1.1 404 \r\ncontent-type: text/plain;charset=UTF-8\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n7\r\nmissing\r\n0\r\n\r\n");
});

void test("does not write a response body for a HEAD upgrade request", async () => {
  const socket = new TestSocket();
  const request = new Request("https://app.appd.local/socket", { method: "HEAD" });
  const response = new Response("missing", { status: 404 });

  await writeUpgradeResponse(socket, request, response);

  assert.equal(socket.output(), "HTTP/1.1 404 \r\ncontent-type: text/plain;charset=UTF-8\r\nConnection: close\r\n\r\n");
});

void test("writes each Set-Cookie header separately", async () => {
  const socket = new TestSocket();
  const request = new Request("https://app.appd.local/socket");
  const headers = new Headers();
  headers.append("set-cookie", "first=one; Path=/");
  headers.append("set-cookie", "second=two; Path=/");

  await writeUpgradeResponse(socket, request, new Response(null, { headers }));

  assert.match(socket.output(), /set-cookie: first=one; Path=\/\r\nset-cookie: second=two; Path=\//);
});

class TestSocket {
  readonly #writes: Array<string | Uint8Array> = [];

  end(data?: string | Uint8Array): void {
    if (data !== undefined) this.#writes.push(data);
  }

  once(): this {
    return this;
  }

  write(data: string | Uint8Array): boolean {
    this.#writes.push(data);
    return true;
  }

  output(): string {
    return this.#writes.map((write) => typeof write === "string" ? write : new TextDecoder().decode(write)).join("");
  }
}
