import Buffer from "bare-buffer"
import crypto from "bare-crypto/web"

import { installAesGcm } from "../../../target/runtime-js/crypto.js"

const keepAlive = setInterval(() => {}, 1_000)

void run().then(
  () => {
    clearInterval(keepAlive)
    console.log("crypto checks passed")
  },
  (error) => {
    clearInterval(keepAlive)
    Bare.exitCode = 1
    console.error(error)
  },
)

async function run() {
  installAesGcm(crypto)

  const key = await crypto.subtle.importKey(
    "raw",
    new Uint8Array(32).fill(1),
    "AES-GCM",
    false,
    ["encrypt", "decrypt"],
  )
  const iv = new Uint8Array(12).fill(2)
  const message = Buffer.from("appd")
  const encrypted = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, message)
  const decrypted = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, encrypted)

  if (Buffer.from(decrypted).toString() !== "appd") {
    throw new Error("AES-GCM round trip failed")
  }
}
