import assert from "bare-assert"
import { processShim, setProcessEnvironment } from "../../../target/runtime-js/globals.js"
import { Buffer } from "../../../target/runtime-js/node-compat/buffer.js"
import path from "../../../target/runtime-js/node-compat/path.js"
import { parse, stringify } from "../../../target/runtime-js/node-compat/querystring.js"
import { StringDecoder } from "../../../target/runtime-js/node-compat/string-decoder.js"
import { arrayBuffer, text } from "../../../target/runtime-js/node-compat/stream-consumers.js"
import Stream from "../../../target/runtime-js/node-compat/stream.js"
import { MockTracker } from "../../../target/runtime-js/node-compat/test.js"
import { setTimeout } from "../../../target/runtime-js/node-compat/timers-promises.js"
import { URL } from "../../../target/runtime-js/node-compat/url.js"
import { connect } from "../../../target/runtime-js/sockets.js"
import { handleAsNodeRequest, httpServerHandler } from "../../../target/runtime-js/cloudflare-node.js"

const keepAlive = setInterval(() => {}, 1_000)
void run().then(
  () => {
    clearInterval(keepAlive)
    console.log("node-compat checks passed")
  },
  (error) => {
    clearInterval(keepAlive)
    Bare.exitCode = 1
    console.error(error)
  },
)

