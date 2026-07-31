BareKit.IPC.on("data", () => {
  BareKit.IPC.write(
    new Uint8Array([108, 105, 115, 116, 101, 110, 105, 110, 103, 32, 56, 52, 52, 51, 10]),
  )
})
