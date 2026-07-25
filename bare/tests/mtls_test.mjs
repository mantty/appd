import fs from "bare-fs";
import tcp from "bare-tcp";
import tls from "bare-tls";
import Buffer from "bare-buffer";
import "./runtime/globals.js";
const { startServer } = await import("./runtime/server.js");

const [mode, certificateDirectory] = Bare.argv.slice(2);
const expectedFailure = mode !== "valid";
const certificates = (name) => fs.readFileSync(`${certificateDirectory}/${name}`);

const serverIdentity = mode === "hostname"
  ? "server-wrong-host.identity.pem"
  : mode === "expired"
    ? "server-expired.identity.pem"
    : "server.identity.pem";

let finished = false;
function finish(code, message) {
  if (finished) return;
  finished = true;
  clearTimeout(timeout);
  if (message) console.error(message);
  Bare.exit(code);
}

function fail(message) {
  finish(1, message);
}

function clientOptions() {
  const options = {
    host: "localhost",
    ca: certificates("ca.pem"),
    rejectUnauthorized: true,
  };
  if (mode !== "missing-client") {
    options.cert = certificates(mode === "wrong-client-ca" ? "client-wrong-ca.pem" : "client.pem");
    options.key = certificates(mode === "wrong-client-ca" ? "client-wrong-ca.key" : "client.key");
  }
  return options;
}

const timeout = setTimeout(() => fail("TLS handshake test timed out"), 5000);
void start();

async function start() {
  try {
    const port = await startServer({
      certificates: {
        ca: `${certificateDirectory}/ca.pem`,
        identity: `${certificateDirectory}/${serverIdentity}`,
      },
      host: "localhost",
      port: 0,
      requireClientCertificate: true,
    });
    const socket = tcp.createConnection(port, "127.0.0.1");
    socket.once("connect", () => {
      socket.write("CONNECT localhost:443 HTTP/1.1\r\nHost: localhost:443\r\n\r\n");
    });
    waitForConnect(socket);
  } catch (error) {
    fail(`appd proxy failed to start: ${error.message}`);
  }
}

function waitForConnect(socket) {
  let buffered = new Uint8Array(0);
  const onData = (chunk) => {
    buffered = append(buffered, chunk);
    const end = headerEnd(buffered);
    if (end < 0) return;
    socket.off("data", onData);
    if (Buffer.from(buffered.subarray(0, end)).toString() !== "HTTP/1.1 200 Connection Established") {
      fail("proxy rejected the app CONNECT request");
      return;
    }
    const remainder = buffered.subarray(end + 4);
    if (remainder.byteLength > 0) socket.unshift(remainder);
    const client = new tls.Socket(socket, clientOptions());
    handleClientSocket(client);
  };
  socket.on("data", onData);
}

function handleClientSocket(client) {
  client.on("error", (error) => {
    if (expectedFailure) finish(0);
    else fail(`valid TLS client failed: ${error.message}`);
  });
  client.on("close", () => {
    if (expectedFailure) finish(0);
    else fail("valid TLS client closed before receiving a response");
  });
  client.on("data", (data) => {
    if (expectedFailure) fail("proxy accepted a client certificate that should have been rejected");
    if (Buffer.from(data).toString().startsWith("HTTP/1.1 204")) finish(0);
  });
  client.on("connect", () => {
    client.write("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
  });
}

function append(left, right) {
  const result = new Uint8Array(left.byteLength + right.byteLength);
  result.set(left);
  result.set(right, left.byteLength);
  return result;
}

function headerEnd(data) {
  for (let index = 0; index <= data.byteLength - 4; index += 1) {
    if (data[index] === 13 && data[index + 1] === 10 && data[index + 2] === 13 && data[index + 3] === 10) {
      return index;
    }
  }
  return -1;
}
