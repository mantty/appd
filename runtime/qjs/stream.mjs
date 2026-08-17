import EventEmitter from "./events.mjs";

export class Writable extends EventEmitter {
  write(chunk, encoding, callback) { callback?.(); this.emit("drain"); return true; }
  end(chunk, encoding, callback) { if (chunk !== undefined) this.write(chunk, encoding); callback?.(); this.emit("finish"); return this; }
  destroy(error) { if (error) this.emit("error", error); this.emit("close"); return this; }
}

export class Readable extends EventEmitter {
  constructor(options = {}) { super(); this.readable = true; this.__chunks = []; this.__read = options.read; }
  push(chunk) { if (chunk === null) this.emit("end"); else this.__chunks.push(chunk); return true; }
  read() { return this.__chunks.shift() ?? null; }
  pipe(destination) { this.on("data", (chunk) => destination.write(chunk)); this.once("end", () => destination.end()); return destination; }
  destroy(error) { if (error) this.emit("error", error); this.emit("close"); return this; }
}

export class Duplex extends Readable {
  write(chunk, encoding, callback) { callback?.(); this.emit("data", chunk); return true; }
  end(chunk, encoding, callback) { if (chunk !== undefined) this.write(chunk, encoding); callback?.(); this.emit("finish"); this.emit("end"); return this; }
}

export class Transform extends Duplex {}
export class PassThrough extends Transform {}
export class Stream extends Duplex {}

export default { Writable, Readable, Duplex, Transform, PassThrough, Stream };