async function run() {
  setProcessEnvironment({
    JSON: { enabled: true },
    TEXT: "value"
  })
  
  assert.strictEqual(processShim.env.TEXT, "value")
  assert.strictEqual(processShim.env.JSON, '{"enabled":true}')
  assert.strictEqual(processShim.env.PATH, undefined)
  assert.strictEqual(processShim.arch, "x64")
  assert.strictEqual(processShim.argv[0], "workerd")
  assert.strictEqual(processShim.cwd(), "/bundle")
  assert.strictEqual(processShim.pid, 1)
  assert.strictEqual(processShim.platform, "linux")
  assert.strictEqual(processShim.umask(), 0o022)
  assert.strictEqual(processShim.version, "v22.19.0")
  assert.strictEqual(processShim.versions.node, "22.19.0")
  const builtinAssert = processShim.getBuiltinModule("node:assert")
  const builtinChildProcess = processShim.getBuiltinModule("node:child_process")
  const builtinConsole = processShim.getBuiltinModule("node:console")
  const builtinConstants = processShim.getBuiltinModule("node:constants")
  const builtinCrypto = processShim.getBuiltinModule("node:crypto")
  const builtinDiagnosticsChannel = processShim.getBuiltinModule("node:diagnostics_channel")
  const builtinDns = processShim.getBuiltinModule("node:dns")
  const builtinDnsPromises = processShim.getBuiltinModule("node:dns/promises")
  const builtinHttp = processShim.getBuiltinModule("node:http")
  const builtinHttps = processShim.getBuiltinModule("node:https")
  const builtinHttp2 = processShim.getBuiltinModule("node:http2")
  const builtinModule = processShim.getBuiltinModule("node:module")
  const builtinNet = processShim.getBuiltinModule("node:net")
  const builtinOs = processShim.getBuiltinModule("node:os")
  const builtinPerfHooks = processShim.getBuiltinModule("node:perf_hooks")
  const builtinPath = processShim.getBuiltinModule("node:path")
  const builtinStream = processShim.getBuiltinModule("node:stream")
  const builtinStreamPromises = processShim.getBuiltinModule("node:stream/promises")
  const builtinStreamWeb = processShim.getBuiltinModule("node:stream/web")
  const builtinTest = processShim.getBuiltinModule("node:test")
  const builtinTimers = processShim.getBuiltinModule("node:timers")
  const builtinTimersPromises = processShim.getBuiltinModule("node:timers/promises")
  const builtinTls = processShim.getBuiltinModule("node:tls")
  const builtinUrl = processShim.getBuiltinModule("node:url")
  const builtinUtil = processShim.getBuiltinModule("node:util")
  const builtinUtilTypes = processShim.getBuiltinModule("node:util/types")
  const builtinSys = processShim.getBuiltinModule("node:sys")
  const builtinZlib = processShim.getBuiltinModule("node:zlib")
  
  assert.strictEqual(typeof builtinAssert, "function")
  builtinAssert(true)
  builtinAssert.deepStrictEqual({ appd: ["bare"] }, { appd: ["bare"] })
  builtinAssert.throws(() => builtinAssert.strictEqual(1, 2))
  function expectsAssertion(callback) {
    let error
    try {
      callback()
    } catch (caught) {
      error = caught
    }
    assert.ok(error instanceof builtinAssert.AssertionError)
  }
  expectsAssertion(() => builtinAssert.deepStrictEqual(-0, 0))
  expectsAssertion(() => builtinAssert.throws(() => { throw Error("wrong") }, Error("expected")))
  builtinAssert.deepEqual(1, "1")
  builtinAssert.deepEqual(null, undefined)
  const cyclicActual = {}
  const cyclicExpected = {}
  cyclicActual.self = cyclicActual
  cyclicExpected.self = cyclicExpected
  builtinAssert.deepStrictEqual(cyclicActual, cyclicExpected)
  expectsAssertion(() => builtinAssert.deepStrictEqual({ self: cyclicActual }, cyclicExpected))
  builtinAssert.deepStrictEqual(new Date(0), new Date(0))
  expectsAssertion(() => builtinAssert.deepStrictEqual(new Date("invalid"), new Date("invalid")))
  builtinAssert.deepStrictEqual(new Number(1), new Number(1))
  expectsAssertion(() => builtinAssert.deepStrictEqual(new Number(1), new Number(2)))
  expectsAssertion(() => builtinAssert.deepStrictEqual(new WeakMap(), new WeakMap()))
  builtinAssert.deepStrictEqual(Promise.resolve(1), Promise.resolve(2))
  builtinAssert.deepStrictEqual(/appd/gi, /appd/gi)
  builtinAssert.deepStrictEqual(new Uint8Array([1, 2]), new Uint8Array([1, 2]))
  if (typeof SharedArrayBuffer !== "undefined") {
    builtinAssert.deepStrictEqual(new SharedArrayBuffer(2), new SharedArrayBuffer(2))
  expectsAssertion(() => builtinAssert.deepStrictEqual(new SharedArrayBuffer(2), new SharedArrayBuffer(3)))
  }
  builtinAssert.deepStrictEqual(new Set([{ appd: true }]), new Set([{ appd: true }]))
  builtinAssert.deepStrictEqual(new Map([[{ appd: true }, [1]]]), new Map([[{ appd: true }, [1]]]))
  const symbol = Symbol("appd")
  builtinAssert.deepStrictEqual({ [symbol]: true }, { [symbol]: true })
  builtinAssert.partialDeepStrictEqual(new Date(0), new Date(0))
  expectsAssertion(() => builtinAssert.partialDeepStrictEqual(new Date(0), new Date(1)))
  expectsAssertion(() => builtinAssert.partialDeepStrictEqual(/appd/, /bare/))
  expectsAssertion(() => builtinAssert.partialDeepStrictEqual(new Number(1), new Number(2)))
  builtinAssert.partialDeepStrictEqual(["appd", "bare"], ["appd"])
  builtinAssert.partialDeepStrictEqual(new Map([[{ appd: true, bare: true }, { runtime: "bare" }]]), new Map([[{ appd: true }, { runtime: "bare" }]]))
  builtinAssert.partialDeepStrictEqual(new Set([{ appd: true, bare: true }]), new Set([{ appd: true }]))
  builtinAssert.partialDeepStrictEqual(new Error("appd", { cause: Error("bare") }), Error("appd"))
  builtinAssert.throws(() => { throw new Error("appd") }, function predicate(error) {
    return error instanceof Error && error.message === "appd"
  })
  builtinAssert.throws(() => { throw new Error("appd") }, (error) => error instanceof Error)
  expectsAssertion(() => builtinAssert.throws(() => { throw new Error("appd") }, TypeError))
  function AppdError() {}
  builtinAssert.throws(() => { throw new AppdError() }, AppdError)
  let childProcessError
  try {
    builtinChildProcess.spawn("echo")
  } catch (error) {
    childProcessError = error
  }
  assert.ok(childProcessError instanceof Error)
  assert.ok(childProcessError.message.includes("child_process.spawn method is not implemented"))
  assert.strictEqual(typeof builtinConsole.Console, "function")
  assert.strictEqual(typeof builtinConsole.log, "function")
  assert.ok(builtinConsole instanceof builtinConsole.Console)
  assert.strictEqual(builtinConstants.O_RDONLY, 0)
  assert.strictEqual(builtinConstants.SIGINT, 2)
  assert.strictEqual(builtinCrypto.createHash("sha256").update("appd").digest("hex"), "202f02b0b11359d092bdff94a65202a11070abf00c75113b0e99f8a1ca387ceb")
  let diagnosticMessage
  const diagnosticChannel = builtinDiagnosticsChannel.channel("appd")
  diagnosticChannel.subscribe((message) => { diagnosticMessage = message })
  diagnosticChannel.publish("appd")
  assert.strictEqual(diagnosticMessage, "appd")
  const tracingChannel = builtinDiagnosticsChannel.tracingChannel("appd-trace")
  let traceEnded = false
  tracingChannel.subscribe({ end: () => { traceEnded = true } })
  let traceError
  try {
    tracingChannel.traceSync(() => { throw new Error("appd trace") })
  } catch (error) {
    traceError = error
  }
  assert.strictEqual(traceError.message, "appd trace")
  assert.ok(traceEnded)
  assert.strictEqual(await tracingChannel.tracePromise(async () => "appd"), "appd")
  assert.strictEqual(builtinDns.ADDRCONFIG, 1024)
  assert.strictEqual(typeof builtinDns.lookup, "function")
  assert.strictEqual(typeof builtinDns.promises.lookup, "function")
  assert.strictEqual(typeof builtinDnsPromises.resolveTxt, "function")
  const dnsResolver = new builtinDnsPromises.Resolver()
  assert.strictEqual(typeof dnsResolver.resolveTxt, "function")
  dnsResolver.cancel()
  assert.ok(builtinModule.builtinModules.includes("assert"))
  assert.ok(builtinModule.isBuiltin("node:assert"))
  assert.strictEqual(typeof builtinModule.createRequire("file:///worker.js")("node:assert"), "function")
  assert.strictEqual(typeof builtinHttp.request, "function")
  assert.strictEqual(typeof builtinHttps.request, "function")
  assert.strictEqual(typeof builtinHttp2, "function")
  assert.ok(builtinHttp.METHODS.includes("GET"))
  const overriddenRequest = builtinHttp.request("https://example.test/initial", {
    hostname: "appd.local",
    path: "/override?runtime=bare",
    port: 4_321,
  })
  assert.strictEqual(overriddenRequest.host, "appd.local:4321")
  assert.strictEqual(overriddenRequest.path, "/override?runtime=bare")
  const virtualServer = builtinHttp.createServer((request, response) => {
    assert.strictEqual(request.method, "POST")
    assert.strictEqual(request.url, "/node?runtime=bare")
    assert.strictEqual(request.headers.host, "appd.local")
    response.setHeader("x-appd", "bare")
    response.setHeader("set-cookie", ["a=1", "b=2"])
    builtinAssert.deepStrictEqual(response.getHeaders()["set-cookie"], ["a=1", "b=2"])
    response.writeHead(201, "Created")
    response.write("app")
    response.end("d")
  })
  virtualServer.listen(4_321)
  const virtualResponse = await handleAsNodeRequest(4_321, new Request("https://appd.local/node?runtime=bare", {
    headers: { host: "appd.local" },
    method: "POST",
  }))
  assert.strictEqual(virtualResponse.status, 201)
  assert.strictEqual(virtualResponse.headers.get("x-appd"), "bare")
  assert.strictEqual(await virtualResponse.text(), "appd")
  virtualServer.close()
  const headServer = builtinHttp.createServer((_request, response) => { response.end("ignored") })
  headServer.listen(4_322)
  const headResponse = await handleAsNodeRequest(4_322, new Request("https://appd.local/node", { method: "HEAD" }))
  assert.strictEqual(await headResponse.text(), "")
  headServer.close()
  const implicitServer = builtinHttp.createServer((_request, response) => { response.end("implicit") })
  const implicitHandler = httpServerHandler(implicitServer)
  assert.strictEqual(await (await implicitHandler.fetch(new Request("https://appd.local/"))).text(), "implicit")
  implicitServer.close()
  assert.strictEqual(builtinNet.isIP("127.0.0.1"), 4)
  assert.strictEqual(typeof builtinNet.createConnection, "function")
  const blockList = new builtinNet.BlockList()
  blockList.addSubnet("192.168.0.0", 16)
  blockList.addRange("2001:db8::1", "2001:db8::ffff")
  assert.strictEqual(blockList.check("192.168.1.1"), true)
  assert.strictEqual(blockList.check("192.169.1.1"), false)
  assert.strictEqual(blockList.check("2001:db8::2"), true)
  const socketAddress = new builtinNet.SocketAddress({ address: "127.0.0.1", port: 443 })
  assert.strictEqual(socketAddress.family, "ipv4")
  assert.strictEqual(builtinNet.SocketAddress.parse("127.0.0.1:443").port, 443)
  let netPathError
  try {
    new builtinNet.Socket().connect("/tmp/appd.sock")
  } catch (error) {
    netPathError = error
  }
  assert.ok(netPathError instanceof Error)
  for (const connectPrivateAddress of [
    () => builtinNet.connect({ host: "127.0.0.1", port: 443 }),
    () => builtinNet.connect({ host: "app.localhost", port: 443 }),
    () => builtinNet.connect({ host: "example.com", port: 25 }),
    () => builtinTls.connect({ host: "127.0.0.1", port: 443 }),
  ]) {
    let privateAddressError
    try {
      connectPrivateAddress()
    } catch (error) {
      privateAddressError = error
    }
    assert.ok(privateAddressError instanceof Error)
  }
  assert.strictEqual(builtinOs.arch(), "x64")
  assert.strictEqual(builtinOs.cpus().length, 0)
  assert.strictEqual(builtinOs.hostname(), "localhost")
  assert.strictEqual(builtinOs.constants.signals.SIGINT, 2)
  assert.strictEqual(builtinPath.join("appd", "worker"), "appd/worker")
  assert.strictEqual(builtinPath.format({ dir: "/appd", name: "worker", ext: ".js" }), "/appd/worker.js")
  assert.strictEqual(builtinPath.parse("/appd/worker.js").name, "worker")
  assert.strictEqual(builtinPath.matchesGlob("worker.js", "*.js"), true)
  assert.strictEqual(builtinStream, Stream)
  assert.strictEqual(typeof builtinStreamPromises.pipeline, "function")
  assert.strictEqual(typeof builtinStreamWeb.ReadableStream, "function")
  assert.strictEqual(typeof builtinTest, "function")
  const tracker = new MockTracker()
  const functionMock = tracker.fn((value) => `original:${value}`, (value) => `mocked:${value}`, { times: 1 })
  assert.strictEqual(functionMock("first"), "mocked:first")
  assert.strictEqual(functionMock("second"), "original:second")
  const optionOnlyMock = new MockTracker().fn({ times: 1 })
  assert.strictEqual(optionOnlyMock(), undefined)
  const unconfiguredConstructor = new MockTracker().fn()
  assert.ok(new unconfiguredConstructor() instanceof unconfiguredConstructor)
  class MockedConstructor {}
  const constructorMock = new MockTracker().fn(MockedConstructor)
  assert.ok(new constructorMock() instanceof MockedConstructor)
  assert.strictEqual(functionMock.mock.callCount(), 2)
  builtinAssert.deepStrictEqual(functionMock.mock.calls[0].arguments, ["first"])
  const target = { prefix: "appd", value(name) { return `${this.prefix}:${name}` } }
  const originalValue = target.value
  const methodMock = tracker.method(target, "value")
  assert.strictEqual(target.value("bare"), "appd:bare")
  assert.strictEqual(methodMock.mock.calls[0].this, target)
  methodMock.mock.restore()
  assert.strictEqual(target.value, originalValue)
  const scheduledMock = new MockTracker().fn(() => "original")
  scheduledMock.mock.mockImplementationOnce(() => "scheduled", 2)
  builtinAssert.deepStrictEqual([scheduledMock(), scheduledMock(), scheduledMock(), scheduledMock()], ["original", "original", "scheduled", "original"])
  let timesError
  try {
    new MockTracker().fn(() => undefined, { times: 0 })
  } catch (error) {
    timesError = error
  }
  assert.ok(timesError instanceof RangeError)
  let testRunnerError
  try {
    builtinTest()
  } catch (error) {
    testRunnerError = error
  }
  assert.ok(testRunnerError instanceof Error)
  assert.strictEqual(typeof builtinTimers.setTimeout, "function")
  assert.strictEqual(typeof builtinTimersPromises.setTimeout, "function")
  assert.strictEqual(builtinTls.DEFAULT_MIN_VERSION, "TLSv1.2")
  assert.strictEqual(new Uint8Array(builtinTls.convertALPNProtocols(["h2"]))[0], 2)
  assert.strictEqual(builtinUrl.URL, URL)
  assert.strictEqual(builtinUtil.format("appd %s", "bare"), "appd bare")
  assert.strictEqual(builtinUtil.format("%s", undefined), "undefined")
  assert.strictEqual(builtinUtil.format("%i", "1.2"), "1")
  const mime = new builtinUtil.MIMEType('Text/Plain; Charset="UTF=8"')
  assert.strictEqual(mime.essence, "text/plain")
  assert.strictEqual(mime.params.get("charset"), "UTF=8")
  mime.params.set("boundary", "appd=bare")
  assert.strictEqual(mime.toString(), 'text/plain;charset="UTF=8";boundary="appd=bare"')
  const parameters = new builtinUtil.MIMEParams()
  parameters.set("AppD", "bare")
  assert.strictEqual(parameters.get("AppD"), "bare")
  assert.strictEqual(parameters.get("appd"), null)
  assert.strictEqual(parameters.get("appd invalid"), null)
  assert.strictEqual(parameters.has("appd invalid"), false)
  assert.strictEqual(parameters.delete("appd invalid"), undefined)
  const customPromisify = () => Promise.resolve("custom")
  const customCallback = () => {}
  customCallback[builtinUtil.promisify.custom] = customPromisify
  assert.strictEqual(builtinUtil.promisify(customCallback), customPromisify)
  const receiver = { prefix: "appd", callback(value, callback) { callback(null, `${this.prefix}:${value}`) } }
  assert.strictEqual(await builtinUtil.promisify(receiver.callback).call(receiver, "bare"), "appd:bare")
  assert.ok(builtinUtilTypes.isDate(new Date()))
  assert.strictEqual(builtinUtil.isDeepStrictEqual(-0, 0), false)
  assert.strictEqual(builtinSys.format("appd %s", "bare"), "appd bare")
  assert.strictEqual(globalThis.performance, builtinPerfHooks.performance)
  assert.strictEqual(typeof builtinPerfHooks.performance.now(), "number")
  assert.strictEqual(builtinZlib.gunzipSync(builtinZlib.gzipSync("appd")).toString(), "appd")
  const compressed = await transform(new CompressionStream("gzip"), Buffer.from("appd"))
  assert.strictEqual(builtinZlib.gunzipSync(compressed).toString(), "appd")
  assert.strictEqual((await transform(new DecompressionStream("gzip"), compressed)).toString(), "appd")
  const cloneSource = { nested: { value: "appd" } }
  const cloned = structuredClone(cloneSource)
  cloned.nested.value = "changed"
  assert.strictEqual(cloned.nested.value, "changed")
  assert.strictEqual(cloneSource.nested.value, "appd")
  const channel = new MessageChannel()
  const received = new Promise((resolve) => {
    channel.port2.onmessage = (event) => { resolve(event.data) }
  })
  channel.port1.postMessage({ value: "appd" })
  assert.strictEqual((await received).value, "appd")
  await scheduler.wait(1)
  const schedulerController = new AbortController()
  const cancelledWait = scheduler.wait(10_000, { signal: schedulerController.signal })
  schedulerController.abort()
  let schedulerError
  try {
    await cancelledWait
  } catch (error) {
    schedulerError = error
  }
  assert.strictEqual(schedulerError.name, "AbortError")
  assert.strictEqual(processShim.getBuiltinModule("node:process"), processShim)
  
  const builtinShapes = [
    ["node:assert/strict", (module) => typeof module === "function"],
    ["node:buffer", (module) => module.Buffer === Buffer],
    ["node:events", (module) => module.EventEmitter === module],
    ["node:path/posix", (module) => typeof module.join === "function"],
    ["node:path/win32", (module) => typeof module.join === "function"],
    ["node:punycode", (module) => typeof module.toASCII === "function"],
    ["node:querystring", (module) => typeof module.parse === "function"],
    ["node:stream/consumers", (module) => typeof module.text === "function"],
    ["node:_stream_duplex", (module) => module === Stream.Duplex],
    ["node:_stream_passthrough", (module) => module === Stream.PassThrough],
    ["node:_stream_readable", (module) => module === Stream.Readable],
    ["node:_stream_transform", (module) => module === Stream.Transform],
    ["node:_stream_writable", (module) => module === Stream.Writable],
    ["node:string_decoder", (module) => module.StringDecoder === StringDecoder],
  ]
  
  for (const [name, hasExpectedShape] of builtinShapes) {
    assert.ok(hasExpectedShape(processShim.getBuiltinModule(name)), name)
  }
  assert.strictEqual(Buffer.from("appd").toString(), "appd")
  assert.strictEqual(path.resolve("worker"), "/worker")
  assert.strictEqual(stringify(parse("one=1&one=2")), "one=1&one=2")
  assert.strictEqual(new StringDecoder("utf8").write(Buffer.from("appd")), "appd")
  assert.strictEqual(await text([Buffer.from("app"), Buffer.from("d")]), "appd")
  assert.strictEqual(new Uint8Array(await arrayBuffer([Buffer.from("appd")]))[0], 97)
  assert.strictEqual(await setTimeout(1, "appd"), "appd")
  assert.strictEqual(new URL("/worker", "https://appd.local").pathname, "/worker")
  for (const address of [
    { hostname: "127.0.0.1", port: 443 },
    { hostname: "::1", port: 443 },
    { hostname: "10.0.0.1", port: 443 },
    { hostname: "app.localhost", port: 443 },
    { hostname: "example.com", port: 25 },
  ]) {
    let error
    try {
      connect(address)
    } catch (caught) {
      error = caught
    }
    assert.ok(error instanceof Error)
  }

}

async function transform(stream, value) {
  const output = read(stream.readable)
  const writer = stream.writable.getWriter()
  await writer.write(value)
  await writer.close()
  return output
}

async function read(stream) {
  const chunks = []
  const reader = stream.getReader()
  while (true) {
    const result = await reader.read()
    if (result.done) return Buffer.concat(chunks)
    chunks.push(Buffer.from(result.value))
  }
}
