import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import test from "node:test";

interface FrameOptions {
  rsv1?: boolean;
  rsv2?: boolean;
  rsv3?: boolean;
  mask?: Buffer | null;
}

interface Frame {
  toBuffer(): Buffer;
}

interface FrameConstructor {
  new (opcode: number, payload: Buffer, options?: FrameOptions): Frame;
}

interface SocketPrototype {
  _isServer: boolean;
  _onframe(frame: Frame): void;
}

interface CommonJsModule {
  exports: unknown;
}

type CommonJsRequire = (name: string) => unknown;

const vendor = new URL("../../vendor/bare-ws/lib/", import.meta.url);

function loadCommonJs(file: string, dependencies: Record<string, unknown>): unknown {
  const module: CommonJsModule = { exports: {} };
  const source = fs.readFileSync(new URL(file, vendor), "utf8");
  const wrapper = new vm.Script(`(function(require, module, exports) {${source}\n})`)
    .runInNewContext({ Buffer }) as (
      require: CommonJsRequire,
      module: CommonJsModule,
      exports: unknown,
    ) => void;
  wrapper((name) => dependencies[name], module, module.exports);
  return module.exports;
}

const Frame = loadCommonJs("frame.js", {
  "bare-crypto": { randomFill: (buffer: Buffer) => { buffer.fill(0); } },
  "./errors": {},
}) as FrameConstructor;

const protocolError = (code: string) => () => Object.assign(new Error(code), { code });
const WebSocket = loadCommonJs("socket.js", {
  "bare-stream": { Duplex: function Duplex() {} },
  "bare-http1": {},
  "bare-https": {},
  "bare-crypto": {},
  "./constants": { GUID: "", opcode: { CLOSE: 8 } },
  "./errors": {
    EXPECTED_MASK: protocolError("EXPECTED_MASK"),
    UNEXPECTED_MASK: protocolError("UNEXPECTED_MASK"),
  },
  "./frame": Frame,
}) as { prototype: SocketPrototype };

void test("encodes RSV bits independently", () => {
  const flags = [
    { rsv1: true, expected: 0x40 },
    { rsv2: true, expected: 0x20 },
    { rsv3: true, expected: 0x10 },
  ];

  for (const { expected, ...options } of flags) {
    const firstByte = new Frame(1, Buffer.from("x"), options).toBuffer()[0] ?? 0;
    assert.equal(firstByte & 0x70, expected);
  }
});

void test("validates masks on zero-length frames", () => {
  const server = Object.create(WebSocket.prototype) as SocketPrototype;
  server._isServer = true;
  assert.throws(
    () => { server._onframe(new Frame(1, Buffer.alloc(0), { mask: null })); },
    /EXPECTED_MASK/,
  );

  const client = Object.create(WebSocket.prototype) as SocketPrototype;
  client._isServer = false;
  assert.throws(
    () => { client._onframe(new Frame(1, Buffer.alloc(0), { mask: Buffer.alloc(4) })); },
    /UNEXPECTED_MASK/,
  );
});
