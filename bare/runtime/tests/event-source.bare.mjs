import assert from "bare-assert"
import { setTimeout } from "bare-timers"

import "../../../target/runtime-js/globals.js"
import { EventSource } from "../../../target/runtime-js/event-source.js"
import { RuntimeResponse } from "../../../target/runtime-js/response.js"

const keepAlive = setInterval(() => {}, 1_000)
void run().then(
  () => {
    clearInterval(keepAlive)
    console.log("event source checks passed")
  },
  (error) => {
    clearInterval(keepAlive)
    Bare.exitCode = 1
    console.error(error)
  },
)

async function run() {
  await verifiesEventsAndFetcher()
  await verifiesCarriageReturnEvents()
  await verifiesSplitCarriageReturnLineFeeds()
  await verifiesCloseDuringConnection()
  await verifiesCloseDuringRead()
  await verifiesReconnectsWithLastEventId()
  await verifiesNoContentCloses()
  await verifiesInvalidResponsesReportErrors()
}

async function verifiesEventsAndFetcher() {
  const requests = []
  const fetcher = {
    fetch(request) {
      requests.push(request)
      return Promise.resolve(new RuntimeResponse(stream([
        "id: 7\nevent: lo",
        "cation\ndata: first\ndata: second\n\n",
      ]), {
        headers: { "content-type": "text/event-stream" },
      }))
    },
  }
  const source = new EventSource("https://events.appd.local/stream", { fetcher, withCredentials: true })
  const received = await once(source, "location")
  source.close()

  assert.strictEqual(source.url, "https://events.appd.local/stream")
  assert.strictEqual(source.withCredentials, true)
  assert.strictEqual(source.readyState, EventSource.CLOSED)
  assert.strictEqual(received.data, "first\nsecond")
  assert.strictEqual(received.lastEventId, "7")
  assert.strictEqual(received instanceof MessageEvent, true)
  assert.strictEqual(requests.length, 1)
  assert.strictEqual(requests[0].headers.get("accept"), "text/event-stream")
}

async function verifiesNoContentCloses() {
  const source = new EventSource("https://events.appd.local/none", {
    fetcher: { fetch: () => Promise.resolve(new RuntimeResponse(null, { status: 204 })) },
  })
  await eventually(() => source.readyState === EventSource.CLOSED)
}

async function verifiesCarriageReturnEvents() {
  const source = new EventSource("https://events.appd.local/cr", {
    fetcher: {
      fetch: () => Promise.resolve(new RuntimeResponse("data: carriage\r\r", {
        headers: { "content-type": "text/event-stream" },
      })),
    },
  })
  const received = await once(source, "message")
  source.close()
  assert.strictEqual(received.data, "carriage")
}

async function verifiesSplitCarriageReturnLineFeeds() {
  const source = new EventSource("https://events.appd.local/crlf", {
    fetcher: {
      fetch: () => Promise.resolve(new RuntimeResponse(stream([
        "data: one\r",
        "\ndata: two\r\n\r\n",
      ]), { headers: { "content-type": "text/event-stream" } })),
    },
  })
  const received = await once(source, "message")
  source.close()
  assert.strictEqual(received.data, "one\ntwo")
}

async function verifiesCloseDuringConnection() {
  let resolveResponse
  const response = new Promise((resolve) => { resolveResponse = resolve })
  const source = new EventSource("https://events.appd.local/late", { fetcher: { fetch: () => response } })
  let opened = false
  source.onopen = () => { opened = true }
  source.close()
  resolveResponse(new RuntimeResponse("data: late\n\n", { headers: { "content-type": "text/event-stream" } }))
  await pause()
  assert.strictEqual(source.readyState, EventSource.CLOSED)
  assert.strictEqual(opened, false)
}

async function verifiesCloseDuringRead() {
  let controller
  const body = new ReadableStream({ start: (stream) => { controller = stream } })
  const source = new EventSource("https://events.appd.local/read", {
    fetcher: {
      fetch: () => Promise.resolve(new RuntimeResponse(body, { headers: { "content-type": "text/event-stream" } })),
    },
  })
  let messages = 0
  source.onmessage = () => { messages += 1 }
  await pause()
  controller.enqueue(new TextEncoder().encode("data: late\n"))
  await pause()
  source.close()
  controller.enqueue(new TextEncoder().encode("\n"))
  controller.close()
  await pause()
  assert.strictEqual(messages, 0)
}

async function verifiesReconnectsWithLastEventId() {
  const requests = []
  const source = new EventSource("https://events.appd.local/reconnect", {
    fetcher: {
      fetch: (request) => {
        requests.push(request)
        const body = requests.length === 1 ? "id: 7\nretry: 0\n\n" : "data: reconnected\n\n"
        return Promise.resolve(new RuntimeResponse(body, { headers: { "content-type": "text/event-stream" } }))
      },
    },
  })
  const received = await once(source, "message")
  source.close()
  assert.strictEqual(received.data, "reconnected")
  assert.strictEqual(requests.length, 2)
  assert.strictEqual(requests[1].headers.get("last-event-id"), "7")
}

async function verifiesInvalidResponsesReportErrors() {
  const source = new EventSource("https://events.appd.local/invalid", {
    fetcher: {
      fetch: () => Promise.resolve(new RuntimeResponse("no", {
        headers: { "content-type": "text/event-stream" },
        status: 201,
      })),
    },
  })
  await once(source, "error")
  source.close()
}

function once(source, type) {
  return new Promise((resolve) => source.addEventListener(type, resolve, { once: true }))
}

async function eventually(predicate) {
  for (let attempts = 0; attempts < 20; attempts += 1) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  assert.strictEqual(predicate(), true)
}

function pause() {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

function stream(chunks) {
  let index = 0
  return new ReadableStream({
    pull(controller) {
      const chunk = chunks[index]
      index += 1
      if (chunk === undefined) {
        controller.close()
        return
      }
      controller.enqueue(new TextEncoder().encode(chunk))
    },
  })
}
