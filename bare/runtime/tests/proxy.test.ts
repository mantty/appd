import assert from "node:assert/strict";
import test from "node:test";

import { MAX_CONNECT_HEADER_BYTES, parseConnectRequest } from "../src/proxy.js";

void test("parses app CONNECT requests and preserves tunneled bytes", () => {
  const request = new TextEncoder().encode(
    "CONNECT APP.APPD.LOCAL:443 HTTP/1.1\r\nHost: APP.APPD.LOCAL:443\r\n\r\nhello",
  );
  const result = parseConnectRequest(request);
  assert.deepEqual(result && { host: result.host, port: result.port }, { host: "app.appd.local", port: 443 });
  assert.equal(new TextDecoder().decode(result?.remainder), "hello");
});

void test("waits for a complete CONNECT header", () => {
  const request = new TextEncoder().encode("CONNECT app.appd.local:443 HTTP/1.1\r\n");
  assert.equal(parseConnectRequest(request), null);
});

void test("rejects non-CONNECT proxy requests", () => {
  const request = new TextEncoder().encode("GET https://example.com/ HTTP/1.1\r\n\r\n");
  assert.throws(() => parseConnectRequest(request), /HTTP CONNECT/);
});

void test("rejects CONNECT request lines with extra fields", () => {
  const request = new TextEncoder().encode("CONNECT app.appd.local:443 HTTP/1.1 extra\r\n\r\n");
  assert.throws(() => parseConnectRequest(request), /HTTP CONNECT/);
});

void test("rejects CONNECT authorities with extra colons", () => {
  const request = new TextEncoder().encode("CONNECT app.appd.local:443:extra HTTP/1.1\r\n\r\n");
  assert.throws(() => parseConnectRequest(request), /authority is invalid/);
});

void test("rejects oversized incomplete CONNECT headers", () => {
  const request = new Uint8Array(MAX_CONNECT_HEADER_BYTES + 1);
  request.fill(65);
  assert.throws(() => parseConnectRequest(request), /exceed 8 KiB/);
});

void test("rejects oversized complete CONNECT headers", () => {
  const request = new TextEncoder().encode(
    `CONNECT app.appd.local:443 HTTP/1.1\r\nx: ${"a".repeat(MAX_CONNECT_HEADER_BYTES)}\r\n\r\n`,
  );
  assert.throws(() => parseConnectRequest(request), /exceed 8 KiB/);
});
