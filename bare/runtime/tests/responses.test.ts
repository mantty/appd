import assert from "node:assert/strict";
import test from "node:test";

import { writeResponse } from "../src/responses.js";

void test("preserves repeated Set-Cookie headers", async () => {
  const headers = new Headers();
  headers.append("set-cookie", "first=one; Path=/");
  headers.append("set-cookie", "second=two; Path=/");
  const writer = new TestWriter();

  await writeResponse(writer, new Request("https://app.appd.local/"), new Response(null, { headers }));

  assert.deepEqual(writer.headers?.["set-cookie"], ["first=one; Path=/", "second=two; Path=/"]);
});

class TestWriter {
  headers: Readonly<Record<string, string | string[]>> | undefined;

  end(): void {}

  write(): boolean {
    return true;
  }

  writeHead(_: number, headers: Readonly<Record<string, string | string[]>>): void {
    this.headers = headers;
  }
}
