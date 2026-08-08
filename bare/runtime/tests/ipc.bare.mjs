import Buffer from "bare-buffer"

import { readLine, reportListening, reportStartupFailure } from "../../../target/runtime-js/ipc.js"

const keepAlive = setInterval(() => {}, 1_000)

void run().then(
  () => {
    clearInterval(keepAlive)
    console.log("ipc checks passed")
  },
  (error) => {
    clearInterval(keepAlive)
    Bare.exitCode = 1
    console.error(error)
  },
)

function fakeStream() {
  const listeners = { data: [], error: [] }
  return {
    written: [],
    on(event, listener) {
      listeners[event].push(listener)
      return this
    },
    once(event, listener) {
      listeners[event].push(listener)
      return this
    },
    off(event, listener) {
      listeners[event] = listeners[event].filter((current) => current !== listener)
      return this
    },
    write(data) {
      this.written.push(Buffer.from(data).toString("utf8"))
      return true
    },
    emit(chunk) {
      for (const listener of [...listeners.data]) listener(chunk)
    },
    emitError(error) {
      for (const listener of [...listeners.error]) listener(error)
    },
    listenerCount(event) {
      return listeners[event].length
    }
  }
}

async function run() {
  const split = fakeStream()
  const line = readLine(split)
  split.emit(Buffer.from('{"host":"a.appd'))
  split.emit(Buffer.from('.local","p'))
  split.emit(Buffer.from('ort":0}\n'))

  const config = JSON.parse(await line)
  if (config.host !== "a.appd.local" || config.port !== 0) {
    throw new Error("readLine did not reassemble a split line")
  }
  if (split.listenerCount("data") !== 0 || split.listenerCount("error") !== 0) {
    throw new Error("readLine did not detach its listeners")
  }

  const utf8 = fakeStream()
  const decoded = readLine(utf8)
  const snowman = Buffer.from('{"host":"☃"}\n')
  utf8.emit(snowman.subarray(0, 11))
  utf8.emit(snowman.subarray(11))
  if (JSON.parse(await decoded).host !== "☃") {
    throw new Error("readLine mis-decoded a split multi-byte character")
  }

  const failed = fakeStream()
  const failureResult = readLine(failed).then(
    () => "resolved",
    (error) => error.message
  )
  failed.emitError(new Error("IPC failed"))
  if (await failureResult !== "IPC failed") {
    throw new Error("readLine did not reject an IPC error")
  }
  if (failed.listenerCount("data") !== 0 || failed.listenerCount("error") !== 0) {
    throw new Error("readLine did not detach its listeners after an error")
  }

  const replies = fakeStream()
  reportListening(replies, 49_152)
  if (replies.written[0] !== "listening 49152\n") {
    throw new Error(`unexpected listening reply: ${replies.written[0]}`)
  }

  reportStartupFailure(replies, new Error("broke\nacross\r\nlines"))
  const failure = replies.written[1]
  if (!failure.startsWith("error ") || !failure.endsWith("\n")) {
    throw new Error(`unexpected failure reply: ${failure}`)
  }
  if (failure.slice(0, -1).includes("\n")) {
    throw new Error("failure reply must occupy one line")
  }
}
