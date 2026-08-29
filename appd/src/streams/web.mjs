export class ReadableStream {
  constructor(source = {}) {
    this.locked = false;
    this.__queue = [];
    this.__readers = [];
    this.__closed = false;
    const controller = {
      enqueue: (value) => {
        if (this.__closed) return;
        const reader = this.__readers.shift();
        if (reader) reader({ done: false, value });
        else this.__queue.push(value);
      },
      close: () => {
        this.__closed = true;
        while (this.__readers.length) this.__readers.shift()({ done: true, value: undefined });
      },
      error: (error) => {
        this.__closed = true;
        while (this.__readers.length) this.__readers.shift()(Promise.reject(error));
      },
    };
    try {
      const started = source.start?.(controller);
      if (started?.catch) started.catch(controller.error);
    } catch (error) {
      controller.error(error);
    }
  }
  getReader() {
    this.locked = true;
    return {
      read: () => {
        if (this.__queue.length) return Promise.resolve({ done: false, value: this.__queue.shift() });
        if (this.__closed) return Promise.resolve({ done: true, value: undefined });
        return new Promise((resolve) => this.__readers.push(resolve));
      },
      releaseLock: () => { this.locked = false; },
    };
  }
  cancel() { this.__closed = true; return Promise.resolve(); }
  tee() { return [new ReadableStream(), new ReadableStream()]; }
}
