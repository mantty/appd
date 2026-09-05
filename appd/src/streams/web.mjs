export class ReadableStream {
  constructor(source = {}) {
    const stream = this;
    this.locked = false;
    this.__queue = [];
    this.__readers = [];
    this.__closed = false;
    this.__error = null;
    this.__started = false;
    this.__pulling = false;
    this.__source = source;
    const controller = {
      get desiredSize() { return stream.__queue.length ? 0 : 1; },
      enqueue: (value) => {
        if (this.__closed) return;
        const reader = this.__readers.shift();
        if (reader) reader.resolve({ done: false, value });
        else this.__queue.push(value);
        this.__pullIfNeeded();
      },
      close: () => {
        if (this.__closed) return;
        this.__closed = true;
        this.__drain();
      },
      error: (error) => {
        if (this.__closed) return;
        this.__closed = true;
        this.__error = error;
        this.__queue = [];
        this.__drain();
      },
    };
    this.__controller = controller;
    try {
      const started = source.start?.(controller);
      if (started?.then) {
        started.then(() => {
          this.__started = true;
          this.__drain();
        }, controller.error);
      } else {
        this.__started = true;
        this.__drain();
      }
    } catch (error) {
      controller.error(error);
    }
  }

  getReader() {
    if (this.locked) throw new TypeError("ReadableStream is locked");
    this.locked = true;
    let released = false;
    const assertActive = () => {
      if (released) throw new TypeError("ReadableStream reader was released");
    };
    return {
      read: () => {
        assertActive();
        return this.__read();
      },
      cancel: (reason) => {
        assertActive();
        return this.cancel(reason);
      },
      releaseLock: () => {
        if (!released) {
          released = true;
          this.locked = false;
        }
      },
    };
  }

  __read() {
    if (this.__queue.length) {
      const value = this.__queue.shift();
      this.__pullIfNeeded();
      return Promise.resolve({ done: false, value });
    }
    if (this.__error) return Promise.reject(this.__error);
    if (this.__closed) return Promise.resolve({ done: true, value: undefined });
    const promise = new Promise((resolve, reject) => this.__readers.push({ resolve, reject }));
    this.__pullIfNeeded();
    return promise;
  }

  __drain() {
    while (this.__readers.length && this.__queue.length) {
      this.__readers.shift().resolve({ done: false, value: this.__queue.shift() });
    }
    if (this.__error) {
      while (this.__readers.length) this.__readers.shift().reject(this.__error);
      return;
    }
    if (this.__closed) {
      while (this.__readers.length) this.__readers.shift().resolve({ done: true, value: undefined });
      return;
    }
    this.__pullIfNeeded();
  }

  __pullIfNeeded() {
    if (!this.__started || this.__closed || this.__pulling || !this.__readers.length || this.__queue.length) return;
    const pull = this.__source.pull;
    if (!pull) return;
    this.__pulling = true;
    try {
      const result = pull.call(this.__source, this.__controller);
      if (result?.then) {
        result.then(() => {
          this.__pulling = false;
          this.__drain();
        }, (error) => {
          this.__pulling = false;
          this.__controller.error(error);
        });
      } else {
        this.__pulling = false;
        this.__drain();
      }
    } catch (error) {
      this.__pulling = false;
      this.__controller.error(error);
    }
  }

  cancel(reason) {
    if (this.__closed) return Promise.resolve();
    this.__closed = true;
    this.__queue = [];
    this.__drain();
    try {
      const result = this.__source.cancel?.call(this.__source, reason);
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  async pipeTo(destination, options = {}) {
    const reader = this.getReader();
    const writer = destination.getWriter();
    try {
      while (true) {
        const result = await reader.read();
        if (result.done) break;
        await writer.write(result.value);
      }
      if (!options.preventClose) await writer.close();
    } catch (error) {
      if (!options.preventAbort) await writer.abort(error);
      throw error;
    } finally {
      reader.releaseLock();
      writer.releaseLock();
    }
  }

  pipeThrough(transform, options = {}) {
    this.pipeTo(transform.writable, options);
    return transform.readable;
  }

  tee() {
    throw new TypeError("ReadableStream.tee is not supported");
  }
}

export class WritableStream {
  constructor(sink = {}) {
    this.locked = false;
    this.__sink = sink;
    this.__closed = false;
  }

  getWriter() {
    if (this.locked) throw new TypeError("WritableStream is locked");
    this.locked = true;
    let released = false;
    const assertActive = () => {
      if (released) throw new TypeError("WritableStream writer was released");
    };
    return {
      write: (value) => {
        assertActive();
        return this.__write(value);
      },
      close: () => {
        assertActive();
        return this.__close();
      },
      abort: (reason) => {
        assertActive();
        return this.__abort(reason);
      },
      releaseLock: () => {
        if (!released) {
          released = true;
          this.locked = false;
        }
      },
    };
  }

  __write(value) {
    if (this.__closed) return Promise.reject(new TypeError("WritableStream is closed"));
    try {
      const result = this.__sink.write?.call(this.__sink, value);
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  __close() {
    if (this.__closed) return Promise.resolve();
    this.__closed = true;
    try {
      const result = this.__sink.close?.call(this.__sink);
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  __abort(reason) {
    this.__closed = true;
    try {
      const result = this.__sink.abort?.call(this.__sink, reason);
      return Promise.resolve(result);
    } catch (error) {
      return Promise.reject(error);
    }
  }
}

export class TransformStream {
  constructor(transformer = {}) {
    let controller;
    this.readable = new ReadableStream({
      start: (readableController) => {
        controller = readableController;
        return transformer.start?.call(transformer, controller);
      },
    });
    this.writable = new WritableStream({
      write: (chunk) => {
        try {
          if (transformer.transform) {
            const result = transformer.transform.call(transformer, chunk, controller);
            return result;
          }
          controller.enqueue(chunk);
        } catch (error) {
          controller.error(error);
          return Promise.reject(error);
        }
      },
      close: () => {
        try {
          const result = transformer.flush?.call(transformer, controller);
          if (result?.then) return result.then(() => controller.close());
          controller.close();
        } catch (error) {
          controller.error(error);
          return Promise.reject(error);
        }
      },
      abort: (reason) => controller.error(reason),
    });
  }
}
