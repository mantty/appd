import assert from "bare-assert"
import fs from "bare-fs"

import "../../../target/runtime-js/globals.js"
import { CacheStorage, caches, configureCaches } from "../../../target/runtime-js/cache.js"
import { RuntimeResponse } from "../../../target/runtime-js/response.js"

const keepAlive = setInterval(() => {}, 1_000)
const directory = `/tmp/appd-cache-${Date.now()}`
void run().then(
  () => {
    clearInterval(keepAlive)
    console.log("cache checks passed")
  },
  (error) => {
    clearInterval(keepAlive)
    Bare.exitCode = 1
    console.error(error)
  },
)

async function run() {
  configureCaches(directory)

  try {
    await verifiesCacheStorage()
    await verifiesCacheRules()
    await verifiesConditionalAndRangeResponses()
  } finally {
    fs.rmSync(directory, { force: true, recursive: true })
  }
}

async function verifiesCacheStorage() {
  assert.strictEqual(globalThis.caches, caches)
  const cache = caches.default
  const request = new Request("https://appd.local/cache?value=one")
  await cache.put(request, new RuntimeResponse("appd", {
    headers: { "cache-control": "max-age=60", "content-type": "text/plain" },
  }))

  const first = await cache.match(request)
  const second = await cache.match(request)
  assert.strictEqual(await first.text(), "appd")
  assert.strictEqual(await second.text(), "appd")
  assert.strictEqual(first.headers.get("content-type"), "text/plain")

  const named = await caches.open("appd")
  await named.put("https://appd.local/named", new RuntimeResponse("named", {
    headers: { "cache-control": "max-age=60" },
  }))
  assert.strictEqual(await (await named.match("https://appd.local/named")).text(), "named")
  const restored = new CacheStorage()
  restored.configure(directory)
  assert.strictEqual(await (await restored.default.match(request)).text(), "appd")
  assert.strictEqual(await (await (await restored.open("appd")).match("https://appd.local/named")).text(), "named")
  assert.strictEqual((await caches.keys()).includes("appd"), true)
  assert.strictEqual(await caches.delete("appd"), true)
  assert.strictEqual(await caches.has("appd"), false)

  await caches.open("created")
  assert.strictEqual(await caches.has("created"), true)
}

async function verifiesCacheRules() {
  const cache = caches.default
  const request = new Request("https://appd.local/rules")
  await expectsFailure(() => cache.put(new Request(request, { method: "POST" }), new RuntimeResponse("no")))
  await expectsFailure(() => cache.put(request, new RuntimeResponse("no", { status: 206 })))
  await expectsFailure(() => cache.put(request, new RuntimeResponse("no", { headers: { vary: "*" } })))

  await expectsFailure(() => cache.put(request, new RuntimeResponse("ignored", {
    headers: { "cache-control": "max-age=0" },
  })))
  await expectsFailure(() => cache.put(request, new RuntimeResponse("ignored", {
    headers: { "cache-control": "no-cache" },
  })))
  await expectsFailure(() => cache.put(request, new RuntimeResponse("ignored", {
    headers: { "cache-control": "private" },
  })))
  assert.strictEqual(await cache.match(request), undefined)

  const expiring = new Request("https://appd.local/expiring")
  await cache.put(expiring, new RuntimeResponse("temporary", { headers: { "cache-control": "max-age=1" } }))
  assert.strictEqual(await (await cache.match(expiring)).text(), "temporary")
  await wait(1_100)
  assert.strictEqual(await cache.match(expiring), undefined)

  await cache.put(request, new RuntimeResponse("method", { headers: { "cache-control": "max-age=60" } }))
  assert.strictEqual(await cache.match(new Request(request, { method: "POST" })), undefined)
  assert.strictEqual(await (await cache.match(new Request(request, { method: "POST" }), { ignoreMethod: true })).text(), "method")
  assert.strictEqual(await cache.delete(request), true)
  assert.strictEqual(await cache.delete(request), false)

  const cookieRequest = new Request("https://appd.local/cookie")
  await expectsFailure(() => cache.put(cookieRequest, new RuntimeResponse("no", {
    headers: { "cache-control": "max-age=60", "set-cookie": "session=blocked" },
  })))
  await cache.put(cookieRequest, new RuntimeResponse("yes", {
    headers: { "cache-control": "private=Set-Cookie, max-age=60", "set-cookie": "session=allowed" },
  }))
  assert.strictEqual((await cache.match(cookieRequest)).headers.getSetCookie().join(","), "session=allowed")

  const varied = new Request("https://appd.local/vary", { headers: { "accept-language": "en" } })
  await cache.put(varied, new RuntimeResponse("english", {
    headers: { "cache-control": "max-age=60", vary: "accept-language" },
  }))
  assert.strictEqual(await (await cache.match(varied)).text(), "english")
  assert.strictEqual(await cache.match(new Request(varied, { headers: { "accept-language": "cy" } })), undefined)

  const welsh = new Request(varied, { headers: { "accept-language": "cy" } })
  await cache.put(welsh, new RuntimeResponse("welsh", {
    headers: { "cache-control": "max-age=60", vary: "accept-language" },
  }))
  assert.strictEqual(await (await cache.match(varied)).text(), "english")
  assert.strictEqual(await (await cache.match(welsh)).text(), "welsh")
  const languages = (await cache.keys())
    .filter((key) => key.url === varied.url)
    .map((key) => key.headers.get("accept-language"))
    .sort()
  assert.strictEqual(languages.join(","), "cy,en")
}

async function verifiesConditionalAndRangeResponses() {
  const cache = caches.default
  const request = new Request("https://appd.local/conditional")
  const response = new RuntimeResponse("appd-runtime", {
    headers: {
      "cache-control": "max-age=60",
      "content-length": "12",
      etag: '"appd"',
      "last-modified": "Wed, 21 Oct 2015 07:28:00 GMT",
    },
  })
  assert.strictEqual(response.headers.get("etag"), '"appd"')
  await cache.put(request, response)

  const conditionalRequest = new Request(request, { headers: { "if-none-match": '"appd"' } })
  assert.strictEqual(conditionalRequest.headers.get("if-none-match"), '"appd"')
  const notModified = await cache.match(conditionalRequest)
  assert.strictEqual(notModified.status, 304)

  const modifiedSince = await cache.match(new Request(request, {
    headers: { "if-modified-since": "Wed, 21 Oct 2015 07:28:00 GMT" },
  }))
  assert.strictEqual(modifiedSince.status, 304)

  const range = await cache.match(new Request(request, { headers: { range: "bytes=5-11" } }))
  assert.strictEqual(range.status, 206)
  assert.strictEqual(range.headers.get("content-range"), "bytes 5-11/12")
  assert.strictEqual(await range.text(), "runtime")
}

async function expectsFailure(action) {
  let failed = false
  try {
    await action()
  } catch {
    failed = true
  }
  assert.strictEqual(failed, true)
}

function wait(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}
