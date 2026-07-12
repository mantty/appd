export interface ResponseWriter {
  write(chunk: Uint8Array): boolean;
  once(event: "drain", listener: () => void): void;
  end(): void;
}

export function requestBody(source: AsyncIterable<Uint8Array>): ReadableStream<Uint8Array> {
  const iterator = source[Symbol.asyncIterator]();
  return new ReadableStream({
    async pull(controller) {
      try {
        const result = await iterator.next();
        if (result.done) controller.close();
        else controller.enqueue(result.value);
      } catch (error) {
        controller.error(error);
      }
    },
    async cancel() {
      await iterator.return?.();
    },
  });
}

export async function responseBody(
  body: ReadableStream<Uint8Array>,
  writer: ResponseWriter,
): Promise<void> {
  const reader = body.getReader();
  try {
    let result = await reader.read();
    while (!result.done) {
      if (!writer.write(result.value)) await drain(writer);
      result = await reader.read();
    }
  } finally {
    reader.releaseLock();
  }
  writer.end();
}

function drain(writer: ResponseWriter): Promise<void> {
  return new Promise((resolve) => {
    writer.once("drain", resolve);
  });
}
