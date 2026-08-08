import assert from "bare-assert"
import EventEmitter from "bare-events"
import BareNet from "bare-net"

import { revocableWebStreams } from "../../../target/runtime-js/socket-streams.js"
import { Socket } from "../../../target/runtime-js/sockets.js"

class Transport extends EventEmitter {
  constructor() {
    super()
    this.backpressured = false
    this.ended = false
    this.paused = false
    this.writes = []
  }

  destroy() {}

  end() {
    this.ended = true
  }

  pause() {
    this.paused = true
  }

  resume() {
    this.paused = false
  }

  write(data) {
    this.writes.push(data)
    return !this.backpressured
  }
}

const keepAlive = setInterval(() => {}, 1_000)
void run().then(
  () => {
    clearInterval(keepAlive)
    console.log("socket stream checks passed")
  },
  (error) => {
    clearInterval(keepAlive)
    Bare.exitCode = 1
    console.error(error)
  },
)

async function run() {
  const transport = new Transport()
  const streams = revocableWebStreams(transport)
  const reader = streams.readable.getReader()
  const writer = streams.writable.getWriter()

  transport.emit("data", new Uint8Array([1, 2]))
  assert.strictEqual(Array.from((await reader.read()).value).join(","), "1,2")
  await writer.write(new Uint8Array([3, 4]))
  assert.strictEqual(Array.from(transport.writes[0]).join(","), "3,4")

  const pending = reader.read()
  streams.revoke()
  await rejects(pending)
  await rejects(writer.write(new Uint8Array([5])))
  assert.strictEqual(transport.writes.length, 1)

  const blockedTransport = new Transport()
  blockedTransport.backpressured = true
  const blockedStreams = revocableWebStreams(blockedTransport)
  const blockedWrite = blockedStreams.writable.getWriter().write(new Uint8Array([6]))
  blockedStreams.revoke()
  await rejects(blockedWrite)

  const closingTransport = new Transport()
  const closingStreams = revocableWebStreams(closingTransport)
  await closingStreams.writable.getWriter().close()
  assert.strictEqual(closingTransport.ended, true)

  const rawSocket = new BareNet.Socket()
  const socket = new Socket(rawSocket, { hostname: "example.com", port: 443 }, false, true)
  const socketReader = socket.readable.getReader()
  const socketWriter = socket.writable.getWriter()
  const tlsSocket = socket.startTls()
  assert.ok(tlsSocket instanceof Socket)
  await rejects(socketReader.read())
  await rejects(socketWriter.write(new Uint8Array([7])))
  await rejects(socket.close())
  assert.strictEqual(tlsSocket.readable.locked, false)
}

async function rejects(promise) {
  let failed = false
  try {
    await promise
  } catch {
    failed = true
  }
  assert.strictEqual(failed, true)
}
