export interface ResponseWriter {
  write(chunk: Uint8Array, callback: (error?: Error | null) => void): boolean;
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
      await writeChunk(writer, result.value);
      result = await reader.read();
    }
  } finally {
    reader.releaseLock();
  }
  writer.end();
}

function writeChunk(writer: ResponseWriter, chunk: Uint8Array): Promise<void> {
  if (chunk.byteLength === 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const buffer = Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    writer.write(buffer, (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
}
