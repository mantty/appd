import fs from "bare-fs";
import tls from "bare-tls";

const [mode, certificateDirectory] = Bare.argv.slice(2);
const expectedFailure = mode !== "valid";
const certificates = (name) => fs.readFileSync(`${certificateDirectory}/${name}`);

const serverCertificate = mode === "hostname"
  ? "server-wrong-host.pem"
  : mode === "expired"
    ? "server-expired.pem"
    : "server.pem";
const serverKey = mode === "hostname"
  ? "server-wrong-host.key"
  : mode === "expired"
    ? "server-expired.key"
    : "server.key";

let finished = false;
let server;

function finish(code, message) {
  if (finished) return;
  finished = true;
  clearTimeout(timeout);
  if (message) console.error(message);
  server.close();
  Bare.exit(code);
}

function fail(message) {
  finish(1, message);
}

function clientOptions(port) {
  const options = {
    port,
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

server = tls.createServer({
  cert: certificates(serverCertificate),
  key: certificates(serverKey),
  ca: certificates("ca.pem"),
  rejectUnauthorized: true,
}, (socket) => {
  socket.on("error", (error) => {
    if (expectedFailure) finish(0);
    else fail(`server socket failed: ${error.message}`);
  });
  socket.on("connect", () => {
    if (expectedFailure) {
      fail("server accepted a client certificate that should have been rejected");
      return;
    }
    socket.on("data", (data) => {
      if (data.toString() !== "ping") {
        fail("server received an unexpected payload");
        return;
      }
      socket.end("pong");
    });
  });
});

server.on("error", (error) => fail(`TLS server failed: ${error.message}`));

const timeout = setTimeout(() => fail("TLS handshake test timed out"), 5000);

server.listen(0, "127.0.0.1", () => {
  const client = tls.connect(clientOptions(server.address().port));
  client.on("error", (error) => {
    if (expectedFailure) finish(0);
    else fail(`valid TLS client failed: ${error.message}`);
  });
  client.on("close", () => {
    if (expectedFailure) finish(0);
  });
  client.on("connect", () => {
    if (expectedFailure) {
      client.end("ping");
      return;
    }
    client.on("data", (data) => {
      if (data.toString() === "pong") {
        client.end();
        finish(0);
      } else {
        fail("client received an unexpected payload");
      }
    });
    client.end("ping");
  });
});
